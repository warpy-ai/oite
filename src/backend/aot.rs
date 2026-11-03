//! Ahead-of-time (AOT) compilation to standalone executables.
//!
//! Each module is compiled with LLVM (optionally through ThinLTO or full
//! LTO) and linked with the native runtime library embedded in `oitec`.

use super::{BackendConfig, BackendError, BackendKind, LtoMode};
use crate::ir::IrModule;
use std::path::{Path, PathBuf};

/// AOT compilation target format
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OutputFormat {
    /// Object file (.o)
    Object,
    /// Static library (.a)
    StaticLib,
    /// Shared library (.so/.dylib/.dll)
    SharedLib,
    /// Executable
    #[default]
    Executable,
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
        self.compile_modules_to_file(&[module], output)
    }

    /// Compile modules with LLVM and link them with the native runtime.
    fn compile_modules_llvm(
        &mut self,
        modules: &[&IrModule],
        output: &Path,
    ) -> Result<(), BackendError> {
        use super::llvm::lto;

        let temp_dir = output
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(".compile_temp");
        std::fs::create_dir_all(&temp_dir).map_err(|e| {
            BackendError::AotError(format!("Failed to create temp directory: {}", e))
        })?;

        let objects: Vec<PathBuf> = if self.options.lto_mode != super::LtoMode::None {
            let mut bitcode_files = Vec::new();
            for (i, module) in modules.iter().enumerate() {
                let bc_file = temp_dir.join(format!("module_{}.bc", i));
                super::llvm::compile_to_bitcode_file(module, &self.config, &bc_file)?;
                bitcode_files.push(bc_file);
            }
            let temp_obj = temp_dir.join("lto_output.o");
            lto::run_lto(
                &bitcode_files,
                &temp_obj,
                self.options.lto_mode,
                self.config.opt_level,
            )?;
            vec![temp_obj]
        } else {
            let mut objects = Vec::new();
            for (i, module) in modules.iter().enumerate() {
                let obj = temp_dir.join(format!("module_{}.o", i));
                super::llvm::compile_to_object_file(module, &self.config, &obj)?;
                objects.push(obj);
            }
            objects
        };

        let result = match self.options.format {
            OutputFormat::Executable | OutputFormat::SharedLib => {
                let runtime_lib = super::llvm::runtime_lib::write_to(&temp_dir)?;
                super::llvm::linker::link_object_files_with_lto(
                    &objects,
                    output,
                    self.options.format,
                    Some(&runtime_lib),
                    self.options.lto_mode,
                )
            }
            OutputFormat::Object => {
                if objects.len() != 1 {
                    Err(BackendError::AotError(
                        "--format obj needs exactly one module (or LTO)".into(),
                    ))
                } else {
                    std::fs::copy(&objects[0], output).map(|_| ()).map_err(|e| {
                        BackendError::AotError(format!("Failed to copy object file: {}", e))
                    })
                }
            }
            OutputFormat::StaticLib => super::llvm::linker::create_static_library(&objects, output),
        };

        let _ = std::fs::remove_dir_all(&temp_dir);
        result
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
                let temp_file = std::env::temp_dir().join(format!("ot_{}.o", timestamp));
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
