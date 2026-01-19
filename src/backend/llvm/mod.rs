//! LLVM backend module root
//!
//! This module provides AOT compilation using LLVM. It translates tscl SSA IR
//! to LLVM IR and generates optimized native object files.

pub mod types;
pub mod codegen;
pub mod abi;
pub mod optimizer;
pub mod object;
pub mod linker;

pub use codegen::LlvmCodegen;

use std::path::Path;

use crate::backend::{BackendConfig, BackendError};
use crate::ir::IrModule;

/// Compile an IR module and emit an object file
pub fn compile_to_object_file(
    module: &IrModule,
    config: &BackendConfig,
    output_path: &Path,
) -> Result<(), BackendError> {
    // Get target triple
    let target_triple = object::get_default_target_triple()?;
    
    // Create codegen
    let mut codegen = LlvmCodegen::new(target_triple.clone())?;
    
    // Compile module
    codegen.compile_module(module)?;
    
    // Get target machine
    let target_machine = unsafe {
        object::create_target_machine(&target_triple, config.opt_level)?
    };
    
    // Run optimizations
    unsafe {
        optimizer::run_optimizations(codegen.module, config.opt_level)?;
    }
    
    // Emit object file
    unsafe {
        object::emit_object_file(codegen.module, target_machine, output_path)?;
        llvm_sys::target_machine::LLVMDisposeTargetMachine(target_machine);
    }
    
    Ok(())
}
