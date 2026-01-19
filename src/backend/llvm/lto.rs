//! Link-Time Optimization (LTO) pipeline
//!
//! This module orchestrates ThinLTO and Full LTO compilation using LLVM tools.
//! It handles bitcode linking, optimization, and code generation.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::backend::{BackendError, LtoMode, OptLevel};

/// Find LLVM tools directory
fn find_llvm_tools() -> Result<PathBuf, BackendError> {
    // Try LLVM_SYS_180_PREFIX first
    if let Ok(prefix) = std::env::var("LLVM_SYS_180_PREFIX") {
        let bin_dir = PathBuf::from(prefix).join("bin");
        if bin_dir.exists() {
            return Ok(bin_dir);
        }
    }

    // Fall back to system PATH
    // Check if llvm-link is available
    if Command::new("llvm-link").arg("--version").output().is_ok() {
        // Tools are in PATH, return empty (will use just tool name)
        return Ok(PathBuf::new());
    }

    Err(BackendError::Llvm(
        "LLVM tools not found. Set LLVM_SYS_180_PREFIX or ensure llvm-link is in PATH".into(),
    ))
}

/// Get path to an LLVM tool
fn get_llvm_tool(tools_dir: &Path, tool_name: &str) -> PathBuf {
    if tools_dir.as_os_str().is_empty() {
        // Tools are in PATH
        PathBuf::from(tool_name)
    } else {
        tools_dir.join(tool_name)
    }
}

/// Run ThinLTO pipeline
///
/// ThinLTO process for LLVM 18:
/// Option 1: Use llvm-lto tool (simplest, if available)
/// Option 2: Manual workflow with module summaries
pub fn run_thinlto(
    bitcode_files: &[PathBuf],
    output_obj: &Path,
    opt_level: OptLevel,
) -> Result<(), BackendError> {
    if bitcode_files.is_empty() {
        return Err(BackendError::Llvm(
            "No bitcode files provided for ThinLTO".into(),
        ));
    }

    let tools_dir = find_llvm_tools()?;

    // Try using llvm-lto first (simpler and more reliable)
    let llvm_lto = get_llvm_tool(&tools_dir, "llvm-lto");
    if llvm_lto.exists() || tools_dir.as_os_str().is_empty() {
        // Try llvm-lto approach
        let mut cmd = Command::new(&llvm_lto);
        cmd.arg("-o").arg(output_obj);

        // Export main symbol to prevent elimination
        cmd.arg("--exported-symbol=main");
        cmd.arg("--exported-symbol=_main");

        // Add optimization level
        match opt_level {
            OptLevel::None => {
                cmd.arg("-O0");
            }
            OptLevel::Speed => {
                cmd.arg("-O2");
            }
            OptLevel::SpeedAndSize => {
                cmd.arg("-O3");
            }
        }

        for bc_file in bitcode_files {
            cmd.arg(bc_file);
        }

        let output = cmd
            .output()
            .map_err(|e| BackendError::Llvm(format!("Failed to run llvm-lto: {}", e)))?;

        if output.status.success() {
            return Ok(());
        }
        // If llvm-lto fails, fall through to manual workflow
    }

    // Fallback: Manual ThinLTO workflow
    let temp_dir = output_obj
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(".lto_temp");

    // Create temp directory
    std::fs::create_dir_all(&temp_dir)
        .map_err(|e| BackendError::Llvm(format!("Failed to create temp directory: {}", e)))?;

    let index_file = temp_dir.join("index.bc");
    let optimized_files: Vec<PathBuf> = (0..bitcode_files.len())
        .map(|i| temp_dir.join(format!("optimized_{}.bc", i)))
        .collect();

    // Step 1: Create ThinLTO index
    create_thinlto_index(bitcode_files, &index_file, &tools_dir)?;

    // Step 2: Optimize each module in parallel (for now, sequential)
    for (i, bc_file) in bitcode_files.iter().enumerate() {
        optimize_with_thinlto(
            bc_file,
            &index_file,
            &optimized_files[i],
            opt_level,
            &tools_dir,
        )?;
    }

    // Step 3: Link optimized modules
    let linked_bc = temp_dir.join("linked.bc");
    link_bitcode_files(&optimized_files, &linked_bc, &tools_dir)?;

    // Step 4: Generate final object file
    generate_object_file(&linked_bc, output_obj, opt_level, &tools_dir)?;

    // Clean up temp directory
    let _ = std::fs::remove_dir_all(&temp_dir);

    Ok(())
}

