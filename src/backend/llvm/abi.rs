//! Runtime ABI integration
//!
//! Declares runtime stubs as external functions for LLVM to call.

use llvm_sys::prelude::*;
use std::collections::HashMap;
use std::ffi::CString;
use std::ptr;

use crate::backend::BackendError;

/// Declare all runtime stubs in the LLVM module
pub unsafe fn declare_runtime_stubs(
    module: LLVMModuleRef,
    context: LLVMContextRef,
    stubs: &mut HashMap<String, LLVMValueRef>,
) -> Result<(), BackendError> {
    let i64_ty = llvm_sys::core::LLVMInt64TypeInContext(context);
    let i8_ty = llvm_sys::core::LLVMInt8TypeInContext(context);
    let i8_ptr_ty = llvm_sys::core::LLVMPointerType(i8_ty, 0);

    // Allocation stubs
    declare_stub(module, context, stubs, "tscl_alloc_object", llvm_sys::core::LLVMFunctionType(i64_ty, ptr::null_mut(), 0, 0))?;
    
    let mut alloc_array_params = vec![i64_ty]; // capacity: usize (i64)
    declare_stub(module, context, stubs, "tscl_alloc_array", llvm_sys::core::LLVMFunctionType(i64_ty, alloc_array_params.as_mut_ptr(), 1, 0))?;
    
    let mut alloc_string_params = vec![i8_ptr_ty, i64_ty]; // data: *const u8, len: usize
    declare_stub(module, context, stubs, "tscl_alloc_string", llvm_sys::core::LLVMFunctionType(i64_ty, alloc_string_params.as_mut_ptr(), 2, 0))?;

    // Property access stubs
    let mut get_prop_params = vec![i64_ty, i8_ptr_ty, i64_ty]; // obj, key, key_len
    declare_stub(module, context, stubs, "tscl_get_prop", llvm_sys::core::LLVMFunctionType(i64_ty, get_prop_params.as_mut_ptr(), 3, 0))?;
    
    let mut set_prop_params = vec![i64_ty, i8_ptr_ty, i64_ty, i64_ty]; // obj, key, key_len, val
    declare_stub(module, context, stubs, "tscl_set_prop", llvm_sys::core::LLVMFunctionType(llvm_sys::core::LLVMVoidTypeInContext(context), set_prop_params.as_mut_ptr(), 4, 0))?;
    
    let mut get_element_params = vec![i64_ty, i64_ty]; // obj, idx
    declare_stub(module, context, stubs, "tscl_get_element", llvm_sys::core::LLVMFunctionType(i64_ty, get_element_params.as_mut_ptr(), 2, 0))?;
    
    let mut set_element_params = vec![i64_ty, i64_ty, i64_ty]; // obj, idx, val
    declare_stub(module, context, stubs, "tscl_set_element", llvm_sys::core::LLVMFunctionType(llvm_sys::core::LLVMVoidTypeInContext(context), set_element_params.as_mut_ptr(), 3, 0))?;

    // Dynamic arithmetic stubs
    let mut binary_params = vec![i64_ty, i64_ty]; // a, b
    declare_stub(module, context, stubs, "tscl_add_any", llvm_sys::core::LLVMFunctionType(i64_ty, binary_params.as_mut_ptr(), 2, 0))?;
    declare_stub(module, context, stubs, "tscl_sub_any", llvm_sys::core::LLVMFunctionType(i64_ty, binary_params.as_mut_ptr(), 2, 0))?;
    declare_stub(module, context, stubs, "tscl_mul_any", llvm_sys::core::LLVMFunctionType(i64_ty, binary_params.as_mut_ptr(), 2, 0))?;
    declare_stub(module, context, stubs, "tscl_div_any", llvm_sys::core::LLVMFunctionType(i64_ty, binary_params.as_mut_ptr(), 2, 0))?;
    declare_stub(module, context, stubs, "tscl_mod_any", llvm_sys::core::LLVMFunctionType(i64_ty, binary_params.as_mut_ptr(), 2, 0))?;

    // Unary operations
    let mut unary_params = vec![i64_ty]; // a
    declare_stub(module, context, stubs, "tscl_neg", llvm_sys::core::LLVMFunctionType(i64_ty, unary_params.as_mut_ptr(), 1, 0))?;
    declare_stub(module, context, stubs, "tscl_not", llvm_sys::core::LLVMFunctionType(i64_ty, unary_params.as_mut_ptr(), 1, 0))?;

    // Comparison stubs
    declare_stub(module, context, stubs, "tscl_eq_strict", llvm_sys::core::LLVMFunctionType(i64_ty, binary_params.as_mut_ptr(), 2, 0))?;
    declare_stub(module, context, stubs, "tscl_lt", llvm_sys::core::LLVMFunctionType(i64_ty, binary_params.as_mut_ptr(), 2, 0))?;
    declare_stub(module, context, stubs, "tscl_gt", llvm_sys::core::LLVMFunctionType(i64_ty, binary_params.as_mut_ptr(), 2, 0))?;

    // Type conversion stubs
    declare_stub(module, context, stubs, "tscl_to_boolean", llvm_sys::core::LLVMFunctionType(i64_ty, unary_params.as_mut_ptr(), 1, 0))?;
    declare_stub(module, context, stubs, "tscl_to_number", llvm_sys::core::LLVMFunctionType(i64_ty, unary_params.as_mut_ptr(), 1, 0))?;

    // Console/IO stubs
    declare_stub(module, context, stubs, "tscl_console_log", llvm_sys::core::LLVMFunctionType(i64_ty, unary_params.as_mut_ptr(), 1, 0))?;
    
    let mut call_params = vec![i64_ty, i64_ty, i8_ptr_ty]; // func, argc, argv
    declare_stub(module, context, stubs, "tscl_call", llvm_sys::core::LLVMFunctionType(i64_ty, call_params.as_mut_ptr(), 3, 0))?;

    // Closure stubs
    let mut make_closure_params = vec![i64_ty, i64_ty]; // func_addr, env
    declare_stub(module, context, stubs, "tscl_make_closure", llvm_sys::core::LLVMFunctionType(i64_ty, make_closure_params.as_mut_ptr(), 2, 0))?;

    Ok(())
}

unsafe fn declare_stub(
    module: LLVMModuleRef,
    _context: LLVMContextRef,
    stubs: &mut HashMap<String, LLVMValueRef>,
    name: &str,
    func_ty: LLVMTypeRef,
) -> Result<(), BackendError> {
    let name_cstr = CString::new(name).unwrap();
    let func_val = llvm_sys::core::LLVMAddFunction(module, name_cstr.as_ptr(), func_ty);
    
    if func_val.is_null() {
        return Err(BackendError::Llvm(format!("Failed to declare stub: {}", name)));
    }

    // Mark as external
    llvm_sys::core::LLVMSetLinkage(func_val, llvm_sys::LLVMLinkage::LLVMExternalLinkage);
    
    stubs.insert(name.to_string(), func_val);
    Ok(())
}
