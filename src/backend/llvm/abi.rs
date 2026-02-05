//! Runtime ABI integration
//!
//! Defines runtime stubs as LLVM IR functions that are self-contained
//! and don't require external Rust runtime linking.

// Allow these for LLVM FFI code
#![allow(clippy::manual_c_str_literals)]
#![allow(clippy::missing_safety_doc)]

use llvm_sys::core::*;
use llvm_sys::prelude::*;
use std::collections::BTreeMap;
use std::ffi::{CString, c_char};
use std::ptr;

use crate::backend::BackendError;

/// Declare and define all runtime stubs in the LLVM module
pub unsafe fn declare_runtime_stubs(
    module: LLVMModuleRef,
    context: LLVMContextRef,
    stubs: &mut BTreeMap<String, LLVMValueRef>,
) -> Result<(), BackendError> {
    unsafe {
        // Declare libc functions we'll use
        declare_libc_functions(module, context)?;

        // Define runtime stubs with LLVM IR bodies
        define_ot_call(module, context, stubs)?;
        define_ot_console_log(module, context, stubs)?;

        // Simple stubs that just return undefined or passthrough
        define_simple_stubs(module, context, stubs)?;

        Ok(())
    }
}

/// Declare libc functions (printf, etc.)
unsafe fn declare_libc_functions(
    module: LLVMModuleRef,
    context: LLVMContextRef,
) -> Result<(), BackendError> {
    unsafe {
        let i32_ty = LLVMInt32TypeInContext(context);
        let i8_ptr_ty = LLVMPointerType(LLVMInt8TypeInContext(context), 0);

        // int printf(const char* format, ...)
        let printf_name = CString::new("printf").unwrap();
        if LLVMGetNamedFunction(module, printf_name.as_ptr()).is_null() {
            let mut param_types = vec![i8_ptr_ty];
            let printf_ty = LLVMFunctionType(i32_ty, param_types.as_mut_ptr(), 1, 1); // vararg
            LLVMAddFunction(module, printf_name.as_ptr(), printf_ty);
        }

        Ok(())
    }
}

