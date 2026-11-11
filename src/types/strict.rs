//! The switch for strict types (`--strict-types`): what the checker
//! finds is an error, and values are checked against type annotations
//! when the program runs (docs/docs/types.md, section 3).

/// Whether type annotations are checked when the program runs
/// (`--strict-types`).
static STRICT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Turn the run-time checks of type annotations on.
pub fn set_strict(on: bool) {
    STRICT.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// Whether type annotations are checked when the program runs: with
/// `--strict-types`, or when `OITE_STRICT_TYPES` is set to anything but
/// `0`.
pub fn strict() -> bool {
    STRICT.load(std::sync::atomic::Ordering::Relaxed)
        || std::env::var("OITE_STRICT_TYPES").is_ok_and(|v| !v.is_empty() && v != "0")
}
