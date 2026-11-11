//! Tests for the type checker (docs/docs/types.md).
//!
//! Every `tests/types/*.ot` file is a program, and `name.expected` lists
//! exactly what `oitec check` must report for it, one finding per line as
//! `line:col: message`. An empty file means the program is accepted.
//!
//! Set `OITE_BLESS=1` to rewrite the `.expected` files from what the
//! checker reports now. Only do this after reading the new findings.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const OITEC: &str = env!("CARGO_BIN_EXE_oitec");

fn programs() -> Vec<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/types");
    let mut files: Vec<PathBuf> = fs::read_dir(&dir)
        .expect("tests/types")
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "ot"))
        .collect();
    files.sort();
    files
}

/// What `oitec check` reports for `file`, as `line:col: message` lines,
/// and whether it exited with success.
fn check(file: &Path) -> (Vec<String>, bool) {
    let output = Command::new(OITEC)
        .arg("check")
        .arg(file)
        .output()
        .expect("run oitec");
    let stderr = String::from_utf8_lossy(&output.stderr);
    let prefix = format!("{}:", file.display());
    let found = stderr
        .lines()
        .filter_map(|line| line.strip_prefix(&prefix))
        .filter_map(|rest| {
            let (position, message) = rest.split_once(": type error: ")?;
            Some(format!("{position}: {message}"))
        })
        .collect();
    (found, output.status.success())
}

#[test]
fn checker_findings() {
    let bless = std::env::var("OITE_BLESS").is_ok();
    let mut failures = Vec::new();
    let files = programs();
    assert!(!files.is_empty(), "no programs in tests/types");
    for file in files {
        let expected_path = file.with_extension("expected");
        let (found, success) = check(&file);
        if bless {
            let mut text = found.join("\n");
            if !text.is_empty() {
                text.push('\n');
            }
            fs::write(&expected_path, text).expect("write expected");
            continue;
        }
        let expected: Vec<String> = fs::read_to_string(&expected_path)
            .unwrap_or_else(|_| panic!("missing {}", expected_path.display()))
            .lines()
            .map(str::to_string)
            .collect();
        let name = file.file_name().unwrap().to_string_lossy().into_owned();
        if found != expected {
            let missing: Vec<&String> = expected.iter().filter(|e| !found.contains(e)).collect();
            let extra: Vec<&String> = found.iter().filter(|f| !expected.contains(f)).collect();
            failures.push(format!(
                "{name}:\n  not reported: {missing:#?}\n  reported but not expected: {extra:#?}"
            ));
        }
        // `check` fails exactly when it reports something.
        if success != expected.is_empty() {
            failures.push(format!(
                "{name}: `oitec check` exited with success = {success}, with {} findings expected",
                expected.len()
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

/// For now the checker's findings are warnings: a program with a wrong
/// annotation still runs, and says what is wrong on stderr.
#[test]
fn findings_are_warnings_when_running() {
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/types/issue_124.ot");
    let output = Command::new(OITEC).arg(&file).output().expect("run oitec");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "the program should run: {stderr}");
    assert!(stdout.contains("helloworld"), "stdout: {stdout}");
    assert!(
        stderr.contains("warning: Type 'number' is not assignable to type 'string'."),
        "stderr: {stderr}"
    );
    assert!(
        stderr.contains(
            "warning: Argument of type 'string' is not assignable to parameter of type 'number'."
        ),
        "stderr: {stderr}"
    );
}

fn collect_programs(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            // `_lib/`, `_rolls/`: modules other programs import.
            if !name.starts_with('_') {
                collect_programs(&path, out);
            }
        } else if name.ends_with(".ot") {
            out.push(path);
        }
    }
}

/// The Oite code in this repository keeps its promises: the checker finds
/// nothing in the self-hosted compiler, the standard library, the
/// examples, the benchmarks or the language tests. (`tests/types` holds
/// the programs that are wrong on purpose.) With `OITE_CORE_ROLLS`, the
/// core rolls are held to the same.
#[test]
fn existing_code_has_no_findings() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    for dir in [
        "compiler",
        "bootstrap",
        "std",
        "examples",
        "benches/perf",
        "tests/lang",
        "tests/language",
        "tests/compiler",
        "tests/integration",
        "tests/core_rolls",
    ] {
        collect_programs(&root.join(dir), &mut files);
    }
    if let Ok(rolls) = std::env::var("OITE_CORE_ROLLS") {
        collect_programs(Path::new(&rolls), &mut files);
    }
    files.sort();
    assert!(files.len() > 100, "found only {} programs", files.len());
    let mut findings = Vec::new();
    for file in &files {
        let (found, _) = check(file);
        for line in found {
            findings.push(format!("{}:{line}", file.display()));
        }
    }
    assert!(
        findings.is_empty(),
        "the type checker reports:\n{}",
        findings.join("\n")
    );
}