/// Define ot_call: calls a function pointer with arguments
///
/// In our implementation, func_ptr is actually a function address that we can call directly.
/// For simple cases (like calling known functions), we bitcast and call.
unsafe fn define_ot_call(
    module: LLVMModuleRef,
    context: LLVMContextRef,
    stubs: &mut BTreeMap<String, LLVMValueRef>,
) -> Result<(), BackendError> {
    unsafe {
        let i64_ty = LLVMInt64TypeInContext(context);
        let i8_ptr_ty = LLVMPointerType(LLVMInt8TypeInContext(context), 0);

        // ot_call(func: i64, argc: i64, argv: i8*) -> i64
        let mut param_types = vec![i64_ty, i64_ty, i8_ptr_ty];
        let func_ty = LLVMFunctionType(i64_ty, param_types.as_mut_ptr(), 3, 0);

        let func_name = CString::new("ot_call").unwrap();
        let func = LLVMAddFunction(module, func_name.as_ptr(), func_ty);

        if func.is_null() {
            return Err(BackendError::Llvm("Failed to create ot_call".into()));
        }

        // Create entry basic block
        let entry_name = CString::new("entry").unwrap();
        let entry_bb = LLVMAppendBasicBlockInContext(context, func, entry_name.as_ptr());
        let builder = LLVMCreateBuilderInContext(context);
        LLVMPositionBuilderAtEnd(builder, entry_bb);

        // Get parameters
        let func_ptr_param = LLVMGetParam(func, 0);
        let argc_param = LLVMGetParam(func, 1);
        let _argv_param = LLVMGetParam(func, 2);

        // Check if we have 0 or 1 arg (common cases for fib)
        let zero = LLVMConstInt(i64_ty, 0, 0);
        let one = LLVMConstInt(i64_ty, 1, 0);

        // Create blocks for different arg counts
        let call0_name = CString::new("call0").unwrap();
        let call1_name = CString::new("call1").unwrap();
        let default_name = CString::new("default").unwrap();

        let call0_bb = LLVMAppendBasicBlockInContext(context, func, call0_name.as_ptr());
        let call1_bb = LLVMAppendBasicBlockInContext(context, func, call1_name.as_ptr());
        let default_bb = LLVMAppendBasicBlockInContext(context, func, default_name.as_ptr());

        // Create switch on argc
        let switch = LLVMBuildSwitch(builder, argc_param, default_bb, 2);
        LLVMAddCase(switch, zero, call0_bb);
        LLVMAddCase(switch, one, call1_bb);

        // Block for 0 args: call func()
        LLVMPositionBuilderAtEnd(builder, call0_bb);
        {
            // Cast func_ptr to function type: () -> i64
            let callee_ty = LLVMFunctionType(i64_ty, ptr::null_mut(), 0, 0);
            let callee_ptr_ty = LLVMPointerType(callee_ty, 0);
            let callee = LLVMBuildIntToPtr(
                builder,
                func_ptr_param,
                callee_ptr_ty,
                b"callee\0".as_ptr() as *const c_char,
            );
            let call_name = CString::new("result0").unwrap();
            let result = LLVMBuildCall2(
                builder,
                callee_ty,
                callee,
                ptr::null_mut(),
                0,
                call_name.as_ptr(),
            );
            LLVMBuildRet(builder, result);
        }

        // Block for 1 arg: call func(arg0)
        LLVMPositionBuilderAtEnd(builder, call1_bb);
        {
            // Get arg0 from stack (we pass args on stack via alloca in call sites)
            // For now, we'll get it from the argv pointer
            // Actually, argv is null in our current impl, we need to change call_indirect
            // For simplicity, load arg0 from argv if not null, else return undefined

            // For fibonacci specifically, we need to handle this properly
            // The args are passed in argv as an array of i64
            // But our current impl passes null... we need to fix that

            // For now, return undefined for 1-arg calls through ot_call
            // The real fix is in how we generate Call ops
            let undefined = LLVMConstInt(i64_ty, 0x7FF8000000000001u64, 0); // NaN undefined
            LLVMBuildRet(builder, undefined);
        }

        // Default block: return undefined
        LLVMPositionBuilderAtEnd(builder, default_bb);
        {
            let undefined = LLVMConstInt(i64_ty, 0x7FF8000000000001u64, 0);
            LLVMBuildRet(builder, undefined);
        }

        LLVMDisposeBuilder(builder);
        stubs.insert("ot_call".to_string(), func);
        Ok(())
    }
}

/// Define ot_console_log: prints a value to stdout
unsafe fn define_ot_console_log(
    module: LLVMModuleRef,
    context: LLVMContextRef,
    stubs: &mut BTreeMap<String, LLVMValueRef>,
) -> Result<(), BackendError> {
    unsafe {
        let i64_ty = LLVMInt64TypeInContext(context);
        let i32_ty = LLVMInt32TypeInContext(context);
        let double_ty = LLVMDoubleTypeInContext(context);
        let i8_ty = LLVMInt8TypeInContext(context);
        let i8_ptr_ty = LLVMPointerType(i8_ty, 0);

        // ot_console_log(value: i64) -> i64
        let mut param_types = vec![i64_ty];
        let func_ty = LLVMFunctionType(i64_ty, param_types.as_mut_ptr(), 1, 0);

        let func_name = CString::new("ot_console_log").unwrap();
        let func = LLVMAddFunction(module, func_name.as_ptr(), func_ty);

        if func.is_null() {
            return Err(BackendError::Llvm("Failed to create ot_console_log".into()));
        }

        // Create entry basic block
        let entry_name = CString::new("entry").unwrap();
        let entry_bb = LLVMAppendBasicBlockInContext(context, func, entry_name.as_ptr());
        let builder = LLVMCreateBuilderInContext(context);
        LLVMPositionBuilderAtEnd(builder, entry_bb);

        // Get value parameter
        let value_param = LLVMGetParam(func, 0);

        // NaN-boxing check: if high bits are 0x7FF8 or higher, it's a special value
        // For simplicity, treat all values as numbers (NaN-boxed doubles)

        // Bitcast i64 to double
        let double_val = LLVMBuildBitCast(
            builder,
            value_param,
            double_ty,
            b"dval\0".as_ptr() as *const c_char,
        );

        // Create format string "%g\n" as a global constant
        // Use LLVMBuildGlobalStringPtr for simpler string handling
        let fmt_ptr = LLVMBuildGlobalStringPtr(
            builder,
            b"%g\n\0".as_ptr() as *const c_char,
            b".fmt\0".as_ptr() as *const c_char,
        );

        // Call printf
        let printf_name = CString::new("printf").unwrap();
        let printf_func = LLVMGetNamedFunction(module, printf_name.as_ptr());
        if !printf_func.is_null() {
            let printf_ty = LLVMFunctionType(i32_ty, [i8_ptr_ty].as_mut_ptr(), 1, 1);
            let mut printf_args = vec![fmt_ptr, double_val];
            let _call_result = LLVMBuildCall2(
                builder,
                printf_ty,
                printf_func,
                printf_args.as_mut_ptr(),
                2,
                b"printf_result\0".as_ptr() as *const c_char,
            );
        }

        // Return undefined
        let undefined = LLVMConstInt(i64_ty, 0x7FF8000000000001u64, 0);
        LLVMBuildRet(builder, undefined);

        LLVMDisposeBuilder(builder);
        stubs.insert("ot_console_log".to_string(), func);
        Ok(())
    }
}

