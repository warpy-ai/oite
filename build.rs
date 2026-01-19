//! Build script - currently minimal
//!
//! The runtime library is built on-demand by the AOT compiler
//! to avoid hanging during `cargo build`.

fn main() {
    // Re-run if runtime sources change (so AOT compiler knows to rebuild)
    println!("cargo:rerun-if-changed=src/runtime/mod.rs");
    println!("cargo:rerun-if-changed=src/runtime/abi.rs");
    println!("cargo:rerun-if-changed=src/runtime/heap.rs");
    println!("cargo:rerun-if-changed=src/runtime/stubs.rs");
    println!("cargo:rerun-if-changed=src/lib.rs");
}
