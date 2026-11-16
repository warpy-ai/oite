//! Runtime ABI: declarations of the native runtime's functions.
//!
//! The runtime itself is Rust (`src/runtime/stubs/`), compiled by `build.rs`
//! into a static library that every native executable links. This module only
//! declares those functions in the LLVM module, with signatures that must
//! match the Rust definitions exactly.

#![allow(clippy::missing_safety_doc)]

use llvm_sys::core::*;
use llvm_sys::prelude::*;
use std::collections::BTreeMap;
use std::ffi::CString;

use crate::backend::BackendError;
use crate::backend::runtime_abi::{RUNTIME, Ty};

/// Declare every runtime function in `module`. Returns name -> (function, type).
pub unsafe fn declare_runtime(
    module: LLVMModuleRef,
    context: LLVMContextRef,
) -> Result<BTreeMap<String, (LLVMValueRef, LLVMTypeRef)>, BackendError> {
    let mut out = BTreeMap::new();
    unsafe {
        let i64_ty = LLVMInt64TypeInContext(context);
        let ptr_ty = LLVMPointerTypeInContext(context, 0);
        let void_ty = LLVMVoidTypeInContext(context);
        for (name, returns, params) in RUNTIME {
            let mut param_tys: Vec<LLVMTypeRef> = params
                .iter()
                .map(|p| match p {
                    Ty::Val | Ty::Len => i64_ty,
                    Ty::Ptr => ptr_ty,
                })
                .collect();
            let ret = if *returns { i64_ty } else { void_ty };
            let fn_ty = LLVMFunctionType(ret, param_tys.as_mut_ptr(), param_tys.len() as u32, 0);
            let cname = CString::new(*name).unwrap();
            let f = LLVMAddFunction(module, cname.as_ptr(), fn_ty);
            if f.is_null() {
                return Err(BackendError::Llvm(format!("failed to declare {}", name)));
            }
            out.insert(name.to_string(), (f, fn_ty));
        }
    }
    Ok(out)
}