/// Run Full LTO pipeline
///
/// Full LTO process:
/// 1. Link all bitcode files into one
/// 2. Run whole-program optimization
/// 3. Generate final object file
pub fn run_full_lto(
    bitcode_files: &[PathBuf],
    output_obj: &Path,
    opt_level: OptLevel,
) -> Result<(), BackendError> {
    if bitcode_files.is_empty() {
        return Err(BackendError::Llvm(
            "No bitcode files provided for Full LTO".into(),
        ));
    }

    let tools_dir = find_llvm_tools()?;
    let temp_dir = output_obj
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(".lto_temp");

    // Create temp directory
    std::fs::create_dir_all(&temp_dir)
        .map_err(|e| BackendError::Llvm(format!("Failed to create temp directory: {}", e)))?;

    let linked_bc = temp_dir.join("linked.bc");
    let optimized_bc = temp_dir.join("optimized.bc");

    // Step 1: Link all bitcode files
    link_bitcode_files(bitcode_files, &linked_bc, &tools_dir)?;

    // Step 2: Run whole-program optimization
    optimize_whole_program(&linked_bc, &optimized_bc, opt_level, &tools_dir)?;

    // Step 3: Generate final object file
    generate_object_file(&optimized_bc, output_obj, opt_level, &tools_dir)?;

    // Clean up temp directory
    let _ = std::fs::remove_dir_all(&temp_dir);

    Ok(())
}

/// Create ThinLTO index from bitcode files
///
/// In LLVM 18, the workflow is:
/// 1. Add module summaries to each bitcode file using opt --module-summary
/// 2. Link them together - this automatically creates/merges the summary index
/// 3. The summary index is embedded in the linked bitcode or can be extracted
///
/// For simplicity, we'll link all summarized files and use the resulting
/// bitcode as both the index source and for optimization.
fn create_thinlto_index(
    bitcode_files: &[PathBuf],
    index_file: &Path,
    tools_dir: &Path,
) -> Result<(), BackendError> {
    let opt = get_llvm_tool(tools_dir, "opt");
    let llvm_link = get_llvm_tool(tools_dir, "llvm-link");
    let temp_dir = index_file.parent().unwrap();

    // Step 1: Add module summaries to each bitcode file
    let summarized_files: Vec<PathBuf> = bitcode_files
        .iter()
        .enumerate()
        .map(|(i, bc_file)| {
            let summarized = temp_dir.join(format!("summarized_{}.bc", i));

            let mut cmd = Command::new(&opt);
            cmd.arg("--module-summary")
                .arg("-o")
                .arg(&summarized)
                .arg(bc_file);

            let output = cmd.output().map_err(|e| {
                BackendError::Llvm(format!("Failed to run opt to add module summary: {}", e))
            })?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(BackendError::Llvm(format!(
                    "opt failed to add module summary: {}",
                    stderr
                )));
            }

            Ok(summarized)
        })
        .collect::<Result<Vec<_>, _>>()?;

    // Step 2: Link summarized files - this creates/merges the summary index
    // The summary index information is embedded in the linked bitcode
    let linked_with_index = temp_dir.join("linked_with_index.bc");
    let mut cmd = Command::new(&llvm_link);
    cmd.arg("-o").arg(&linked_with_index);

    for summarized_file in &summarized_files {
        cmd.arg(summarized_file);
    }

    let output = cmd
        .output()
        .map_err(|e| BackendError::Llvm(format!("Failed to run llvm-link: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(BackendError::Llvm(format!(
            "llvm-link failed to create summary index: {}",
            stderr
        )));
    }

    // Step 3: Extract summary index from the linked bitcode
    // In LLVM 18, we can use the linked bitcode itself as the summary source
    // or extract it. For now, we'll use the linked bitcode as the index source.
    // Copy it to the index file location (it contains the summary information)
    std::fs::copy(&linked_with_index, index_file)
        .map_err(|e| BackendError::Llvm(format!("Failed to copy summary index: {}", e)))?;

    Ok(())
}

