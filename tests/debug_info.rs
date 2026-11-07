//! `oitec build --debug` emits DWARF: a line table for the Oite source and
//! a subprogram per function, so debuggers stop on Oite lines (#125).

use std::fs;
use std::path::Path;
use std::process::Command;

const OITEC: &str = env!("CARGO_BIN_EXE_oitec");

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

#[test]
fn debug_build_has_line_info() {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("debug_info");
    fs::create_dir_all(&dir).unwrap();
    let src = dir.join("dbg_program.ot");
    fs::write(
        &src,
        "function twice_value(n) {\n  return n * 2;\n}\nconsole.log(twice_value(21));\n",
    )
    .unwrap();
    let exe = dir.join("dbg_program");
    let build = Command::new(OITEC)
        .arg("build")
        .arg(&src)
        .arg("--debug")
        .arg("-o")
        .arg(&exe)
        .current_dir(&dir)
        .output()
        .expect("run oitec build");
    assert!(
        build.status.success(),
        "build failed: {}",
        String::from_utf8_lossy(&build.stderr)
    );

    let run = Command::new(&exe).output().expect("run the binary");
    assert_eq!(String::from_utf8_lossy(&run.stdout), "42\n");

    // The LLVM IR of the same build carries the debug metadata.
    let ir = Command::new(OITEC)
        .arg("build")
        .arg(&src)
        .arg("--debug")
        .arg("--emit-llvm")
        .arg("-o")
        .arg(&exe)
        .current_dir(&dir)
        .output()
        .expect("run oitec build --emit-llvm");
    assert!(ir.status.success());
    let ll = fs::read(exe.with_extension("ll")).expect("the .ll file");
    assert!(contains(&ll, b"!DICompileUnit("), "no compile unit");
    assert!(
        contains(&ll, b"!DIFile(filename: \"dbg_program.ot\""),
        "the source file is not described"
    );
    assert!(
        contains(&ll, b"!DISubprogram(name: \"twice_value\""),
        "no subprogram named as in the source"
    );
    // `return n * 2;` is line 2; its code starts at `n`, column 10.
    assert!(
        contains(&ll, b"!DILocation(line: 2, column: 10"),
        "no location for line 2"
    );
}

/// A release build has no debug metadata (only exception trace sites).
#[test]
fn release_build_has_no_debug_metadata() {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("debug_info_release");
    fs::create_dir_all(&dir).unwrap();
    let src = dir.join("rel_program.ot");
    fs::write(&src, "console.log(1 + 1);\n").unwrap();
    let exe = dir.join("rel_program");
    let ir = Command::new(OITEC)
        .arg("build")
        .arg(&src)
        .arg("--release")
        .arg("--emit-llvm")
        .arg("-o")
        .arg(&exe)
        .current_dir(&dir)
        .output()
        .expect("run oitec build --emit-llvm");
    assert!(ir.status.success());
    let ll = fs::read(exe.with_extension("ll")).expect("the .ll file");
    assert!(!contains(&ll, b"!DICompileUnit("));
}
