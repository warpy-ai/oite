//! Static linking support for LLVM AOT compilation
//!
//! This module provides functions to link object files with the runtime library
//! using external linkers (clang/ld).

use std::path::{Path, PathBuf};
use std::process::Command;

use super::super::{BackendError, LtoMode, aot::OutputFormat};

/// Link object files with runtime library to create an executable or library
pub fn link_object_files(
    objects: &[PathBuf],
    output: &Path,
    format: OutputFormat,
    runtime_lib: Option<&Path>,
) -> Result<(), BackendError> {
    link_object_files_with_lto(objects, output, format, runtime_lib, LtoMode::None)
}

/// Link object files with runtime library, supporting LTO
pub fn link_object_files_with_lto(
    objects: &[PathBuf],
    output: &Path,
    format: OutputFormat,
    runtime_lib: Option<&Path>,
    lto_mode: LtoMode,
) -> Result<(), BackendError> {
    // Detect linker (prefer clang, fall back to cc/ld)
    let linker = detect_linker()?;

    let mut cmd = Command::new(&linker);

    // Add LTO flags if LTO is enabled
    if lto_mode != LtoMode::None {
        // Pass -flto flag to linker
        if linker.contains("clang") || linker.contains("gcc") {
            match lto_mode {
                LtoMode::Thin => {
                    cmd.arg("-flto=thin");
                }
                LtoMode::Full => {
                    cmd.arg("-flto");
                }
                LtoMode::None => {
                    // Should not happen, but handle gracefully
                }
            }
        }
    }

    // #region agent log
    // Log object files and runtime library being linked
    let obj_files_str: Vec<String> = objects.iter().map(|p| p.display().to_string()).collect();
    let runtime_lib_str = runtime_lib.map(|p| p.display().to_string());
    let log_msg = format!(
        r#"{{"sessionId":"debug-session","runId":"run1","hypothesisId":"C","location":"llvm/linker.rs:52","message":"Preparing linker command","data":{{"linker":"{}","object_files":{:?},"runtime_lib":{:?},"format":"{:?}"}},"timestamp":{}}}"#,
        linker,
        obj_files_str,
        runtime_lib_str,
        format,
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    );
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("/Volumes/WD_2TB/warpy/script/.cursor/debug.log")
    {
        use std::io::Write;
        let _ = writeln!(f, "{}", log_msg);
    }
    // #endregion

    // Add object files
    for obj in objects {
        // #region agent log
        // Check if object file exists and has main symbol
        if obj.exists() {
            if let Ok(nm_output) = std::process::Command::new("nm").arg("-g").arg(obj).output() {
                let symbols = String::from_utf8_lossy(&nm_output.stdout);
                let has_main = symbols.contains("main") || symbols.contains("_main");
                let log_msg = format!(
                    r#"{{"sessionId":"debug-session","runId":"run1","hypothesisId":"D","location":"llvm/linker.rs:60","message":"Object file symbols check","data":{{"obj_file":"{}","has_main":{},"symbols_preview":"{}"}},"timestamp":{}}}"#,
                    obj.display(),
                    has_main,
                    symbols.lines().take(5).collect::<Vec<_>>().join(";"),
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_millis()
                );
                if let Ok(mut f) = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open("/Volumes/WD_2TB/warpy/script/.cursor/debug.log")
                {
                    use std::io::Write;
                    let _ = writeln!(f, "{}", log_msg);
                }
            }
        } else {
            let log_msg = format!(
                r#"{{"sessionId":"debug-session","runId":"run1","hypothesisId":"E","location":"llvm/linker.rs:75","message":"Object file missing","data":{{"obj_file":"{}"}},"timestamp":{}}}"#,
                obj.display(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_millis()
            );
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open("/Volumes/WD_2TB/warpy/script/.cursor/debug.log")
            {
                use std::io::Write;
                let _ = writeln!(f, "{}", log_msg);
            }
        }
        // #endregion
        cmd.arg(obj);
    }

    // Add runtime library if provided
    // Use -all_load on macOS to ensure ALL symbols from ALL archives are included
    if let Some(lib) = runtime_lib {
        if lib.exists() {
            #[cfg(target_os = "macos")]
            {
                if linker.contains("clang") {
                    // -all_load forces loading all symbols from all archives
                    cmd.arg("-Wl,-all_load").arg(lib);
                } else {
                    cmd.arg(lib);
                }
            }
            #[cfg(not(target_os = "macos"))]
            {
                cmd.arg(lib);
            }

            // Runtime uses Vec/String from std, which should be statically linked in libruntime.a
            // No additional libraries needed since we removed HashMap dependency
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

    // #region agent log
    // Log the exact linker command
    let cmd_str = format!("{:?}", cmd);
    let log_msg = format!(
        r#"{{"sessionId":"debug-session","runId":"run1","hypothesisId":"F","location":"llvm/linker.rs:85","message":"Executing linker command","data":{{"command":"{}"}},"timestamp":{}}}"#,
        cmd_str,
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    );
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("/Volumes/WD_2TB/warpy/script/.cursor/debug.log")
    {
        use std::io::Write;
        let _ = writeln!(f, "{}", log_msg);
    }
    // #endregion

    // Execute linker
    let status = cmd
        .status()
        .map_err(|e| BackendError::Llvm(format!("Failed to execute linker {}: {}", linker, e)))?;

    if !status.success() {
        // #region agent log
        // Log linker failure
        let log_msg = format!(
            r#"{{"sessionId":"debug-session","runId":"run1","hypothesisId":"G","location":"llvm/linker.rs:95","message":"Linker failed","data":{{"exit_code":{:?}}},"timestamp":{}}}"#,
            status.code(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis()
        );
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open("/Volumes/WD_2TB/warpy/script/.cursor/debug.log")
        {
            use std::io::Write;
            let _ = writeln!(f, "{}", log_msg);
        }
        // #endregion
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

    let status = cmd
        .status()
        .map_err(|e| BackendError::Llvm(format!("Failed to execute ar: {}", e)))?;

    if !status.success() {
        return Err(BackendError::Llvm(format!(
            "ar failed with exit code: {:?}",
            status.code()
        )));
    }

    Ok(())
}

/// Link using rustc (handles Rust dependencies automatically)
fn link_with_rustc(
    objects: &[PathBuf],
    output: &Path,
    format: OutputFormat,
    runtime_lib: Option<&Path>,
    _lto_mode: LtoMode,
) -> Result<(), BackendError> {
    let mut cmd = Command::new("rustc");

    // Add object files
    for obj in objects {
        cmd.arg(obj);
    }

    // Add runtime library
    if let Some(lib) = runtime_lib {
        if lib.exists() {
            cmd.arg(lib);
        }
    }

    // Set output
    match format {
        OutputFormat::Executable => {
            cmd.arg("-o").arg(output);
            // Link as executable - rustc will handle all Rust dependencies
            cmd.arg("--crate-type=bin");
        }
        OutputFormat::SharedLib => {
            cmd.arg("-o").arg(output);
            cmd.arg("--crate-type=cdylib");
        }
        _ => {
            return Err(BackendError::Llvm(
                "rustc linker only supports Executable and SharedLib formats".into(),
            ));
        }
    }

    let status = cmd
        .status()
        .map_err(|e| BackendError::Llvm(format!("Failed to execute rustc linker: {}", e)))?;

    if !status.success() {
        return Err(BackendError::Llvm(format!(
            "rustc linker failed with exit code: {:?}",
            status.code()
        )));
    }

    Ok(())
}

/// Detect available linker on the system
/// For Rust runtime libraries, prefer rustc which handles std linking automatically
fn detect_linker() -> Result<String, BackendError> {
    // If we have Rust runtime dependencies, use rustc for linking
    // This ensures libstd and other Rust libraries are linked correctly
    if Command::new("rustc").arg("--version").output().is_ok() {
        // Check if we're linking a Rust runtime library
        // (This will be determined by the caller)
        // For now, we'll use rustc when available
    }

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
