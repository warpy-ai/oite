//! Golden-output tests for the Oite language.
//!
//! Every `tests/lang/**/*.ot` file is a program. Next to it lives the expected
//! result:
//!
//! - `name.out`  — exact expected stdout; the program must exit 0.
//! - `name.err`  — the program must exit non-zero, and stderr must contain
//!   every non-empty line of this file (substring match, in order).
//!
//! Programs run in a subprocess through the real `oitec` binary, so a VM panic
//! or abort is reported as a test failure instead of taking the harness down.
//!
//! Directives (comment lines at the top of the `.ot` file):
//!
//! - `// backends: vm jit aot` — run the program on each listed backend
//!   (`oitec file`, `oitec jit file`, `oitec build file`) and hold every one
//!   to the same `.out`. The expectation files are the reference;
//!   no backend is. Default: `vm` only. Every test that the native backends
//!   can run should list `jit aot`; the goal is every test on `aot`, Oite's
//!   production target.
//!
//! Set `OITE_BLESS=1` to (re)write `.out` files from the current VM output.
//! Only do this after checking the new output by hand: it records what the
//! VM does, not what is correct.
//!
//! Set `OITE_GOLDEN_FILTER=substr` to run only matching files.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const OITEC: &str = env!("CARGO_BIN_EXE_oitec");

#[derive(Debug, Clone, Copy, PartialEq)]
enum Backend {
    Vm,
    Jit,
    Aot,
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(&path, out);
        } else if path.extension().is_some_and(|e| e == "ot") {
            out.push(path);
        }
    }
}

fn backends(source: &str) -> Vec<Backend> {
    for line in source.lines() {
        let line = line.trim();
        if !line.starts_with("//") {
            break;
        }
        if let Some(rest) = line
            .trim_start_matches("//")
            .trim()
            .strip_prefix("backends:")
        {
            return rest
                .split_whitespace()
                .map(|b| match b {
                    "vm" => Backend::Vm,
                    "jit" => Backend::Jit,
                    "aot" => Backend::Aot,
                    other => panic!("unknown backend in directive: {other}"),
                })
                .collect();
        }
    }
    vec![Backend::Vm]
}

fn run(backend: Backend, file: &Path, scratch: &Path) -> Result<Output, String> {
    match backend {
        // Run from a scratch directory, the way a user program runs: no
        // std/prelude.ot from the repository root is picked up.
        Backend::Vm => Command::new(OITEC)
            .arg(file)
            .current_dir(scratch)
            .output()
            .map_err(|e| format!("failed to spawn oitec: {e}")),
        Backend::Jit => Command::new(OITEC)
            .arg("jit")
            .arg(file)
            .current_dir(scratch)
            .output()
            .map_err(|e| format!("failed to spawn oitec jit: {e}")),
        Backend::Aot => {
            let exe = scratch.join(file.file_stem().unwrap());
            let build = Command::new(OITEC)
                .arg("build")
                .arg(file)
                .arg("--release")
                .arg("-o")
                .arg(&exe)
                .current_dir(scratch)
                .output()
                .map_err(|e| format!("failed to spawn oitec build: {e}"))?;
            // A failed build stands for the program's result: `.err` tests
            // check its message, `.out` tests report it as a failure.
            if !build.status.success() {
                return Ok(build);
            }
            Command::new(&exe)
                .output()
                .map_err(|e| format!("failed to run native binary: {e}"))
        }
    }
}

fn check(file: &Path, scratch: &Path) -> Result<(), String> {
    let source = fs::read_to_string(file).map_err(|e| e.to_string())?;
    let out_path = file.with_extension("out");
    let err_path = file.with_extension("err");
    let bless = std::env::var_os("OITE_BLESS").is_some();

    for backend in backends(&source) {
        let output = run(backend, file, scratch).map_err(|e| format!("[{backend:?}] {e}"))?;
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        if err_path.exists() {
            let expected = fs::read_to_string(&err_path).map_err(|e| e.to_string())?;
            if output.status.success() {
                return Err(format!(
                    "[{backend:?}] expected a non-zero exit, got success\nstdout:\n{stdout}"
                ));
            }
            let mut rest = stderr.as_str();
            for want in expected.lines().map(str::trim).filter(|l| !l.is_empty()) {
                match rest.find(want) {
                    Some(i) => rest = &rest[i + want.len()..],
                    None => {
                        return Err(format!(
                            "[{backend:?}] stderr is missing {want:?}\nstderr:\n{stderr}"
                        ));
                    }
                }
            }
            continue;
        }

        if bless && backend == Backend::Vm {
            fs::write(&out_path, &stdout).map_err(|e| e.to_string())?;
        }
        let expected = fs::read_to_string(&out_path)
            .map_err(|_| format!("missing {} (run with OITE_BLESS=1)", out_path.display()))?;

        if !output.status.success() {
            return Err(format!(
                "[{backend:?}] exited with {}\nstdout:\n{stdout}\nstderr:\n{stderr}",
                output.status
            ));
        }
        if stdout != expected {
            return Err(format!(
                "[{backend:?}] stdout mismatch\n--- expected\n{expected}--- actual\n{stdout}--- stderr\n{stderr}"
            ));
        }
    }
    Ok(())
}

#[test]
fn golden() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/lang");
    let mut files = Vec::new();
    collect(&root, &mut files);
    files.sort();
    assert!(
        !files.is_empty(),
        "no golden tests found in {}",
        root.display()
    );

    let filter = std::env::var("OITE_GOLDEN_FILTER").ok();
    let scratch = Path::new(env!("CARGO_TARGET_TMPDIR")).join("golden");
    fs::create_dir_all(&scratch).unwrap();

    let mut failures = Vec::new();
    let mut ran = 0;
    for file in &files {
        let rel = file.strip_prefix(&root).unwrap().display().to_string();
        if filter.as_deref().is_some_and(|f| !rel.contains(f)) {
            continue;
        }
        ran += 1;
        if let Err(msg) = check(file, &scratch) {
            failures.push(format!("=== {rel}\n{msg}"));
        }
    }

    if !failures.is_empty() {
        panic!(
            "{} of {} golden tests failed:\n\n{}",
            failures.len(),
            ran,
            failures.join("\n\n")
        );
    }
    eprintln!("{ran} golden tests passed");
}