/// Create a binary floating-point operation stub
unsafe fn create_binary_fp_op<F>(
    module: LLVMModuleRef,
    context: LLVMContextRef,
    name: &str,
    op: F,
) -> Result<LLVMValueRef, BackendError>
where
    F: FnOnce(LLVMBuilderRef, LLVMValueRef, LLVMValueRef, LLVMContextRef) -> LLVMValueRef,
{
    unsafe {
        let i64_ty = LLVMInt64TypeInContext(context);
        let double_ty = LLVMDoubleTypeInContext(context);

        let mut param_types = vec![i64_ty, i64_ty];
        let func_ty = LLVMFunctionType(i64_ty, param_types.as_mut_ptr(), 2, 0);

        let func_name = CString::new(name).unwrap();
        let func = LLVMAddFunction(module, func_name.as_ptr(), func_ty);

        if func.is_null() {
            return Err(BackendError::Llvm(format!("Failed to create {}", name)));
        }

        let entry_name = CString::new("entry").unwrap();
        let entry_bb = LLVMAppendBasicBlockInContext(context, func, entry_name.as_ptr());
        let builder = LLVMCreateBuilderInContext(context);
        LLVMPositionBuilderAtEnd(builder, entry_bb);

        // Get parameters
        let a_i64 = LLVMGetParam(func, 0);
        let b_i64 = LLVMGetParam(func, 1);

        // Bitcast i64 to double
        let a_fp = LLVMBuildBitCast(
            builder,
            a_i64,
            double_ty,
            b"a_fp\0".as_ptr() as *const c_char,
        );
        let b_fp = LLVMBuildBitCast(
            builder,
            b_i64,
            double_ty,
            b"b_fp\0".as_ptr() as *const c_char,
        );

        // Perform operation
        let result_fp = op(builder, a_fp, b_fp, context);

        // Bitcast result back to i64
        let result_i64 = LLVMBuildBitCast(
            builder,
            result_fp,
            i64_ty,
            b"result\0".as_ptr() as *const c_char,
        );

        LLVMBuildRet(builder, result_i64);
        LLVMDisposeBuilder(builder);

        Ok(func)
    }
}

