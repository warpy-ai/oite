//! Native code backend for tscl
//!
//! This module provides native code generation from SSA IR using Cranelift.
//! It supports both JIT (just-in-time) and AOT (ahead-of-time) compilation.
//!
//! Architecture:
//! - `layout.rs` - Memory layout calculation for structs/arrays
//! - `cranelift.rs` - IR to Cranelift IR translation
//! - `jit.rs` - JIT compilation and execution runtime
//! - `aot.rs` - Ahead-of-time compilation pipeline (future)
//! - `tier.rs` - Tiered compilation manager

pub mod aot;
pub mod cranelift;
pub mod jit;
pub mod layout;
pub mod llvm;
pub mod tier;

use crate::ir::IrModule;

/// Backend compilation target
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BackendKind {
    /// JIT compilation with Cranelift
    #[default]
    CraneliftJit,
    /// AOT compilation with Cranelift (future)
    CraneliftAot,
    /// AOT compilation with LLVM
    LlvmAot,
    /// Fall back to VM interpreter
    Interpreter,
}

/// Optimization level for native compilation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OptLevel {
    /// No optimization (fastest compile)
    None,
    /// Basic optimizations (default for JIT)
    #[default]
    Speed,
    /// Aggressive optimizations (default for AOT)
    SpeedAndSize,
}

/// Link-time optimization mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum LtoMode {
    /// No LTO (dev mode - fastest compilation)
    #[default]
    None,
    /// ThinLTO (release mode - parallel, fast builds, near-full performance)
    Thin,
    /// Full LTO (dist mode - maximum performance, slower builds)
    Full,
}

/// Configuration for the native backend
#[derive(Debug, Clone)]
pub struct BackendConfig {
    /// Which backend to use
    pub kind: BackendKind,
    /// Optimization level
    pub opt_level: OptLevel,
    /// Enable debug info generation
    pub debug_info: bool,
    /// Enable bounds checking (safety vs speed tradeoff)
    pub bounds_check: bool,
    /// Link-time optimization mode
    pub lto_mode: LtoMode,
}

impl Default for BackendConfig {
    fn default() -> Self {
        Self {
            kind: BackendKind::CraneliftJit,
            opt_level: OptLevel::Speed,
            debug_info: false,
            bounds_check: true,
            lto_mode: LtoMode::None,
        }
    }
}

/// Result of backend compilation
pub struct CompiledModule {
    /// Entry point function pointer
    pub main_ptr: Option<*const u8>,
    /// All compiled function pointers by name
    pub functions: std::collections::HashMap<String, *const u8>,
}

/// Errors that can occur during backend compilation
#[derive(Debug)]
pub enum BackendError {
    /// Cranelift compilation error
    Cranelift(String),
    /// LLVM compilation error
    Llvm(String),
    /// Unsupported IR operation
    UnsupportedOp(String),
    /// Memory layout error
    LayoutError(String),
    /// JIT runtime error
    JitError(String),
    /// AOT compilation error
    AotError(String),
}

impl std::fmt::Display for BackendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BackendError::Cranelift(msg) => write!(f, "Cranelift error: {}", msg),
            BackendError::Llvm(msg) => write!(f, "LLVM error: {}", msg),
            BackendError::UnsupportedOp(op) => write!(f, "Unsupported IR operation: {}", op),
            BackendError::LayoutError(msg) => write!(f, "Memory layout error: {}", msg),
            BackendError::JitError(msg) => write!(f, "JIT error: {}", msg),
            BackendError::AotError(msg) => write!(f, "AOT error: {}", msg),
        }
    }
}

impl std::error::Error for BackendError {}

/// Compile an IR module to native code
pub fn compile(module: &IrModule, config: &BackendConfig) -> Result<CompiledModule, BackendError> {
    match config.kind {
        BackendKind::CraneliftJit => {
            let mut runtime = jit::JitRuntime::new(config)?;
            runtime.compile(module)?;
            Ok(CompiledModule {
                main_ptr: runtime.get_func("main"),
                functions: runtime.get_all_funcs(),
            })
        }
        BackendKind::CraneliftAot => Err(BackendError::AotError(
            "AOT compilation not yet implemented".into(),
        )),
        BackendKind::LlvmAot => {
            // For AOT, use AotCompiler
            let mut aot = aot::AotCompiler::new(config);
            aot.compile_to_bytes(module)?;
            // AOT compilation produces object files, not function pointers
            Ok(CompiledModule {
                main_ptr: None,
                functions: std::collections::HashMap::new(),
            })
        }
        BackendKind::Interpreter => {
            // No native compilation - use VM
            Ok(CompiledModule {
                main_ptr: None,
                functions: std::collections::HashMap::new(),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backend_config_default() {
        let config = BackendConfig::default();
        assert_eq!(config.kind, BackendKind::CraneliftJit);
        assert_eq!(config.opt_level, OptLevel::Speed);
    }
}
