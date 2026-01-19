//! Static linking support for LLVM AOT compilation
//!
//! This module provides functions to link object files with the runtime library
//! using external linkers (clang/ld).

use std::path::{Path, PathBuf};
use std::process::Command;

use super::super::{BackendError, aot::OutputFormat};

/// Link object files with runtime library to create an executable or library
pub fn link_object_files(
    objects: &[PathBuf],
    output: &Path,
    format: OutputFormat,
    runtime_lib: Option<&Path>,
) -> Result<(), BackendError> {
    // Detect linker (prefer clang, fall back to cc/ld)
    let linker = detect_linker()?;

    let mut cmd = Command::new(&linker);

    // Add object files
    for obj in objects {
        cmd.arg(obj);
    }

    // Add runtime library if provided
    if let Some(lib) = runtime_lib {
        if lib.exists() {
            cmd.arg(lib);
        }
    }

    // Set output format
    match format {
        OutputFormat::Executable => {
            cmd.arg("-o").arg(output);
        }
        OutputFormat::StaticLib => {
            return create_static_library(objects, output);
        }
        OutputFormat::SharedLib => {
            if linker.contains("clang") || linker.contains("gcc") {
                cmd.args(&["-shared", "-o"]).arg(output);
            } else {
                cmd.args(&["-shared", "-o"]).arg(output);
            }
        }
        OutputFormat::Object => {
            // No linking needed for object files
            return Ok(());
        }
    }

    // Execute linker
    let status = cmd.status().map_err(|e| {
        BackendError::Llvm(format!("Failed to execute linker {}: {}", linker, e))
    })?;

    if !status.success() {
        return Err(BackendError::Llvm(format!(
            "Linker failed with exit code: {:?}",
            status.code()
        )));
    }

    Ok(())
}

/// Create a static library from object files
pub fn create_static_library(objects: &[PathBuf], output: &Path) -> Result<(), BackendError> {
    // Use ar to create static library
    let mut cmd = Command::new("ar");
    cmd.arg("rcs").arg(output);

    for obj in objects {
        cmd.arg(obj);
    }

    let status = cmd.status().map_err(|e| {
        BackendError::Llvm(format!("Failed to execute ar: {}", e))
    })?;

    if !status.success() {
        return Err(BackendError::Llvm(format!(
            "ar failed with exit code: {:?}",
            status.code()
        )));
    }

    Ok(())
}

/// Detect available linker on the system
fn detect_linker() -> Result<String, BackendError> {
    // Try clang first (works on all platforms)
    if Command::new("clang").arg("--version").output().is_ok() {
        return Ok("clang".to_string());
    }

    // Try gcc
    if Command::new("gcc").arg("--version").output().is_ok() {
        return Ok("gcc".to_string());
    }

    // Try cc (common on Unix)
    if Command::new("cc").arg("--version").output().is_ok() {
        return Ok("cc".to_string());
    }

    Err(BackendError::Llvm(
        "No suitable linker found (tried clang, gcc, cc)".into(),
    ))
}
