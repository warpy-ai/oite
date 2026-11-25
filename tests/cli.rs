//! The `oitec` command line itself, apart from compiling programs.

use std::process::Command;

const OITEC: &str = env!("CARGO_BIN_EXE_oitec");

/// `--version` prints the crate version, which the release workflow checks
/// against the tag being released.
#[test]
fn version_flag_prints_the_crate_version() {
    for flag in ["--version", "-V"] {
        let out = Command::new(OITEC).arg(flag).output().expect("run oitec");
        assert!(out.status.success(), "{flag} exited with {}", out.status);
        assert_eq!(
            String::from_utf8_lossy(&out.stdout),
            format!("oitec {}\n", env!("CARGO_PKG_VERSION")),
            "{flag}"
        );
        assert!(out.stderr.is_empty(), "{flag} wrote to stderr");
    }
}
