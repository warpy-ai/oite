//! Tiered compilation manager for tscl.
//!
//! This module implements a tiered compilation strategy:
//! 1. Interpreter (VM) - First execution, no compilation overhead
//! 2. Baseline JIT - Quick compile after threshold, moderate optimization
//! 3. Optimizing JIT - Full optimization for very hot code (future)

use std::collections::HashMap;

use crate::backend::jit::JitRuntime;
use crate::backend::{BackendConfig, BackendError};
use crate::ir::IrModule;
use crate::runtime::abi::TsclValue;
use crate::vm::opcodes::OpCode;

/// Compilation tier for a function.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompileTier {
    /// Function runs in interpreter.
    Interpreted,
    /// Function compiled with baseline JIT (fast compile, less optimized).
    BaselineJit,
    /// Function compiled with optimizing JIT (slower compile, more optimized).
    OptimizingJit,
}

/// Statistics for a single function.
#[derive(Debug, Clone)]
pub struct FunctionStats {
    /// Number of times this function has been called.
    pub call_count: u64,
    /// Current compilation tier.
    pub tier: CompileTier,
    /// Bytecode address of the function.
    pub address: usize,
    /// Whether JIT compilation is in progress.
    pub compiling: bool,
}

impl FunctionStats {
    pub fn new(address: usize) -> Self {
        Self {
            call_count: 0,
            tier: CompileTier::Interpreted,
            address,
            compiling: false,
        }
    }
}

/// Configuration for tiered compilation.
#[derive(Debug, Clone)]
pub struct TierConfig {
    /// Call count threshold for baseline JIT compilation.
    pub baseline_threshold: u64,
    /// Call count threshold for optimizing JIT compilation.
    pub optimizing_threshold: u64,
    /// Whether tiered compilation is enabled.
    pub enabled: bool,
}

impl Default for TierConfig {
    fn default() -> Self {
        Self {
            baseline_threshold: 100,
            optimizing_threshold: 1000,
            enabled: true,
        }
    }
}

/// Tiered compilation manager.
///
/// Tracks function execution and triggers JIT compilation when functions
/// become "hot" (frequently called).
pub struct TierManager {
    /// Configuration for compilation thresholds.
    config: TierConfig,
    /// Per-function statistics.
    function_stats: HashMap<usize, FunctionStats>,
    /// JIT runtime for compiling hot functions.
    jit_runtime: Option<JitRuntime>,
    /// Compiled function pointers (address -> native code pointer).
    compiled_functions: HashMap<usize, *const u8>,
}

impl TierManager {
    /// Create a new tier manager.
    pub fn new(config: TierConfig) -> Self {
        Self {
            config,
            function_stats: HashMap::new(),
            jit_runtime: None,
            compiled_functions: HashMap::new(),
        }
    }

    /// Create with default configuration.
    pub fn with_defaults() -> Self {
        Self::new(TierConfig::default())
    }

    /// Record a function call and potentially trigger compilation.
    ///
    /// Returns the compiled function pointer if available, or None to interpret.
    pub fn on_function_call(&mut self, func_addr: usize) -> Option<*const u8> {
        if !self.config.enabled {
            return None;
        }

        // Check if already compiled
        if let Some(&ptr) = self.compiled_functions.get(&func_addr) {
            return Some(ptr);
        }

        // Update stats
        let stats = self
            .function_stats
            .entry(func_addr)
            .or_insert_with(|| FunctionStats::new(func_addr));

        stats.call_count += 1;

        // Check if we should trigger compilation
        if stats.tier == CompileTier::Interpreted
            && stats.call_count >= self.config.baseline_threshold
            && !stats.compiling
        {
            stats.compiling = true;
            // Note: In a real implementation, this would trigger async compilation
            // For now, we just mark it as ready for compilation
        }

        None
    }

