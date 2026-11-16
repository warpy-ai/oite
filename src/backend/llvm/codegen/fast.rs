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

/// A function's typed entry: its fast entry without the checks of its
/// parameters, for a caller that has made them.
pub(super) struct TypedEntry {
    pub function: LLVMValueRef,
    /// The type each parameter is checked against on entry, where it is
    /// one the IR models.
    pub params: Vec<Option<IrType>>,
}

/// The checks `func` makes of its parameters before it does anything
/// else: the type each parameter must have, and the results of those
/// `IrOp::CheckType`. A check against a type the IR does not model (a
/// union, an object) is not among them: no caller can prove it.
pub(super) fn param_checks(func: &IrFunction) -> (Vec<Option<IrType>>, HashSet<ValueId>) {
    let mut params = vec![None; func.params.len()];
    let mut results = HashSet::new();
    // Which parameter a local slot, or a value, holds.
    let mut slots: HashMap<u32, usize> = HashMap::new();
    let mut values: HashMap<ValueId, usize> = (0..func.params.len())
        .map(|i| (ValueId(i as u32), i))
        .collect();
    let Some(entry) = func.blocks.first() else {
        return (params, results);
    };
    for op in &entry.ops {
        match op {
            IrOp::Const(_, _) | IrOp::Loc(_) => {}
            IrOp::StoreLocal(slot, v) => match values.get(v) {
                Some(&i) => {
                    slots.insert(*slot, i);
                }
                None => {
                    slots.remove(slot);
                }
            },
            IrOp::LoadLocal(d, slot) => {
                if let Some(&i) = slots.get(slot) {
                    values.insert(*d, i);
                }
            }
            IrOp::Copy(d, s) => {
                if let Some(&i) = values.get(s) {
                    values.insert(*d, i);
                }
            }
            IrOp::CheckType(d, src, kinds) => {
                let ty = IrType::of_kinds(*kinds);
                match values.get(src) {
                    Some(&i) if ty != IrType::Any => {
                        params[i] = Some(ty);
                        results.insert(*d);
                        values.insert(*d, i);
                    }
                    // A check no caller can make for the function: what
                    // follows it must not run before it.
                    _ => break,
                }
            }
            _ => break,
        }
    }
    (params, results)
}

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

            // A typed entry, if the function checks a parameter against
            // a type the IR models. (Not with debug info: one function
            // would be described twice.)
            let (params, _) = param_checks(func);
            if self.debug_info || params.iter().all(Option::is_none) {
                return;
            }
            let name = CString::new(format!("{}.typed", func.name)).unwrap();
            let function = LLVMAddFunction(self.module, name.as_ptr(), ty);
            LLVMSetLinkage(function, LLVMLinkage::LLVMInternalLinkage);
            self.typed
                .insert(func.name.clone(), TypedEntry { function, params });
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
    /// declares. `proved` is what is known about each argument.
    pub(super) unsafe fn call_fast(
        &mut self,
        name: &str,
        args: &[LLVMValueRef],
        proved: &[IrType],
        env: LLVMValueRef,
    ) -> Option<LLVMValueRef> {
        unsafe {
            let (mut fast, fast_ty) = *self.cg.fast.get(name)?;
            // The typed entry, when every parameter the function checks
            // is passed a value already known to have the type.
            if let Some(typed) = self.cg.typed.get(name) {
                let all = typed
                    .params
                    .iter()
                    .enumerate()
                    .all(|(i, wanted)| match wanted {
                        Some(ty) => proved.get(i) == Some(ty),
                        None => true,
                    });
                if all {
                    fast = typed.function;
                }
            }
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
