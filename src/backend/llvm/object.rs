//! Object file generation
//!
//! Handles target machine creation and object file emission.

use llvm_sys::analysis::*;
use llvm_sys::prelude::*;
use llvm_sys::target_machine::*;
use std::ffi::{CStr, CString, c_char};
use std::path::Path;
use std::ptr;

use crate::backend::{BackendError, OptLevel};

/// Get the default target triple for the current platform
pub fn get_default_target_triple() -> Result<String, BackendError> {
    unsafe {
        let triple = llvm_sys::target_machine::LLVMGetDefaultTargetTriple();
        if triple.is_null() {
            return Err(BackendError::Llvm(
                "Failed to get default target triple".into(),
            ));
        }
        let cstr = CStr::from_ptr(triple);
        let result = cstr.to_string_lossy().into_owned();
        llvm_sys::core::LLVMDisposeMessage(triple);
        Ok(result)
    }
}

/// Create a target machine for the given target triple
pub unsafe fn create_target_machine(
    target_triple: &str,
    opt_level: OptLevel,
) -> Result<LLVMTargetMachineRef, BackendError> {
    unsafe {
        // Initialize LLVM targets
        llvm_sys::target::LLVM_InitializeNativeTarget();
        llvm_sys::target::LLVM_InitializeNativeAsmPrinter();
        llvm_sys::target::LLVM_InitializeNativeAsmParser();

        let triple_cstr = CString::new(target_triple).unwrap();

        let mut target: LLVMTargetRef = ptr::null_mut();
        let mut error_msg: *mut c_char = ptr::null_mut();

        if llvm_sys::target_machine::LLVMGetTargetFromTriple(
            triple_cstr.as_ptr(),
            &mut target,
            &mut error_msg,
        ) != 0
        {
            let error = if !error_msg.is_null() {
                let cstr = CStr::from_ptr(error_msg);
                let msg = cstr.to_string_lossy().into_owned();
                llvm_sys::core::LLVMDisposeMessage(error_msg);
                msg
            } else {
                "Unknown error".to_string()
            };
            return Err(BackendError::Llvm(format!(
                "Failed to get target: {}",
                error
            )));
        }

        // Get CPU and features (use defaults for now)
        let cpu_cstr = CString::new("").unwrap();
        let features_cstr = CString::new("").unwrap();

        // Determine optimization level
        let level = match opt_level {
            OptLevel::None => LLVMCodeGenOptLevel::LLVMCodeGenLevelNone,
            OptLevel::Speed => LLVMCodeGenOptLevel::LLVMCodeGenLevelDefault,
            OptLevel::SpeedAndSize => LLVMCodeGenOptLevel::LLVMCodeGenLevelAggressive,
        };

        let target_machine = llvm_sys::target_machine::LLVMCreateTargetMachine(
            target,
            triple_cstr.as_ptr(),
            cpu_cstr.as_ptr(),
            features_cstr.as_ptr(),
            level,
            LLVMRelocMode::LLVMRelocDefault,
            LLVMCodeModel::LLVMCodeModelDefault,
        );

        if target_machine.is_null() {
            return Err(BackendError::Llvm("Failed to create target machine".into()));
        }

        Ok(target_machine)
    }
}

/// Emit an object file from the module
pub unsafe fn emit_object_file(
    module: LLVMModuleRef,
    target_machine: LLVMTargetMachineRef,
    path: &Path,
) -> Result<(), BackendError> {
    unsafe {
        // Set data layout on module
        let data_layout = llvm_sys::target_machine::LLVMCreateTargetDataLayout(target_machine);
        if data_layout.is_null() {
            return Err(BackendError::Llvm(
                "Failed to create target data layout".into(),
            ));
        }

        let data_layout_str = llvm_sys::target::LLVMCopyStringRepOfTargetData(data_layout);
        if data_layout_str.is_null() {
            llvm_sys::target::LLVMDisposeTargetData(data_layout);
            return Err(BackendError::Llvm(
                "Failed to copy target data layout string".into(),
            ));
        }

        llvm_sys::core::LLVMSetDataLayout(module, data_layout_str);
        llvm_sys::core::LLVMDisposeMessage(data_layout_str);
        llvm_sys::target::LLVMDisposeTargetData(data_layout);

        // Verify module is still valid
        if module.is_null() {
            return Err(BackendError::Llvm(
                "Module is null before emitting object file".into(),
            ));
        }

        // Verify module before emitting (optional but helps catch issues)
        let verify_result = LLVMVerifyModule(
            module,
            LLVMVerifierFailureAction::LLVMPrintMessageAction,
            ptr::null_mut(),
        );
        if verify_result != 0 {
            // Module has errors, but continue anyway (might be recoverable)
            eprintln!("Warning: LLVM module verification found issues");
        }

        // Emit object file
        let path_cstr = CString::new(path.to_str().unwrap()).unwrap();
        let mut error_msg: *mut c_char = ptr::null_mut();

        let result = llvm_sys::target_machine::LLVMTargetMachineEmitToFile(
            target_machine,
            module,
            path_cstr.as_ptr() as *mut c_char,
            LLVMCodeGenFileType::LLVMObjectFile,
            &mut error_msg,
        );

        if result != 0 {
            let error = if !error_msg.is_null() {
                let cstr = CStr::from_ptr(error_msg);
                cstr.to_string_lossy().into_owned()
            } else {
                "Unknown LLVM error".to_string()
            };
            llvm_sys::core::LLVMDisposeMessage(error_msg);
            return Err(BackendError::Llvm(format!(
                "Failed to emit object file: {}",
                error
            )));
        }

        Ok(())
    }
}
