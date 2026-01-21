//! Ahead-of-time (AOT) compilation for tscl
//!
//! This module provides AOT compilation to standalone executables.
//! Currently a scaffold for future implementation.
//!
//! Future features:
//! - Object file generation
//! - Static linking with runtime
//! - Platform-specific binary output
//! - Link-time optimization (LTO)

use super::{BackendConfig, BackendError, BackendKind, LtoMode};
use crate::ir::IrModule;
use std::path::{Path, PathBuf};

/// AOT compilation target format
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    /// Object file (.o)
    Object,
    /// Static library (.a)
    StaticLib,
    /// Shared library (.so/.dylib/.dll)
    SharedLib,
    /// Executable
    Executable,
}

impl Default for OutputFormat {
    fn default() -> Self {
        OutputFormat::Executable
    }
}

/// AOT compilation options
#[derive(Debug, Clone)]
pub struct AotOptions {
    /// Output format
    pub format: OutputFormat,
    /// Target triple (e.g., "x86_64-apple-darwin")
    pub target: Option<String>,
    /// Link-time optimization mode
    pub lto_mode: LtoMode,
    /// Strip debug symbols
    pub strip: bool,
}

impl Default for AotOptions {
    fn default() -> Self {
        Self {
            format: OutputFormat::Executable,
            target: None,
            lto_mode: LtoMode::None,
            strip: false,
        }
    }
}

/// AOT compiler state
pub struct AotCompiler {
    config: BackendConfig,
    options: AotOptions,
}

impl AotCompiler {
    /// Create a new AOT compiler
    pub fn new(config: &BackendConfig) -> Self {
        Self {
            config: config.clone(),
            options: AotOptions::default(),
        }
    }

    /// Set AOT options
    pub fn with_options(mut self, options: AotOptions) -> Self {
        self.options = options;
        self
    }

    /// Compile multiple IR modules to a file (with LTO support)
    pub fn compile_modules_to_file(
        &mut self,
        modules: &[&IrModule],
        output: &Path,
    ) -> Result<(), BackendError> {
        if modules.is_empty() {
            return Err(BackendError::AotError("No modules to compile".into()));
        }

        // If only one module and no LTO, use single-module path
        if modules.len() == 1 && self.options.lto_mode == super::LtoMode::None {
            return self.compile_to_file(modules[0], output);
        }

        match self.config.kind {
            BackendKind::LlvmAot => self.compile_modules_llvm(modules, output),
            BackendKind::CraneliftAot => Err(BackendError::AotError(
                "Cranelift AOT compilation not yet implemented".into(),
            )),
            _ => Err(BackendError::AotError(
                "AOT compilation requires LlvmAot or CraneliftAot backend".into(),
            )),
        }
    }

    /// Compile an IR module to a file (single module, with optional LTO)
    pub fn compile_to_file(
        &mut self,
        module: &IrModule,
        output: &Path,
    ) -> Result<(), BackendError> {
        // If LTO is enabled, use LTO path even for single module
        if self.options.lto_mode != super::LtoMode::None {
            return self.compile_modules_to_file(&[module], output);
        }

        match self.config.kind {
            BackendKind::LlvmAot => {
                // Use LLVM backend
                let obj_file = output.with_extension("o");
                super::llvm::compile_to_object_file(module, &self.config, &obj_file)?;

                // Link if output format is executable or shared library
                match self.options.format {
                    OutputFormat::Executable | OutputFormat::SharedLib => {
                        // Find runtime library (required)
                        let runtime_lib = find_runtime_library()?;
                        super::llvm::linker::link_object_files_with_lto(
                            &[obj_file.clone()],
                            output,
                            self.options.format,
                            Some(&runtime_lib),
                            self.options.lto_mode,
                        )?;
                    }
                    OutputFormat::Object => {
                        // Just copy object file to output
                        std::fs::copy(&obj_file, output).map_err(|e| {
                            BackendError::AotError(format!("Failed to copy object file: {}", e))
                        })?;
                    }
                    OutputFormat::StaticLib => {
                        super::llvm::linker::create_static_library(&[obj_file], output)?;
                    }
                }

                Ok(())
            }
            BackendKind::CraneliftAot => Err(BackendError::AotError(
                "Cranelift AOT compilation not yet implemented".into(),
            )),
            _ => Err(BackendError::AotError(
                "AOT compilation requires LlvmAot or CraneliftAot backend".into(),
            )),
        }
    }

