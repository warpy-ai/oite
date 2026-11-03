//! The native runtime library, embedded in `oitec`.
//!
//! `build.rs` compiles `src/runtime/{abi,heap,stubs}.rs` into a static
//! library. It is embedded here so that `oitec build` can link native
//! executables without a Rust toolchain or the Oite source tree.

use std::path::{Path, PathBuf};

use crate::backend::BackendError;

static RUNTIME_LIB: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/liboite_rt.a"));

/// System libraries the runtime needs, as reported by rustc.
static NATIVE_LIBS: &str = include_str!(concat!(env!("OUT_DIR"), "/oite_rt_native_libs.txt"));

/// Write the runtime archive into `dir` and return its path.
pub fn write_to(dir: &Path) -> Result<PathBuf, BackendError> {
    std::fs::create_dir_all(dir).map_err(|e| {
        BackendError::AotError(format!("failed to create {}: {}", dir.display(), e))
    })?;
    let path = dir.join("liboite_rt.a");
    std::fs::write(&path, RUNTIME_LIB).map_err(|e| {
        BackendError::AotError(format!("failed to write {}: {}", path.display(), e))
    })?;
    Ok(path)
}

/// Linker arguments for the system libraries the runtime depends on.
pub fn native_libs() -> impl Iterator<Item = &'static str> {
    NATIVE_LIBS.split_whitespace()
}