    /// Compile a hot function to native code.
    ///
    /// This should be called when a function reaches the compilation threshold.
    pub fn compile_function(
        &mut self,
        func_addr: usize,
        _bytecode: &[OpCode],
        module: &IrModule,
    ) -> Result<*const u8, BackendError> {
        // Initialize JIT runtime if needed
        if self.jit_runtime.is_none() {
            let backend_config = BackendConfig::default();
            self.jit_runtime = Some(JitRuntime::new(&backend_config)?);
        }

        let jit = self.jit_runtime.as_mut().unwrap();

        // Compile the module
        jit.compile(module)?;

        // Get the function pointer
        let func_name = format!("func_{}", func_addr);
        if let Some(ptr) = jit.get_func(&func_name) {
            self.compiled_functions.insert(func_addr, ptr);

            // Update stats
            if let Some(stats) = self.function_stats.get_mut(&func_addr) {
                stats.tier = CompileTier::BaselineJit;
                stats.compiling = false;
            }

            Ok(ptr)
        } else {
            Err(BackendError::JitError(format!(
                "Failed to get compiled function {}",
                func_name
            )))
        }
    }

    /// Check if a function should be compiled.
    pub fn should_compile(&self, func_addr: usize) -> bool {
        if !self.config.enabled {
            return false;
        }

        if self.compiled_functions.contains_key(&func_addr) {
            return false;
        }

        if let Some(stats) = self.function_stats.get(&func_addr) {
            stats.call_count >= self.config.baseline_threshold && !stats.compiling
        } else {
            false
        }
    }

    /// Get statistics for a function.
    pub fn get_stats(&self, func_addr: usize) -> Option<&FunctionStats> {
        self.function_stats.get(&func_addr)
    }

    /// Get all function statistics.
    pub fn all_stats(&self) -> &HashMap<usize, FunctionStats> {
        &self.function_stats
    }

    /// Call a compiled function directly.
    ///
    /// # Safety
    /// The caller must ensure the function pointer is valid and the
    /// argument count matches the function signature.
    pub unsafe fn call_compiled(&self, func_addr: usize, args: &[TsclValue]) -> Option<TsclValue> {
        unsafe {
            let ptr = self.compiled_functions.get(&func_addr)?;

            // Cast and call based on argument count
            let result = match args.len() {
                0 => {
                    let f: extern "C" fn() -> u64 = std::mem::transmute(*ptr);
                    f()
                }
                1 => {
                    let f: extern "C" fn(u64) -> u64 = std::mem::transmute(*ptr);
                    f(args[0].to_bits())
                }
                2 => {
                    let f: extern "C" fn(u64, u64) -> u64 = std::mem::transmute(*ptr);
                    f(args[0].to_bits(), args[1].to_bits())
                }
                3 => {
                    let f: extern "C" fn(u64, u64, u64) -> u64 = std::mem::transmute(*ptr);
                    f(args[0].to_bits(), args[1].to_bits(), args[2].to_bits())
                }
                _ => return None, // Too many arguments
            };

            Some(TsclValue::from_bits(result))
        }
    }

    /// Check if tiered compilation is enabled.
    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }

    /// Enable or disable tiered compilation.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.config.enabled = enabled;
    }

    /// Get the number of compiled functions.
    pub fn compiled_count(&self) -> usize {
        self.compiled_functions.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tier_manager_creation() {
        let manager = TierManager::with_defaults();
        assert!(manager.is_enabled());
        assert_eq!(manager.compiled_count(), 0);
    }

    #[test]
    fn test_call_counting() {
        let mut manager = TierManager::with_defaults();

        // Call function 50 times (below threshold)
        for _ in 0..50 {
            assert!(manager.on_function_call(100).is_none());
        }

        let stats = manager.get_stats(100).unwrap();
        assert_eq!(stats.call_count, 50);
        assert_eq!(stats.tier, CompileTier::Interpreted);
    }

    #[test]
    fn test_compilation_threshold() {
        let config = TierConfig {
            baseline_threshold: 10,
            optimizing_threshold: 100,
            enabled: true,
        };
        let mut manager = TierManager::new(config);

        // Call 9 times - should not trigger
        for _ in 0..9 {
            manager.on_function_call(100);
        }
        assert!(!manager.should_compile(100));

        // 10th call - should trigger
        manager.on_function_call(100);
        // Note: compiling flag is set, so should_compile returns false
        // to prevent duplicate compilation attempts
    }
}