    /// Compile multiple modules using LLVM (with LTO support)
    fn compile_modules_llvm(
        &mut self,
        modules: &[&IrModule],
        output: &Path,
    ) -> Result<(), BackendError> {
        use super::llvm::{cache, lto};
        use std::path::PathBuf;

        let temp_dir = output
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(".compile_temp");
        std::fs::create_dir_all(&temp_dir).map_err(|e| {
            BackendError::AotError(format!("Failed to create temp directory: {}", e))
        })?;

        let mut bitcode_files = Vec::new();

        // Compile each module to bitcode
        for (i, module) in modules.iter().enumerate() {
            let bc_file = temp_dir.join(format!("module_{}.bc", i));

            // For now, skip cache (would need source paths tracked)
            // Compile to bitcode
            super::llvm::compile_to_bitcode_file(module, &self.config, &bc_file)?;
            bitcode_files.push(bc_file);
        }

        // Run LTO if enabled
        let obj_file = if self.options.lto_mode != super::LtoMode::None {
            let temp_obj = temp_dir.join("lto_output.o");
            lto::run_lto(
                &bitcode_files,
                &temp_obj,
                self.options.lto_mode,
                self.config.opt_level,
            )?;
            temp_obj
        } else {
            // No LTO: compile each bitcode to object, then link
            let mut obj_files = Vec::new();
            let tools_dir = PathBuf::new(); // Will use PATH
            for (i, bc_file) in bitcode_files.iter().enumerate() {
                let obj_file = temp_dir.join(format!("module_{}.o", i));
                // Use llc to generate object from bitcode
                lto::generate_object_file(bc_file, &obj_file, self.config.opt_level, &tools_dir)?;
                obj_files.push(obj_file);
            }

            // Link objects
            let linked_obj = temp_dir.join("linked.o");
            super::llvm::linker::link_object_files(
                &obj_files,
                &linked_obj,
                OutputFormat::Object,
                None,
            )?;
            linked_obj
        };

        // Link if output format is executable or shared library
        match self.options.format {
            OutputFormat::Executable | OutputFormat::SharedLib => {
                // Runtime stubs are now implemented directly in LLVM IR (see abi.rs),
                // so no external runtime library is needed for basic operations.
                let runtime_lib: Option<std::path::PathBuf> = None;
                super::llvm::linker::link_object_files_with_lto(
                    &[obj_file],
                    output,
                    self.options.format,
                    runtime_lib.as_deref(),
                    self.options.lto_mode,
                )?;
            }
            OutputFormat::Object => {
                std::fs::copy(&obj_file, output).map_err(|e| {
                    BackendError::AotError(format!("Failed to copy object file: {}", e))
                })?;
            }
            OutputFormat::StaticLib => {
                super::llvm::linker::create_static_library(&[obj_file], output)?;
            }
        }

        // Clean up temp directory
        let _ = std::fs::remove_dir_all(&temp_dir);

        Ok(())
    }

    /// Compile an IR module to bytes (object file in memory)
    pub fn compile_to_bytes(&mut self, module: &IrModule) -> Result<Vec<u8>, BackendError> {
        match self.config.kind {
            BackendKind::LlvmAot => {
                // Use LLVM backend
                use std::time::{SystemTime, UNIX_EPOCH};
                let timestamp = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos();
                let temp_file = std::env::temp_dir().join(format!("tscl_{}.o", timestamp));
                super::llvm::compile_to_object_file(module, &self.config, &temp_file)?;

                // Read object file bytes
                let bytes = std::fs::read(&temp_file).map_err(|e| {
                    BackendError::AotError(format!("Failed to read object file: {}", e))
                })?;

                // Clean up temp file
                let _ = std::fs::remove_file(&temp_file);

                Ok(bytes)
            }
            _ => Err(BackendError::AotError(
                "AOT compilation to bytes requires LlvmAot backend".into(),
            )),
        }
    }

    /// Compile modules to object file
    pub fn compile_modules_to_object(
        &mut self,
        modules: &[&IrModule],
        output: &Path,
    ) -> Result<(), BackendError> {
        if modules.is_empty() {
            return Err(BackendError::AotError("No modules to compile".into()));
        }

        match self.config.kind {
            BackendKind::LlvmAot => {
                // Compile first module to object file
                super::llvm::compile_to_object_file(modules[0], &self.config, output)?;
                Ok(())
            }
            _ => Err(BackendError::AotError(
                "Object file compilation requires LlvmAot backend".into(),
            )),
        }
    }

    /// Compile modules to LLVM IR text format
    pub fn compile_modules_to_llvm_ir(
        &mut self,
        modules: &[&IrModule],
        output: &Path,
    ) -> Result<(), BackendError> {
        if modules.is_empty() {
            return Err(BackendError::AotError("No modules to compile".into()));
        }

        match self.config.kind {
            BackendKind::LlvmAot => {
                // Compile first module to LLVM IR
                super::llvm::compile_to_llvm_ir_file(modules[0], &self.config, output)?;
                Ok(())
            }
            _ => Err(BackendError::AotError(
                "LLVM IR emission requires LlvmAot backend".into(),
            )),
        }
    }
}

/// Get the default target triple for the current platform
pub fn default_target() -> String {
    target_lexicon::Triple::host().to_string()
}

