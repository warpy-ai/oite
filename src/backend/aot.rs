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

use super::{BackendConfig, BackendError, BackendKind};
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
    /// Enable link-time optimization
    pub lto: bool,
    /// Strip debug symbols
    pub strip: bool,
}

impl Default for AotOptions {
    fn default() -> Self {
        Self {
            format: OutputFormat::Executable,
            target: None,
            lto: false,
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

    /// Compile an IR module to a file
    pub fn compile_to_file(
        &mut self,
        module: &IrModule,
        output: &Path,
    ) -> Result<(), BackendError> {
        match self.config.kind {
            BackendKind::LlvmAot => {
                // Use LLVM backend
                let obj_file = output.with_extension("o");
                super::llvm::compile_to_object_file(module, &self.config, &obj_file)?;

                // Link if output format is executable or shared library
                match self.options.format {
                    OutputFormat::Executable | OutputFormat::SharedLib => {
                        // Try to find runtime library
                        let runtime_lib = find_runtime_library();
                        if runtime_lib.is_none() {
                            eprintln!("Warning: Runtime library not found. Linking may fail.");
                            eprintln!("  Expected locations:");
                            eprintln!("    - target/release/libruntime.a");
                            eprintln!("    - target/debug/libruntime.a");
                            eprintln!("    - libruntime.a");
                            eprintln!("  Build the runtime library first or provide the path.");
                        }
                        super::llvm::linker::link_object_files(
                            &[obj_file.clone()],
                            output,
                            self.options.format,
                            runtime_lib.as_deref(),
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
            BackendKind::CraneliftAot => {
                Err(BackendError::AotError(
                    "Cranelift AOT compilation not yet implemented".into(),
                ))
            }
            _ => {
                Err(BackendError::AotError(
                    "AOT compilation requires LlvmAot or CraneliftAot backend".into(),
                ))
            }
        }
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
            _ => {
                Err(BackendError::AotError(
                    "AOT compilation to bytes requires LlvmAot backend".into(),
                ))
            }
        }
    }
}

/// Get the default target triple for the current platform
pub fn default_target() -> String {
    target_lexicon::Triple::host().to_string()
}

/// Try to find the runtime library
fn find_runtime_library() -> Option<PathBuf> {
    // Try common locations
    let candidates = &[
        PathBuf::from("target/release/libruntime.a"),
        PathBuf::from("target/debug/libruntime.a"),
        PathBuf::from("libruntime.a"),
    ];

    for candidate in candidates {
        if candidate.exists() {
            return Some(candidate.clone());
        }
    }

    None
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
        assert!(!opts.lto);
        assert!(!opts.strip);
    }
}