/// Create a unary floating-point operation stub
unsafe fn create_unary_fp_op<F>(
    module: LLVMModuleRef,
    context: LLVMContextRef,
    name: &str,
    op: F,
) -> Result<LLVMValueRef, BackendError>
where
    F: FnOnce(LLVMBuilderRef, LLVMValueRef, LLVMContextRef) -> LLVMValueRef,
{
    unsafe {
        let i64_ty = LLVMInt64TypeInContext(context);
        let double_ty = LLVMDoubleTypeInContext(context);

        let mut param_types = vec![i64_ty];
        let func_ty = LLVMFunctionType(i64_ty, param_types.as_mut_ptr(), 1, 0);

        let func_name = CString::new(name).unwrap();
        let func = LLVMAddFunction(module, func_name.as_ptr(), func_ty);

        if func.is_null() {
            return Err(BackendError::Llvm(format!("Failed to create {}", name)));
        }

        let entry_name = CString::new("entry").unwrap();
        let entry_bb = LLVMAppendBasicBlockInContext(context, func, entry_name.as_ptr());
        let builder = LLVMCreateBuilderInContext(context);
        LLVMPositionBuilderAtEnd(builder, entry_bb);

        // Get parameter
        let a_i64 = LLVMGetParam(func, 0);

        // Bitcast i64 to double
        let a_fp = LLVMBuildBitCast(
            builder,
            a_i64,
            double_ty,
            b"a_fp\0".as_ptr() as *const c_char,
        );

        // Perform operation
        let result_fp = op(builder, a_fp, context);

        // Bitcast result back to i64
        let result_i64 = LLVMBuildBitCast(
            builder,
            result_fp,
            i64_ty,
            b"result\0".as_ptr() as *const c_char,
        );

        LLVMBuildRet(builder, result_i64);
        LLVMDisposeBuilder(builder);

        Ok(func)
    }
}

