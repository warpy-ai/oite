//! What a module needs besides its functions: the C `main`, the stack
//! limit, and the check that the generated IR is well formed.

use super::*;

impl LlvmCodegen {
    /// `int main(int, char**)`: create the function objects, run top-level
    /// code, flush output.
    pub(super) unsafe fn build_c_main(&mut self) -> Result<(), BackendError> {
        unsafe {
            let ctx = self.context;
            let i32_ty = LLVMInt32TypeInContext(ctx);
            let i64_ty = LLVMInt64TypeInContext(ctx);
            let ptr_ty = LLVMPointerTypeInContext(ctx, 0);
            let mut params = [i32_ty, ptr_ty];
            let main_ty = LLVMFunctionType(i32_ty, params.as_mut_ptr(), 2, 0);
            let main = LLVMAddFunction(self.module, b"main\0".as_ptr() as _, main_ty);
            let entry = LLVMAppendBasicBlockInContext(ctx, main, b"entry\0".as_ptr() as _);
            let b = LLVMCreateBuilderInContext(ctx);
            LLVMPositionBuilderAtEnd(b, entry);

            for (addr, global) in &self.fn_objects {
                let f = self.functions[&format!("func_{}", addr)];
                let (mk, mk_ty) = self.runtime["ot_make_function"];
                let mut args = [f];
                let obj = LLVMBuildCall2(b, mk_ty, mk, args.as_mut_ptr(), 1, NONAME);
                LLVMBuildStore(b, obj, *global);
            }

            // Tell the runtime where each global lives, so a lookup by name
            // sees what compiled code stored.
            let (register, register_ty) = self.runtime["ot_register_global"];
            for (name, slot) in &self.global_slots {
                let init = LLVMConstStringInContext(
                    ctx,
                    name.as_ptr() as *const c_char,
                    name.len() as u32,
                    1,
                );
                let text = LLVMAddGlobal(
                    self.module,
                    LLVMTypeOf(init),
                    b"global_name\0".as_ptr() as _,
                );
                LLVMSetInitializer(text, init);
                LLVMSetGlobalConstant(text, 1);
                LLVMSetLinkage(text, LLVMLinkage::LLVMPrivateLinkage);
                let mut args = [text, LLVMConstInt(i64_ty, name.len() as u64, 0), *slot];
                LLVMBuildCall2(b, register_ty, register, args.as_mut_ptr(), 3, NONAME);
            }

            let (init, init_ty) = self.runtime["ot_stack_init"];
            LLVMBuildCall2(b, init_ty, init, std::ptr::null_mut(), 0, NONAME);
            let ot_main = self.functions["main"];
            let mut args = [
                LLVMConstInt(i64_ty, UNDEFINED, 0),
                LLVMConstNull(ptr_ty),
                LLVMConstInt(i64_ty, 0, 0),
                LLVMConstInt(i64_ty, UNDEFINED, 0),
            ];
            let result = LLVMBuildCall2(b, self.fn_ty, ot_main, args.as_mut_ptr(), 4, NONAME);
            // An exception no `catch` handled: report it and exit 1.
            let thrown = LLVMBuildICmp(
                b,
                LLVMIntPredicate::LLVMIntEQ,
                result,
                LLVMConstInt(i64_ty, EXCEPTION_BITS, 0),
                NONAME,
            );
            let uncaught = LLVMAppendBasicBlockInContext(ctx, main, b"uncaught\0".as_ptr() as _);
            let done = LLVMAppendBasicBlockInContext(ctx, main, b"done\0".as_ptr() as _);
            LLVMBuildCondBr(b, thrown, uncaught, done);
            LLVMPositionBuilderAtEnd(b, uncaught);
            let (report, report_ty) = self.runtime["ot_report_uncaught"];
            LLVMBuildCall2(b, report_ty, report, std::ptr::null_mut(), 0, NONAME);
            LLVMBuildUnreachable(b);
            LLVMPositionBuilderAtEnd(b, done);
            // Promise reactions and timers queued by `main`.
            let (event_loop, event_loop_ty) = self.runtime["ot_run_event_loop"];
            LLVMBuildCall2(
                b,
                event_loop_ty,
                event_loop,
                std::ptr::null_mut(),
                0,
                NONAME,
            );
            let (exit, exit_ty) = self.runtime["ot_runtime_exit"];
            LLVMBuildCall2(b, exit_ty, exit, std::ptr::null_mut(), 0, NONAME);
            LLVMBuildRet(b, LLVMConstInt(i32_ty, 0, 0));
            LLVMDisposeBuilder(b);
            Ok(())
        }
    }

    /// The runtime's `OT_STACK_LIMIT`, declared once in the module.
    pub(super) unsafe fn stack_limit_global(&mut self) -> LLVMValueRef {
        unsafe {
            let name = b"OT_STACK_LIMIT\0";
            let existing = LLVMGetNamedGlobal(self.module, name.as_ptr() as _);
            if !existing.is_null() {
                return existing;
            }
            let i64_ty = LLVMInt64TypeInContext(self.context);
            LLVMAddGlobal(self.module, i64_ty, name.as_ptr() as _)
        }
    }

    /// Fail loudly on malformed IR instead of letting LLVM crash later.
    pub(super) unsafe fn verify(&self) -> Result<(), BackendError> {
        unsafe {
            let mut msg: *mut c_char = std::ptr::null_mut();
            let failed = llvm_sys::analysis::LLVMVerifyModule(
                self.module,
                llvm_sys::analysis::LLVMVerifierFailureAction::LLVMReturnStatusAction,
                &mut msg,
            );
            let text = if msg.is_null() {
                String::new()
            } else {
                let t = CStr::from_ptr(msg).to_string_lossy().into_owned();
                LLVMDisposeMessage(msg);
                t
            };
            if failed != 0 {
                return Err(BackendError::Llvm(format!(
                    "internal error: generated invalid LLVM IR:\n{}",
                    text
                )));
            }
            Ok(())
        }
    }
}
