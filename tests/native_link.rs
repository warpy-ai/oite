//! Regression test for #128: a native build of a program that contains a
//! string constant must link on Linux, where the toolchain links PIE
//! executables by default.
//!
//! This only checks that `oitec build` produces a binary that runs. Whether
//! the binary prints the right thing is a separate, known gap in the native
//! runtime (#127); once that is fixed, move this program into `tests/lang/`
//! with a `// backends: vm aot` directive instead.

#![cfg(target_os = "linux")]

use std::fs;
use std::path::Path;
use std::process::Command;

const OITEC: &str = env!("CARGO_BIN_EXE_oitec");

#[test]
fn hello_world_links_as_pie() {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("native_link");
    fs::create_dir_all(&dir).unwrap();
    let src = dir.join("hello.ot");
    let exe = dir.join("hello");
    fs::write(&src, "console.log(\"Hello, world!\");\n").unwrap();
    let _ = fs::remove_file(&exe);

    let build = Command::new(OITEC)
        .args(["build", "hello.ot", "--release", "-o", "hello"])
        .current_dir(&dir)
        .output()
        .expect("failed to spawn oitec");
    assert!(
        build.status.success(),
        "oitec build failed:\n{}{}",
        String::from_utf8_lossy(&build.stdout),
        String::from_utf8_lossy(&build.stderr)
    );

    let run = Command::new(&exe).output().expect("failed to run binary");
    assert!(
        run.status.success(),
        "native binary exited with {}",
        run.status
    );
}
