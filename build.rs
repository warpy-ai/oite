//! Build script: compiles the native runtime library.
//!
//! `oitec build` links every native executable against the Oite runtime
//! (`src/runtime/{abi,builtins,heap,intrinsics}.rs` and `src/runtime/stubs/`). Those files are compiled here, as a
//! standalone static library rooted at `src/runtime/native_rt.rs`, and the
//! archive is embedded into `oitec` (see `backend::llvm::runtime_lib`). Native
//! binaries therefore need no Rust toolchain or source tree at build time.
//!
//! `rustc` is invoked directly rather than through a nested `cargo build`,
//! which would contend for the target-directory lock. The runtime has no
//! dependencies beyond `std`, so a single `rustc` call is enough.

use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let rustc = env::var("RUSTC").unwrap_or_else(|_| "rustc".into());
    let target = env::var("TARGET").unwrap();

    let root = manifest.join("src/runtime/native_rt.rs");
    for file in [
        "native_rt.rs",
        "abi.rs",
        "builtins.rs",
        "heap.rs",
        "intrinsics.rs",
        // A directory: cargo watches every file in it.
        "object",
        "stubs",
    ] {
        println!(
            "cargo:rerun-if-changed={}",
            manifest.join("src/runtime").join(file).display()
        );
    }

    let output = Command::new(&rustc)
        .arg("--crate-name")
        .arg("oite_rt")
        .arg("--crate-type")
        .arg("staticlib")
        .arg("--edition")
        .arg("2024")
        .arg("--target")
        .arg(&target)
        .arg("-C")
        .arg("opt-level=3")
        // One unit: the runtime's small helpers are spread over modules
        // and must inline into each other.
        .arg("-C")
        .arg("codegen-units=1")
        .arg("-C")
        .arg("panic=abort")
        .arg("-C")
        .arg("debuginfo=0")
        // Native executables are linked as PIE on Linux (#128).
        .arg("-C")
        .arg("relocation-model=pic")
        .arg("--print")
        .arg("native-static-libs")
        .arg("--out-dir")
        .arg(&out_dir)
        .arg(&root)
        .output()
        .expect("failed to run rustc for the native runtime");

    let stderr = String::from_utf8_lossy(&output.stderr);
    if !output.status.success() {
        panic!("building the native runtime failed:\n{stderr}");
    }

    // System libraries the static library needs at link time, as reported by
    // rustc, e.g. "-lgcc_s -lutil -lrt -lpthread -lm -ldl -lc".
    let native_libs = stderr
        .lines()
        .find_map(|l| l.split("native-static-libs:").nth(1))
        .map(str::trim)
        .unwrap_or("")
        .to_string();
    std::fs::write(out_dir.join("oite_rt_native_libs.txt"), native_libs).unwrap();
}
