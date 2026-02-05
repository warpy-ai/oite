//! LLVM bitcode I/O operations
//!
//! This module provides functions for reading and writing LLVM bitcode files,
//! which are used for link-time optimization (LTO).

use llvm_sys::prelude::*;
use std::ffi::{CStr, CString, c_char};
use std::path::Path;

use crate::backend::BackendError;

/// Write an LLVM module to a bitcode file
pub unsafe fn emit_bitcode_file(module: LLVMModuleRef, path: &Path) -> Result<(), BackendError> {
    unsafe {
        if module.is_null() {
            return Err(BackendError::Llvm(
                "Module is null before emitting bitcode".into(),
            ));
        }

        let path_cstr = CString::new(path.to_str().unwrap()).unwrap();
        let error_msg: *mut c_char = std::ptr::null_mut();

        let result = llvm_sys::bit_writer::LLVMWriteBitcodeToFile(module, path_cstr.as_ptr());

        if result != 0 {
            let error = if !error_msg.is_null() {
                let cstr = CStr::from_ptr(error_msg);
                cstr.to_string_lossy().into_owned()
            } else {
                format!("Failed to write bitcode file (error code: {})", result)
            };
            if !error_msg.is_null() {
                llvm_sys::core::LLVMDisposeMessage(error_msg);
            }
            return Err(BackendError::Llvm(format!(
                "Failed to emit bitcode file: {}",
                error
            )));
        }

        Ok(())
    }
}

/// Read an LLVM bitcode file into a module
pub unsafe fn read_bitcode_file(
    context: LLVMContextRef,
    path: &Path,
) -> Result<LLVMModuleRef, BackendError> {
    unsafe {
        if context.is_null() {
            return Err(BackendError::Llvm("Context is null".into()));
        }

        let path_cstr = CString::new(path.to_str().unwrap()).unwrap();
        let mut error_msg: *mut c_char = std::ptr::null_mut();
        let mut mem_buf: LLVMMemoryBufferRef = std::ptr::null_mut();

        // Create a memory buffer from the bitcode file
        let result = llvm_sys::core::LLVMCreateMemoryBufferWithContentsOfFile(
            path_cstr.as_ptr(),
            &mut mem_buf,
            &mut error_msg,
        );

        if result != 0 || mem_buf.is_null() {
            let error = if !error_msg.is_null() {
                let cstr = CStr::from_ptr(error_msg);
                cstr.to_string_lossy().into_owned()
            } else {
                format!("Failed to read bitcode file (error code: {})", result)
            };
            if !error_msg.is_null() {
                llvm_sys::core::LLVMDisposeMessage(error_msg);
            }
            return Err(BackendError::Llvm(format!(
                "Failed to create memory buffer for bitcode file: {}",
                error
            )));
        }

        // Parse the bitcode from the memory buffer into a module
        let mut out_module: LLVMModuleRef = std::ptr::null_mut();
        let parse_result = llvm_sys::bit_reader::LLVMParseBitcode2(mem_buf, &mut out_module);

        // We can dispose the memory buffer after parsing
        llvm_sys::core::LLVMDisposeMemoryBuffer(mem_buf);

        if parse_result != 0 || out_module.is_null() {
            return Err(BackendError::Llvm(
                "Failed to parse bitcode buffer into module".into(),
            ));
        }

        Ok(out_module)
    }
}

/// Write an LLVM module to memory as bitcode
pub unsafe fn emit_bitcode_to_memory(module: LLVMModuleRef) -> Result<Vec<u8>, BackendError> {
    unsafe {
        if module.is_null() {
            return Err(BackendError::Llvm(
                "Module is null before emitting bitcode".into(),
            ));
        }

        // LLVMWriteBitcodeToMemoryBuffer returns a memory buffer directly
        let buffer: LLVMMemoryBufferRef =
            llvm_sys::bit_writer::LLVMWriteBitcodeToMemoryBuffer(module);

        if buffer.is_null() {
            return Err(BackendError::Llvm(
                "Failed to write bitcode to memory buffer".into(),
            ));
        }

        // Extract data from memory buffer
        let data_ptr = llvm_sys::core::LLVMGetBufferStart(buffer);
        let data_size = llvm_sys::core::LLVMGetBufferSize(buffer) as usize;

        // Cast from *const c_char (i8) to *const u8
        let data_ptr_u8 = data_ptr as *const u8;

        let mut result_vec = Vec::with_capacity(data_size);
        result_vec.set_len(data_size);
        std::ptr::copy_nonoverlapping(data_ptr_u8, result_vec.as_mut_ptr(), data_size);

        // Dispose of the memory buffer
        llvm_sys::core::LLVMDisposeMemoryBuffer(buffer);

        Ok(result_vec)
    }
}
