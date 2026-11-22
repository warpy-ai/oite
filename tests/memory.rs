//! Peak memory of a long-running program on the VM (#123).
//!
//! The program runs in a subprocess through the real `oitec` binary; its
//! peak resident set size comes from `wait4`, which reports that child
//! alone.

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};

const OITEC: &str = env!("CARGO_BIN_EXE_oitec");

/// The #123 loop: 3 million objects, each garbage one iteration later.
const LOOP: &str = "let n = 0;
for (let i = 0; i < 3000000; i++) { let o = { a: i, b: i }; n = n + o.a; }
console.log(n);
";

/// Measured on Apple Silicon: 50 MB, of which 14 MB is `oitec` running
/// `console.log(1)` and the rest is up to 100,000 dead objects (about
/// 360 bytes each) between two collections. Without the collector the
/// loop peaks at 1,052 MB. 128 MiB leaves 2.5x headroom for other
/// allocators and platforms, and is an eighth of the uncollected peak.
const PEAK_LIMIT: u64 = 128 << 20;

/// Run `oitec file`; its stdout and peak resident set size in bytes.
#[allow(clippy::zombie_processes, reason = "wait4 reaps the child")]
fn run_measured(file: &Path) -> (String, u64) {
    let mut child = Command::new(OITEC)
        .arg(file)
        .current_dir(file.parent().unwrap())
        .env_remove("OITE_GC_STRESS")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("failed to spawn oitec");
    let mut stdout = String::new();
    child
        .stdout
        .take()
        .unwrap()
        .read_to_string(&mut stdout)
        .unwrap();

    let mut status = 0;
    let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
    let pid = unsafe { libc::wait4(child.id() as libc::pid_t, &mut status, 0, &mut usage) };
    assert_eq!(pid, child.id() as libc::pid_t, "wait4 failed");
    assert!(
        libc::WIFEXITED(status) && libc::WEXITSTATUS(status) == 0,
        "oitec failed: status {status}"
    );
    // Bytes on macOS, kilobytes on Linux.
    let unit = if cfg!(target_os = "macos") { 1 } else { 1024 };
    (stdout, usage.ru_maxrss as u64 * unit)
}

#[test]
fn allocation_loop_peak_memory_is_bounded() {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("memory");
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("loop.ot");
    std::fs::write(&file, LOOP).unwrap();

    let (stdout, peak) = run_measured(&file);
    assert_eq!(stdout, "4499998500000\n");
    assert!(
        peak < PEAK_LIMIT,
        "peak RSS {} MiB, limit {} MiB",
        peak >> 20,
        PEAK_LIMIT >> 20
    );
}
