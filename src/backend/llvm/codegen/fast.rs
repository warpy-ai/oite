//! A second entry point for functions, with the arguments in registers.
//!
//! Every function has the dynamic entry `i64 (this, argv, argc, env)`,
//! which is what a function object holds. A call that knows which
//! function it calls does not need the argument array: it calls the fast
//! entry `i64 (this, arg0 .. argN, env)`, where `N` is the number of
//! parameters the function declares (a missing argument is passed as
//! `undefined`, an extra one is dropped: the function could not have
//! seen it). The function's code is compiled into the fast entry, and
//! the dynamic entry only unpacks the array and calls it.

use super::*;

/// More parameters than this, and a function keeps only its dynamic
/// entry.
const MAX_REGISTER_PARAMS: usize = 8;

impl LlvmCodegen {
    /// Whether `func` can take its arguments in registers: it never looks
    /// at the argument array itself (a rest parameter, forwarding every
    /// argument to `super`, an async function's frame).
    pub(super) fn takes_registers(func: &IrFunction) -> bool {
        if func.name == "main" || func.is_async || func.params.len() > MAX_REGISTER_PARAMS {
            return false;
        }
        !func.blocks.iter().flat_map(|b| &b.ops).any(|op| {
            matches!(
                op,
                IrOp::RestArgs(_, _) | IrOp::SuperForward(_, _) | IrOp::IsResume(_)
            )
        })
    }

    /// Declare the fast entry of `func`.
    pub(super) unsafe fn declare_fast(&mut self, func: &IrFunction) {
        unsafe {
            let i64_ty = LLVMInt64TypeInContext(self.context);
            let mut params = vec![i64_ty; func.params.len() + 2];
            let ty = LLVMFunctionType(i64_ty, params.as_mut_ptr(), params.len() as u32, 0);
            let name = CString::new(format!("{}.fast", func.name)).unwrap();
            let f = LLVMAddFunction(self.module, name.as_ptr(), ty);
            LLVMSetLinkage(f, LLVMLinkage::LLVMInternalLinkage);
            self.fast.insert(func.name.clone(), (f, ty));
        }
    }

    /// The dynamic entry of a function compiled into its fast entry:
    /// `argv[i]` if `i < argc`, else `undefined`, for each parameter.
    pub(super) unsafe fn build_dynamic_entry(&mut self, func: &IrFunction) {
        unsafe {
            let entry = self.functions[&func.name];
            let (fast, fast_ty) = self.fast[&func.name];
            let ctx = self.context;
            let i64_ty = LLVMInt64TypeInContext(ctx);
            let b = LLVMCreateBuilderInContext(ctx);
            let block = LLVMAppendBasicBlockInContext(ctx, entry, b"unpack\0".as_ptr() as _);
            LLVMPositionBuilderAtEnd(b, block);
            let undefined = LLVMConstInt(i64_ty, UNDEFINED, 0);
            let argv = LLVMGetParam(entry, 1);
            let argc = LLVMGetParam(entry, 2);
            let undef_slot = LLVMBuildAlloca(b, i64_ty, b"undef\0".as_ptr() as _);
            LLVMBuildStore(b, undefined, undef_slot);
            let mut args = vec![LLVMGetParam(entry, 0)];
            for i in 0..func.params.len() {
                let index = LLVMConstInt(i64_ty, i as u64, 0);
                let present = LLVMBuildICmp(b, LLVMIntPredicate::LLVMIntULT, index, argc, NONAME);
                let mut idx = [index];
                let p = LLVMBuildGEP2(b, i64_ty, argv, idx.as_mut_ptr(), 1, NONAME);
                let src = LLVMBuildSelect(b, present, p, undef_slot, NONAME);
                args.push(LLVMBuildLoad2(b, i64_ty, src, NONAME));
            }
            args.push(LLVMGetParam(entry, 3));
            let r = LLVMBuildCall2(
                b,
                fast_ty,
                fast,
                args.as_mut_ptr(),
                args.len() as u32,
                NONAME,
            );
            LLVMSetTailCall(r, 1);
            LLVMBuildRet(b, r);
            LLVMDisposeBuilder(b);
        }
    }
}

impl Fx<'_> {
    /// Call the function `name` through its fast entry, if it has one:
    /// `args` padded with `undefined`, or cut, to the parameters it
    /// declares.
    pub(super) unsafe fn call_fast(
        &mut self,
        name: &str,
        args: &[LLVMValueRef],
        env: LLVMValueRef,
    ) -> Option<LLVMValueRef> {
        unsafe {
            let (fast, fast_ty) = *self.cg.fast.get(name)?;
            let params = LLVMCountParams(fast) as usize - 2;
            let undefined = self.c(UNDEFINED);
            let mut a = vec![undefined];
            a.extend((0..params).map(|i| args.get(i).copied().unwrap_or(undefined)));
            a.push(env);
            let r = LLVMBuildCall2(
                self.builder,
                fast_ty,
                fast,
                a.as_mut_ptr(),
                a.len() as u32,
                NONAME,
            );
            self.last_call = Some(r);
            Some(r)
        }
    }
}