/// Find or build the runtime library
///
/// NOTE: Runtime stubs are now implemented directly in LLVM IR in abi.rs,
/// so this function is no longer needed for basic programs. It remains
/// for future use when we want to link additional runtime features.
fn find_runtime_library() -> Result<PathBuf, BackendError> {
    // Runtime stubs are now generated in LLVM IR, so no external library needed
    return Err(BackendError::AotError(
        "Runtime library not needed - stubs are generated in LLVM IR".into(),
    ));

    #[allow(unreachable_code)]
    // Get manifest directory (project root)
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            // Fall back to current directory if not in cargo build
            std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
        });

    // Determine profile based on whether we're in release mode
    // Default to release for AOT builds
    let profile = if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    };
    let runtime_lib = manifest_dir
        .join("target")
        .join(profile)
        .join("libruntime.a");

    // If library exists, return it
    if runtime_lib.exists() {
        return Ok(runtime_lib);
    }

    // Library doesn't exist - try to build it on-demand
    // Note: This may fail due to Rust std linking issues, which is okay for simple programs
    println!("Building runtime library...");
    if let Err(e) = build_runtime_library(
        &manifest_dir,
        &runtime_lib.parent().unwrap(),
        profile == "release",
    ) {
        eprintln!("[WARN] Failed to build runtime library: {}", e);
        return Err(BackendError::AotError(format!(
            "Runtime library build failed: {}. Simple programs may work without it.",
            e
        )));
    }

    if runtime_lib.exists() {
        Ok(runtime_lib)
    } else {
        Err(BackendError::AotError(format!(
            "Failed to build runtime library at {}",
            runtime_lib.display()
        )))
    }
}

/// Build the runtime library on-demand
fn build_runtime_library(
    manifest_dir: &PathBuf,
    output_dir: &Path,
    release: bool,
) -> Result<(), BackendError> {
    use std::process::Command;

    // Create output directory if it doesn't exist
    std::fs::create_dir_all(output_dir)
        .map_err(|e| BackendError::AotError(format!("Failed to create output directory: {}", e)))?;

    let cargo_toml = manifest_dir.join("Cargo.toml");
    if !cargo_toml.exists() {
        return Err(BackendError::AotError(format!(
            "Cargo.toml not found at {}",
            cargo_toml.display()
        )));
    }

    // Clean old rlibs to ensure we build fresh without hashbrown/vm_interop
    let profile_dir = if release { "release" } else { "debug" };
    let deps_dir = manifest_dir.join("target").join(profile_dir).join("deps");
    if let Ok(entries) = std::fs::read_dir(&deps_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if name.starts_with("libscript") && name.ends_with(".rlib") {
                    let _ = std::fs::remove_file(&path);
                }
            }
        }
    }

    // Use cargo build to build the library
    // This will handle dependencies correctly
    let mut cmd = Command::new("cargo");
    cmd.arg("build")
        .arg("--lib")
        .arg("--manifest-path")
        .arg(cargo_toml.to_string_lossy().as_ref())
        // Disable default features (vm_interop) for standalone staticlib
        .arg("--no-default-features")
        // Prevent build script from running (avoid recursion)
        .env("TSCL_BUILDING_RUNTIME", "1");

    // Use release profile (LTO is disabled in Cargo.toml)
    if release {
        cmd.arg("--release");
    }

    let output = cmd
        .output()
        .map_err(|e| BackendError::AotError(format!("Failed to execute cargo rustc: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(BackendError::AotError(format!(
            "cargo rustc failed to build runtime library:\n{}",
            stderr
        )));
    }

    // cargo build outputs rlib by default
    // rlibs are actually ar archives and can be used for static linking
    let profile_dir = if release { "release" } else { "debug" };
    let deps_dir = manifest_dir.join("target").join(profile_dir).join("deps");

    // Look for libscript*.rlib (cargo build produces rlib by default)
    let runtime_lib = output_dir.join("libruntime.a");

    // Try to find the rlib file
    let mut found_lib = None;
    if let Ok(entries) = std::fs::read_dir(&deps_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                // Look for libscript*.rlib files
                if name.starts_with("libscript") && name.ends_with(".rlib") {
                    found_lib = Some(path);
                    break;
                }
            }
        }
    }

    if let Some(source_lib) = found_lib {
        // Copy rlib and rename to .a (rlibs are ar archives, compatible with static linking)
        std::fs::copy(&source_lib, &runtime_lib).map_err(|e| {
            BackendError::AotError(format!(
                "Failed to copy {} to {}: {}",
                source_lib.display(),
                runtime_lib.display(),
                e
            ))
        })?;
    } else {
        return Err(BackendError::AotError(format!(
            "Runtime library not found after build in {}. Expected libscript*.rlib",
            deps_dir.display()
        )));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_target() {
        let target = default_target();
        assert!(!target.is_empty());
        // Should contain architecture
        assert!(
            target.contains("x86_64")
                || target.contains("aarch64")
                || target.contains("arm")
                || target.contains("i686")
        );
    }

    #[test]
    fn test_aot_options_default() {
        let opts = AotOptions::default();
        assert_eq!(opts.format, OutputFormat::Executable);
        assert_eq!(opts.lto_mode, LtoMode::None);
        assert!(!opts.strip);
    }
}
