//! Type lowering: tscl IR types → LLVM types

use llvm_sys::prelude::*;
use std::collections::HashMap;

use crate::ir::{IrStructDef, IrType};
use crate::backend::BackendError;

/// Convert an IR type to an LLVM type
pub fn ir_type_to_llvm_type(
    ctx: LLVMContextRef,
    ty: &IrType,
    struct_types: &HashMap<u32, LLVMTypeRef>,
) -> Result<LLVMTypeRef, BackendError> {
    unsafe {
        match ty {
            IrType::Number => Ok(llvm_sys::core::LLVMDoubleTypeInContext(ctx)),
            IrType::Boolean => Ok(llvm_sys::core::LLVMInt1TypeInContext(ctx)),
            IrType::String => {
                // String is a pointer to heap-allocated string
                Ok(llvm_sys::core::LLVMPointerType(llvm_sys::core::LLVMInt8TypeInContext(ctx), 0))
            }
            IrType::Object | IrType::Array => {
                // Objects and arrays are pointers (NaN-boxed in runtime)
                Ok(llvm_sys::core::LLVMInt64TypeInContext(ctx))
            }
            IrType::TypedArray(_) => {
                // Typed arrays are pointers
                Ok(llvm_sys::core::LLVMInt64TypeInContext(ctx))
            }
            IrType::Function => {
                // Functions are pointers
                Ok(llvm_sys::core::LLVMInt64TypeInContext(ctx))
            }
            IrType::Struct(id) => {
                // Look up struct type
                if let Some(&struct_ty) = struct_types.get(&id.0) {
                    Ok(struct_ty)
                } else {
                    Err(BackendError::Llvm(format!("Unknown struct ID: {}", id.0)))
                }
            }
            IrType::Ref(inner) | IrType::MutRef(inner) => {
                let inner_ty = ir_type_to_llvm_type(ctx, inner, struct_types)?;
                Ok(llvm_sys::core::LLVMPointerType(inner_ty, 0))
            }
            IrType::Any => {
                // Any type uses NaN-boxed i64
                Ok(llvm_sys::core::LLVMInt64TypeInContext(ctx))
            }
            IrType::Never => {
                // Never type is void
                Ok(llvm_sys::core::LLVMVoidTypeInContext(ctx))
            }
            IrType::Void => Ok(llvm_sys::core::LLVMVoidTypeInContext(ctx)),
        }
    }
}

/// Create an LLVM struct type from an IR struct definition
pub fn create_struct_type(
    ctx: LLVMContextRef,
    struct_def: &IrStructDef,
    struct_types: &HashMap<u32, LLVMTypeRef>,
) -> Result<LLVMTypeRef, BackendError> {
    unsafe {
        let mut field_types = Vec::new();
        for (_name, field_ty, _offset) in &struct_def.fields {
            let llvm_ty = ir_type_to_llvm_type(ctx, field_ty, struct_types)?;
            field_types.push(llvm_ty);
        }

        let struct_ty = llvm_sys::core::LLVMStructTypeInContext(
            ctx,
            field_types.as_mut_ptr(),
            field_types.len() as u32,
            0, // not packed
        );

        Ok(struct_ty)
    }
}
