//! LLVM backend module root
//!
//! This module provides AOT compilation using LLVM. It translates tscl SSA IR
//! to LLVM IR and generates optimized native object files.

// Allow these for LLVM FFI code
#![allow(clippy::manual_c_str_literals)]
#![allow(clippy::missing_safety_doc)]
#![allow(clippy::type_complexity)]
#![allow(clippy::uninit_vec)]

pub mod abi;
pub mod bitcode;
pub mod cache;
pub mod codegen;
pub mod linker;
pub mod lto;
pub mod object;
pub mod optimizer;
pub mod types;

pub use codegen::LlvmCodegen;

use std::ffi::c_char;
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
    let target_machine =
        unsafe { object::create_target_machine(&target_triple, config.opt_level)? };

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

/// Compile an IR module and emit a bitcode file
pub fn compile_to_bitcode_file(
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

    // Get target machine (needed for data layout)
    let target_machine =
        unsafe { object::create_target_machine(&target_triple, config.opt_level)? };

    // Set data layout on module (needed for LTO)
    unsafe {
        let data_layout = llvm_sys::target_machine::LLVMCreateTargetDataLayout(target_machine);
        if !data_layout.is_null() {
            let data_layout_str = llvm_sys::target::LLVMCopyStringRepOfTargetData(data_layout);
            if !data_layout_str.is_null() {
                llvm_sys::core::LLVMSetDataLayout(codegen.module, data_layout_str);
                llvm_sys::core::LLVMDisposeMessage(data_layout_str);
            }
            llvm_sys::target::LLVMDisposeTargetData(data_layout);
        }
        llvm_sys::target_machine::LLVMDisposeTargetMachine(target_machine);
    }

    // Run optimizations (lightweight for bitcode, full optimization happens during LTO)
    unsafe {
        optimizer::run_optimizations(codegen.module, config.opt_level)?;
    }

    // Emit bitcode file
    unsafe {
        bitcode::emit_bitcode_file(codegen.module, output_path)?;
    }

    Ok(())
}

/// Compile an IR module and emit an LLVM IR text file
pub fn compile_to_llvm_ir_file(
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

    // Get target machine (needed for data layout)
    let target_machine =
        unsafe { object::create_target_machine(&target_triple, config.opt_level)? };

    // Set data layout on module
    unsafe {
        let data_layout = llvm_sys::target_machine::LLVMCreateTargetDataLayout(target_machine);
        if !data_layout.is_null() {
            let data_layout_str = llvm_sys::target::LLVMCopyStringRepOfTargetData(data_layout);
            if !data_layout_str.is_null() {
                llvm_sys::core::LLVMSetDataLayout(codegen.module, data_layout_str);
                llvm_sys::core::LLVMDisposeMessage(data_layout_str);
            }
            llvm_sys::target::LLVMDisposeTargetData(data_layout);
        }
        llvm_sys::target_machine::LLVMDisposeTargetMachine(target_machine);
    }

    // Run optimizations
    unsafe {
        optimizer::run_optimizations(codegen.module, config.opt_level)?;
    }

    // Emit LLVM IR text file
    let output_path_c = std::ffi::CString::new(output_path.to_string_lossy().as_ref())
        .map_err(|e| BackendError::AotError(format!("Invalid output path: {}", e)))?;

    let mut error_msg: *mut c_char = std::ptr::null_mut();
    let result = unsafe {
        llvm_sys::core::LLVMPrintModuleToFile(
            codegen.module,
            output_path_c.as_ptr(),
            &mut error_msg,
        )
    };

    if result != 0 {
        // PrintModuleToFile returns non-zero on error
        if !error_msg.is_null() {
            let error_msg_str = unsafe { std::ffi::CStr::from_ptr(error_msg) }
                .to_string_lossy()
                .into_owned();
            unsafe { llvm_sys::core::LLVMDisposeMessage(error_msg) };
            return Err(BackendError::AotError(format!(
                "LLVM IR emission failed: {}",
                error_msg_str
            )));
        }
        return Err(BackendError::AotError(
            "LLVM IR emission failed with unknown error".into(),
        ));
    }

    Ok(())
}