/// Define simple stubs that just declare externals or return undefined
unsafe fn define_simple_stubs(
    module: LLVMModuleRef,
    context: LLVMContextRef,
    stubs: &mut BTreeMap<String, LLVMValueRef>,
) -> Result<(), BackendError> {
    unsafe {
        let i64_ty = LLVMInt64TypeInContext(context);
        let i8_ty = LLVMInt8TypeInContext(context);
        let i8_ptr_ty = LLVMPointerType(i8_ty, 0);
        let void_ty = LLVMVoidTypeInContext(context);

        // Helper to create a stub that returns undefined
        let create_returning_undefined =
            |name: &str, param_types: &mut [LLVMTypeRef]| -> Result<LLVMValueRef, BackendError> {
                let func_ty = LLVMFunctionType(
                    i64_ty,
                    param_types.as_mut_ptr(),
                    param_types.len() as u32,
                    0,
                );
                let func_name = CString::new(name).unwrap();
                let func = LLVMAddFunction(module, func_name.as_ptr(), func_ty);

                if func.is_null() {
                    return Err(BackendError::Llvm(format!("Failed to create {}", name)));
                }

                let entry_name = CString::new("entry").unwrap();
                let entry_bb = LLVMAppendBasicBlockInContext(context, func, entry_name.as_ptr());
                let builder = LLVMCreateBuilderInContext(context);
                LLVMPositionBuilderAtEnd(builder, entry_bb);

                let undefined = LLVMConstInt(i64_ty, 0x7FF8000000000001u64, 0);
                LLVMBuildRet(builder, undefined);

                LLVMDisposeBuilder(builder);
                Ok(func)
            };

        // Helper to create a void stub
        let create_void_stub =
            |name: &str, param_types: &mut [LLVMTypeRef]| -> Result<LLVMValueRef, BackendError> {
                let func_ty = LLVMFunctionType(
                    void_ty,
                    param_types.as_mut_ptr(),
                    param_types.len() as u32,
                    0,
                );
                let func_name = CString::new(name).unwrap();
                let func = LLVMAddFunction(module, func_name.as_ptr(), func_ty);

                if func.is_null() {
                    return Err(BackendError::Llvm(format!("Failed to create {}", name)));
                }

                let entry_name = CString::new("entry").unwrap();
                let entry_bb = LLVMAppendBasicBlockInContext(context, func, entry_name.as_ptr());
                let builder = LLVMCreateBuilderInContext(context);
                LLVMPositionBuilderAtEnd(builder, entry_bb);
                LLVMBuildRetVoid(builder);

                LLVMDisposeBuilder(builder);
                Ok(func)
            };

        // Allocation stubs - return undefined for now
        stubs.insert(
            "ot_alloc_object".to_string(),
            create_returning_undefined("ot_alloc_object", &mut [])?,
        );
        stubs.insert(
            "ot_alloc_array".to_string(),
            create_returning_undefined("ot_alloc_array", &mut [i64_ty])?,
        );
        stubs.insert(
            "ot_alloc_string".to_string(),
            create_returning_undefined("ot_alloc_string", &mut [i8_ptr_ty, i64_ty])?,
        );

        // Property access stubs
        stubs.insert(
            "ot_get_prop".to_string(),
            create_returning_undefined("ot_get_prop", &mut [i64_ty, i8_ptr_ty, i64_ty])?,
        );
        stubs.insert(
            "ot_set_prop".to_string(),
            create_void_stub("ot_set_prop", &mut [i64_ty, i8_ptr_ty, i64_ty, i64_ty])?,
        );
        stubs.insert(
            "ot_get_element".to_string(),
            create_returning_undefined("ot_get_element", &mut [i64_ty, i64_ty])?,
        );
        stubs.insert(
            "ot_set_element".to_string(),
            create_void_stub("ot_set_element", &mut [i64_ty, i64_ty, i64_ty])?,
        );

        // Dynamic arithmetic stubs - perform actual operations
        // These treat values as NaN-boxed doubles: bitcast to double, operate, bitcast back

        // Add
        stubs.insert(
            "ot_add_any".to_string(),
            create_binary_fp_op(module, context, "ot_add_any", |builder, a, b, _ctx| {
                LLVMBuildFAdd(builder, a, b, b"add\0".as_ptr() as *const c_char)
            })?,
        );

        // Sub
        stubs.insert(
            "ot_sub_any".to_string(),
            create_binary_fp_op(module, context, "ot_sub_any", |builder, a, b, _ctx| {
                LLVMBuildFSub(builder, a, b, b"sub\0".as_ptr() as *const c_char)
            })?,
        );

        // Mul
        stubs.insert(
            "ot_mul_any".to_string(),
            create_binary_fp_op(module, context, "ot_mul_any", |builder, a, b, _ctx| {
                LLVMBuildFMul(builder, a, b, b"mul\0".as_ptr() as *const c_char)
            })?,
        );

        // Div
        stubs.insert(
            "ot_div_any".to_string(),
            create_binary_fp_op(module, context, "ot_div_any", |builder, a, b, _ctx| {
                LLVMBuildFDiv(builder, a, b, b"div\0".as_ptr() as *const c_char)
            })?,
        );

        // Mod (fmod)
        stubs.insert(
            "ot_mod_any".to_string(),
            create_binary_fp_op(module, context, "ot_mod_any", |builder, a, b, _ctx| {
                LLVMBuildFRem(builder, a, b, b"mod\0".as_ptr() as *const c_char)
            })?,
        );

        // Unary operations
        stubs.insert(
            "ot_neg".to_string(),
            create_unary_fp_op(module, context, "ot_neg", |builder, a, ctx| {
                let double_ty = LLVMDoubleTypeInContext(ctx);
                let zero = LLVMConstReal(double_ty, 0.0);
                LLVMBuildFSub(builder, zero, a, b"neg\0".as_ptr() as *const c_char)
            })?,
        );

        stubs.insert(
            "ot_not".to_string(),
            create_returning_undefined("ot_not", &mut [i64_ty])?,
        );

        // Comparison stubs
        stubs.insert(
            "ot_eq_strict".to_string(),
            create_returning_undefined("ot_eq_strict", &mut [i64_ty, i64_ty])?,
        );
        stubs.insert(
            "ot_lt".to_string(),
            create_returning_undefined("ot_lt", &mut [i64_ty, i64_ty])?,
        );
        stubs.insert(
            "ot_gt".to_string(),
            create_returning_undefined("ot_gt", &mut [i64_ty, i64_ty])?,
        );

        // Type conversion stubs
        stubs.insert(
            "ot_to_boolean".to_string(),
            create_returning_undefined("ot_to_boolean", &mut [i64_ty])?,
        );
        stubs.insert(
            "ot_to_number".to_string(),
            create_returning_undefined("ot_to_number", &mut [i64_ty])?,
        );

        // Closure stubs
        stubs.insert(
            "ot_make_closure".to_string(),
            create_returning_undefined("ot_make_closure", &mut [i64_ty, i64_ty])?,
        );

        Ok(())
    }
}