/// Optimize a single module with ThinLTO
fn optimize_with_thinlto(
    bitcode_file: &Path,
    index_file: &Path,
    output_file: &Path,
    opt_level: OptLevel,
    tools_dir: &Path,
) -> Result<(), BackendError> {
    let opt = get_llvm_tool(tools_dir, "opt");

    let mut cmd = Command::new(&opt);
    cmd.arg("--thinlto-bc")
        .arg("--summary-file")
        .arg(index_file)
        .arg("-o")
        .arg(output_file);

    // Add optimization level
    match opt_level {
        OptLevel::None => {
            cmd.arg("-O0");
        }
        OptLevel::Speed => {
            cmd.arg("-O2");
        }
        OptLevel::SpeedAndSize => {
            cmd.arg("-O3");
        }
    }

    cmd.arg(bitcode_file);

    let output = cmd
        .output()
        .map_err(|e| BackendError::Llvm(format!("Failed to run opt: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(BackendError::Llvm(format!("opt failed: {}", stderr)));
    }

    Ok(())
}

/// Link multiple bitcode files into one
fn link_bitcode_files(
    bitcode_files: &[PathBuf],
    output_file: &Path,
    tools_dir: &Path,
) -> Result<(), BackendError> {
    let llvm_link = get_llvm_tool(tools_dir, "llvm-link");

    let mut cmd = Command::new(&llvm_link);
    cmd.arg("-o").arg(output_file);

    for bc_file in bitcode_files {
        cmd.arg(bc_file);
    }

    let output = cmd
        .output()
        .map_err(|e| BackendError::Llvm(format!("Failed to run llvm-link: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(BackendError::Llvm(format!("llvm-link failed: {}", stderr)));
    }

    Ok(())
}

/// Run whole-program optimization on merged bitcode
fn optimize_whole_program(
    input_file: &Path,
    output_file: &Path,
    opt_level: OptLevel,
    tools_dir: &Path,
) -> Result<(), BackendError> {
    let opt = get_llvm_tool(tools_dir, "opt");

    let mut cmd = Command::new(&opt);
    cmd.arg("-lto").arg("-o").arg(output_file);

    // Add optimization level
    match opt_level {
        OptLevel::None => {
            cmd.arg("-O0");
        }
        OptLevel::Speed => {
            cmd.arg("-O2");
        }
        OptLevel::SpeedAndSize => {
            cmd.arg("-O3");
        }
    }

    cmd.arg(input_file);

    let output = cmd
        .output()
        .map_err(|e| BackendError::Llvm(format!("Failed to run opt: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(BackendError::Llvm(format!("opt failed: {}", stderr)));
    }

    Ok(())
}

/// Generate object file from bitcode
pub fn generate_object_file(
    bitcode_file: &Path,
    output_file: &Path,
    opt_level: OptLevel,
    tools_dir: &Path,
) -> Result<(), BackendError> {
    let llc = get_llvm_tool(tools_dir, "llc");

    let mut cmd = Command::new(&llc);
    cmd.arg("-filetype=obj")
        .arg("--function-sections") // Preserve function sections
        .arg("-o")
        .arg(output_file);

    // Add optimization level
    match opt_level {
        OptLevel::None => {
            cmd.arg("-O0");
        }
        OptLevel::Speed => {
            cmd.arg("-O2");
        }
        OptLevel::SpeedAndSize => {
            cmd.arg("-O3");
        }
    }

    cmd.arg(bitcode_file);

    let output = cmd
        .output()
        .map_err(|e| BackendError::Llvm(format!("Failed to run llc: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(BackendError::Llvm(format!("llc failed: {}", stderr)));
    }

    Ok(())
}

/// Run LTO based on mode
pub fn run_lto(
    bitcode_files: &[PathBuf],
    output_obj: &Path,
    lto_mode: LtoMode,
    opt_level: OptLevel,
) -> Result<(), BackendError> {
    match lto_mode {
        LtoMode::None => Err(BackendError::Llvm(
            "LTO mode is None, cannot run LTO".into(),
        )),
        LtoMode::Thin => run_thinlto(bitcode_files, output_obj, opt_level),
        LtoMode::Full => run_full_lto(bitcode_files, output_obj, opt_level),
    }
}
