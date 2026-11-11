//! One IR instruction, or one block terminator, to LLVM IR.

use super::*;

impl Fx<'_> {
    pub(super) unsafe fn op(&mut self, op: &IrOp) -> Result<(), BackendError> {
        unsafe {
            match op {
                IrOp::Const(dst, lit) => {
                    let v = self.literal(lit)?;
                    self.set(*dst, v);
                }

                // Numbers the type checker proved: inline floating point.
                IrOp::AddNum(d, a, b) => self.num_binop(*d, *a, *b, LLVMBuildFAdd)?,
                IrOp::SubNum(d, a, b) => self.num_binop(*d, *a, *b, LLVMBuildFSub)?,
                IrOp::MulNum(d, a, b) => self.num_binop(*d, *a, *b, LLVMBuildFMul)?,
                IrOp::DivNum(d, a, b) => self.num_binop(*d, *a, *b, LLVMBuildFDiv)?,
                IrOp::ModNum(d, a, b) => {
                    let x = self.to_f64(self.get(*a)?);
                    let y = self.to_f64(self.get(*b)?);
                    let r = self.js_mod(x, y);
                    let r = self.from_f64(r);
                    self.set(*d, r);
                }
                IrOp::NegNum(d, a) => {
                    let x = self.to_f64(self.get(*a)?);
                    let r = LLVMBuildFNeg(self.builder, x, NONAME);
                    let r = self.from_f64(r);
                    self.set(*d, r);
                }

                // Everything else: the runtime.
                IrOp::AddAny(d, a, b) => self.any_arith(*d, *a, *b, LLVMBuildFAdd, "ot_add_any")?,
                IrOp::SubAny(d, a, b) => self.any_arith(*d, *a, *b, LLVMBuildFSub, "ot_sub_any")?,
                IrOp::MulAny(d, a, b) => self.any_arith(*d, *a, *b, LLVMBuildFMul, "ot_mul_any")?,
                IrOp::DivAny(d, a, b) => self.any_arith(*d, *a, *b, LLVMBuildFDiv, "ot_div_any")?,
                IrOp::ModAny(d, a, b) => self.any_arith(*d, *a, *b, LLVMBuildFRem, "ot_mod_any")?,
                IrOp::Pow(d, a, b) => self.rt_binop(*d, "ot_pow", *a, *b)?,
                IrOp::BitAnd(d, a, b) => self.int_binop(*d, *a, *b, IntOp::And, "ot_bit_and")?,
                IrOp::BitOr(d, a, b) => self.int_binop(*d, *a, *b, IntOp::Or, "ot_bit_or")?,
                IrOp::Xor(d, a, b) => self.int_binop(*d, *a, *b, IntOp::Xor, "ot_bit_xor")?,
                IrOp::Shl(d, a, b) => self.int_binop(*d, *a, *b, IntOp::Shl, "ot_shl")?,
                IrOp::Shr(d, a, b) => self.int_binop(*d, *a, *b, IntOp::Shr, "ot_shr")?,
                IrOp::ShrU(d, a, b) => self.int_binop(*d, *a, *b, IntOp::ShrU, "ot_shr_u")?,
                IrOp::NegAny(d, a) => {
                    let r = self.rt("ot_neg", &[self.get(*a)?]);
                    self.set(*d, r);
                }

                IrOp::EqStrict(d, a, b) | IrOp::NeStrict(d, a, b) => {
                    let r = if self.is_num(*a) && self.is_num(*b) {
                        let x = self.to_f64(self.get(*a)?);
                        let y = self.to_f64(self.get(*b)?);
                        let cmp = LLVMBuildFCmp(
                            self.builder,
                            LLVMRealPredicate::LLVMRealOEQ,
                            x,
                            y,
                            NONAME,
                        );
                        self.box_bool(cmp)
                    } else {
                        let (x, y) = (self.get(*a)?, self.get(*b)?);
                        self.strict_eq(x, y)
                    };
                    let r = if matches!(op, IrOp::NeStrict(..)) {
                        self.negate(r)
                    } else {
                        r
                    };
                    self.set(*d, r);
                }
                IrOp::EqLoose(d, a, b) | IrOp::NeLoose(d, a, b) => {
                    let r = self.rt("ot_eq_loose", &[self.get(*a)?, self.get(*b)?]);
                    let r = if matches!(op, IrOp::NeLoose(..)) {
                        self.negate(r)
                    } else {
                        r
                    };
                    self.set(*d, r);
                }
                IrOp::Lt(d, a, b) => {
                    self.compare(*d, *a, *b, LLVMRealPredicate::LLVMRealOLT, "ot_lt")?
                }
                IrOp::LtEq(d, a, b) => {
                    self.compare(*d, *a, *b, LLVMRealPredicate::LLVMRealOLE, "ot_lte")?
                }
                IrOp::Gt(d, a, b) => {
                    self.compare(*d, *a, *b, LLVMRealPredicate::LLVMRealOGT, "ot_gt")?
                }
                IrOp::GtEq(d, a, b) => {
                    self.compare(*d, *a, *b, LLVMRealPredicate::LLVMRealOGE, "ot_gte")?
                }

                IrOp::Not(d, a) => {
                    let r = if self.is_bool(*a) {
                        let v = self.get(*a)?;
                        self.negate(v)
                    } else {
                        self.rt("ot_not", &[self.get(*a)?])
                    };
                    self.set(*d, r);
                }
                IrOp::And(d, a, b) | IrOp::Or(d, a, b) => {
                    // a && b: a if falsy, else b.  a || b: a if truthy, else b.
                    let t = self.truthy(*a)?;
                    let (x, y) = (self.get(*a)?, self.get(*b)?);
                    let r = if matches!(op, IrOp::And(..)) {
                        LLVMBuildSelect(self.builder, t, y, x, NONAME)
                    } else {
                        LLVMBuildSelect(self.builder, t, x, y, NONAME)
                    };
                    self.set(*d, r);
                }
                IrOp::ToBool(d, a) => {
                    let r = self.rt("ot_to_boolean", &[self.get(*a)?]);
                    self.set(*d, r);
                }
                IrOp::ToNum(d, a) => {
                    let r = self.rt("ot_to_number", &[self.get(*a)?]);
                    self.set(*d, r);
                }
                IrOp::ToStr(d, a) => {
                    let r = self.rt("ot_to_str", &[self.get(*a)?]);
                    self.set(*d, r);
                }
                IrOp::TypeOf(d, a) => {
                    let r = self.rt("ot_typeof", &[self.get(*a)?]);
                    self.set(*d, r);
                }

                IrOp::LoadLocal(d, slot) => {
                    let p = *self.locals.get(*slot as usize).ok_or_else(|| {
                        BackendError::Llvm(format!("internal error: bad local slot {}", slot))
                    })?;
                    let v = LLVMBuildLoad2(self.builder, self.i64_ty(), p, NONAME);
                    self.set(*d, v);
                }
                IrOp::StoreLocal(slot, v) => {
                    let p = *self.locals.get(*slot as usize).ok_or_else(|| {
                        BackendError::Llvm(format!("internal error: bad local slot {}", slot))
                    })?;
                    LLVMBuildStore(self.builder, self.get(*v)?, p);
                }
                IrOp::LoadGlobal(d, name) => {
                    let r = match self.cg.global_slots.get(name).copied() {
                        // A global of the program: its slot, unless not yet
                        // assigned (then the runtime: a built-in, or "not
                        // defined").
                        Some(slot) => {
                            let v = LLVMBuildLoad2(self.builder, self.i64_ty(), slot, NONAME);
                            let unbound = LLVMBuildICmp(
                                self.builder,
                                LLVMIntPredicate::LLVMIntEQ,
                                v,
                                self.c(UNBOUND_BITS),
                                NONAME,
                            );
                            let ctx = self.cg.context;
                            let slow = LLVMAppendBasicBlockInContext(
                                ctx,
                                self.function,
                                b"global_unbound\0".as_ptr() as _,
                            );
                            let join = LLVMAppendBasicBlockInContext(
                                ctx,
                                self.function,
                                b"global\0".as_ptr() as _,
                            );
                            let fast_end = LLVMGetInsertBlock(self.builder);
                            LLVMBuildCondBr(self.builder, unbound, slow, join);
                            LLVMPositionBuilderAtEnd(self.builder, slow);
                            let (p, n) = self.string(name);
                            let r = self.rt("ot_get_global", &[p, n]);
                            self.check_exception(r, self.unwind);
                            self.last_call = None;
                            let slow_end = LLVMGetInsertBlock(self.builder);
                            LLVMBuildBr(self.builder, join);
                            LLVMPositionBuilderAtEnd(self.builder, join);
                            let phi = LLVMBuildPhi(self.builder, self.i64_ty(), NONAME);
                            let mut values = [v, r];
                            let mut blocks = [fast_end, slow_end];
                            LLVMAddIncoming(phi, values.as_mut_ptr(), blocks.as_mut_ptr(), 2);
                            phi
                        }
                        None => {
                            let (p, n) = self.string(name);
                            self.rt("ot_get_global", &[p, n])
                        }
                    };
                    self.set(*d, r);
                }
                IrOp::TypeOfGlobal(d, name) => {
                    let (p, n) = self.string(name);
                    let r = self.rt("ot_typeof_global", &[p, n]);
                    self.set(*d, r);
                }
                IrOp::StoreGlobal(name, v) => {
                    let v = self.get(*v)?;
                    match self.cg.global_slots.get(name).copied() {
                        Some(slot) => {
                            LLVMBuildStore(self.builder, v, slot);
                        }
                        None => {
                            let (p, n) = self.string(name);
                            self.rt("ot_set_global", &[p, n, v]);
                        }
                    }
                }
                IrOp::LoadEnv(d) => {
                    let e = self.env;
                    self.set(*d, e);
                }
                IrOp::LoadThis(d) => {
                    let t = self.this;
                    self.set(*d, t);
                }

                IrOp::Copy(d, s)
                | IrOp::Move(d, s)
                | IrOp::Borrow(d, s)
                | IrOp::BorrowMut(d, s) => {
                    let v = self.get(*s)?;
                    self.set(*d, v);
                    if let Some(addr) = self.direct.get(s).copied() {
                        self.direct.insert(*d, addr);
                    }
                }

                IrOp::NewObject(d) => {
                    let r = match self.object_slots.get(d).copied() {
                        Some(n) if self.literals.contains_key(d) => {
                            let names = self.literals[d].clone();
                            let cache = self.literal_cache(&names, n);
                            self.rt("ot_alloc_literal", &[cache])
                        }
                        Some(n) => self.rt("ot_alloc_object_sized", &[self.c(n as u64)]),
                        None => self.rt("ot_alloc_object", &[]),
                    };
                    self.set(*d, r);
                }
                IrOp::NewArray(d) => {
                    let r = self.rt("ot_alloc_array", &[self.c(4)]);
                    self.set(*d, r);
                }
                IrOp::GetProp(d, obj, name) => {
                    let o = self.get(*obj)?;
                    let r = if name == "length" {
                        self.length(o)
                    } else {
                        self.cached_prop(o, name, None)
                    };
                    self.set(*d, r);
                }
                IrOp::SetProp(obj, name, v) => {
                    let o = self.get(*obj)?;
                    let v = self.get(*v)?;
                    // One of a literal's own properties: its slot is known.
                    let done = self.literal_stores.get(obj).copied().unwrap_or(0);
                    if self
                        .literals
                        .get(obj)
                        .is_some_and(|names| done < names.len())
                    {
                        debug_assert_eq!(&self.literals[obj][done], name);
                        self.literal_stores.insert(*obj, done + 1);
                        self.store_slot(o, done, v);
                    } else {
                        self.cached_prop(o, name, Some(v));
                    }
                }
                IrOp::GetPrivate(d, obj, name) => {
                    let o = self.get(*obj)?;
                    let (p, n) = self.string(name);
                    let r = self.rt("ot_get_private", &[o, p, n]);
                    self.set(*d, r);
                }
                IrOp::SetPrivate(obj, name, v) => {
                    let o = self.get(*obj)?;
                    let v = self.get(*v)?;
                    let (p, n) = self.string(name);
                    self.rt("ot_set_private", &[o, p, n, v]);
                }
                IrOp::DeleteProp(d, obj, name) => {
                    let o = self.get(*obj)?;
                    let (p, n) = self.string(name);
                    let r = self.rt("ot_delete_prop", &[o, p, n]);
                    self.set(*d, r);
                }
                IrOp::GetElement(d, obj, key) => {
                    let (o, k) = (self.get(*obj)?, self.get(*key)?);
                    let r = self.element(o, k, None);
                    self.set(*d, r);
                }
                IrOp::SetElement(obj, key, v) => {
                    let (o, k, v) = (self.get(*obj)?, self.get(*key)?, self.get(*v)?);
                    self.element(o, k, Some(v));
                }
                IrOp::ArrayLen(d, a) => {
                    let r = self.rt("ot_array_len", &[self.get(*a)?]);
                    self.set(*d, r);
                }
                IrOp::ArrayPush(a, v) => {
                    let args = [self.get(*a)?, self.get(*v)?];
                    self.rt("ot_array_push", &args);
                }

                IrOp::MakeClosure(d, addr, env) => {
                    let name = format!("func_{}", addr);
                    let f = *self.cg.functions.get(&name).ok_or_else(|| {
                        BackendError::Llvm(format!("internal error: no function {}", name))
                    })?;
                    let e = self.get(*env)?;
                    let v = self.rt("ot_make_closure", &[f, e]);
                    self.set(*d, v);
                }
                IrOp::Call(d, callee, args) => {
                    let vals: Vec<LLVMValueRef> = args
                        .iter()
                        .map(|a| self.get(*a))
                        .collect::<Result<_, _>>()?;
                    let undefined = self.c(UNDEFINED);
                    // A callee that is a known function: its fast entry,
                    // with the arguments in registers, if it has one.
                    let known = self.direct.get(callee).copied();
                    if let Some((addr, env)) = known {
                        let env = match env {
                            Some(e) => self.get(e)?,
                            None => undefined,
                        };
                        if let Some(r) = self.call_fast(&format!("func_{}", addr), &vals, env) {
                            self.set(*d, r);
                            return Ok(());
                        }
                    }
                    let (argv, argc) = self.arg_array(&vals);
                    let target = known.and_then(|(addr, env)| {
                        self.cg
                            .functions
                            .get(&format!("func_{}", addr))
                            .map(|f| (*f, env))
                    });
                    let r = match target {
                        Some((f, env)) => {
                            let env = match env {
                                Some(e) => self.get(e)?,
                                None => undefined,
                            };
                            let mut a = [undefined, argv, argc, env];
                            let r = LLVMBuildCall2(
                                self.builder,
                                self.cg.fn_ty,
                                f,
                                a.as_mut_ptr(),
                                4,
                                NONAME,
                            );
                            self.last_call = Some(r);
                            r
                        }
                        None => {
                            let f = self.get(*callee)?;
                            self.rt("ot_call", &[f, undefined, argv, argc])
                        }
                    };
                    self.set(*d, r);
                }
                IrOp::Throw(v) => {
                    let v = self.get(*v)?;
                    self.rt("ot_throw", &[v]);
                }
                IrOp::Construct(d, ctor, args) => {
                    let c = self.get(*ctor)?;
                    let vals: Vec<LLVMValueRef> = args
                        .iter()
                        .map(|a| self.get(*a))
                        .collect::<Result<_, _>>()?;
                    let (argv, argc) = self.arg_array(&vals);
                    let cache = self.new_cache();
                    let r = self.rt("ot_construct_ic", &[c, argv, argc, cache]);
                    self.set(*d, r);
                }
                IrOp::CallSuper(d, parent, args) => {
                    let p = self.get(*parent)?;
                    let vals: Vec<LLVMValueRef> = args
                        .iter()
                        .map(|a| self.get(*a))
                        .collect::<Result<_, _>>()?;
                    let (argv, argc) = self.arg_array(&vals);
                    let this = self.this;
                    let r = self.rt("ot_call_super", &[p, this, argv, argc]);
                    self.set(*d, r);
                }
                IrOp::SuperForward(d, parent) => {
                    let p = self.get(*parent)?;
                    let (argv, argc) = self.args;
                    let this = self.this;
                    let r = self.rt("ot_call_super", &[p, this, argv, argc]);
                    self.set(*d, r);
                }
                IrOp::CallWithThis(d, func, this, args) => {
                    let f = self.get(*func)?;
                    let t = self.get(*this)?;
                    let vals: Vec<LLVMValueRef> = args
                        .iter()
                        .map(|a| self.get(*a))
                        .collect::<Result<_, _>>()?;
                    let (argv, argc) = self.arg_array(&vals);
                    let r = self.rt("ot_call", &[f, t, argv, argc]);
                    self.set(*d, r);
                }
                IrOp::In(d, k, o) => {
                    let args = [self.get(*k)?, self.get(*o)?];
                    let r = self.rt("ot_in", &args);
                    self.set(*d, r);
                }
                IrOp::InstanceOf(d, v, c) => {
                    let args = [self.get(*v)?, self.get(*c)?];
                    let r = self.rt("ot_instanceof", &args);
                    self.set(*d, r);
                }
                IrOp::GetSuperProp(d, this, base, name) => {
                    let t = self.get(*this)?;
                    let o = self.get(*base)?;
                    let (p, n) = self.string(name);
                    let r = self.rt("ot_get_super_prop", &[t, o, p, n]);
                    self.set(*d, r);
                }
                IrOp::TakeException(d) => {
                    let r = self.rt("ot_take_exception", &[]);
                    self.set(*d, r);
                }
                IrOp::Rethrow(thrown, v) => {
                    let args = [self.get(*thrown)?, self.get(*v)?];
                    self.rt("ot_rethrow", &args);
                }
                IrOp::Loc(n) => {
                    self.site = (*n != crate::ir::lower::NO_SITE).then_some(*n);
                    if let (Some(scope), Some(di)) = (&self.di_scope, self.cg.di.as_mut()) {
                        let site = self.site.and_then(|s| self.cg.site_info.get(s as usize));
                        let loc = di.location(scope, site);
                        LLVMSetCurrentDebugLocation2(self.builder, loc);
                    }
                }
                IrOp::RestArgs(d, n) => {
                    let (argv, argc) = self.args;
                    let r = self.rt("ot_rest_args", &[argv, argc, self.c(*n as u64)]);
                    self.set(*d, r);
                }
                IrOp::AsyncStart(d) => {
                    let r = self.rt("ot_async_start", &[]);
                    self.set(*d, r);
                }
                IrOp::AsyncResolve(d, p, v)
                | IrOp::AsyncReject(d, p, v)
                | IrOp::ArraySpread(d, p, v)
                | IrOp::ObjectSpread(d, p, v) => {
                    let name = match op {
                        IrOp::AsyncResolve(..) => "ot_async_resolve",
                        IrOp::AsyncReject(..) => "ot_async_reject",
                        IrOp::ArraySpread(..) => "ot_array_spread",
                        _ => "ot_object_spread",
                    };
                    let args = [self.get(*p)?, self.get(*v)?];
                    let r = self.rt(name, &args);
                    self.set(*d, r);
                }
                IrOp::IsResume(d) => {
                    let resumed = LLVMBuildICmp(
                        self.builder,
                        LLVMIntPredicate::LLVMIntEQ,
                        self.args.1,
                        self.c(crate::runtime::stubs::ASYNC_RESUME_ARGC as u64),
                        NONAME,
                    );
                    let r = self.box_bool(resumed);
                    self.set(*d, r);
                }
                IrOp::ResumePoint(d) => {
                    let (_, frame) = self.frame.unwrap();
                    let r = LLVMBuildLoad2(self.builder, self.i64_ty(), frame, NONAME);
                    self.set(*d, r);
                }
                IrOp::Await(v, k) => {
                    let (frame, frame_ptr) = self.frame.unwrap();
                    LLVMBuildStore(self.builder, self.c((*k as f64).to_bits()), frame_ptr);
                    let code =
                        LLVMBuildPtrToInt(self.builder, self.function, self.i64_ty(), NONAME);
                    let args = [self.get(*v)?, frame, code, self.this, self.env];
                    self.rt("ot_await", &args);
                }
                IrOp::AwaitResult(d) => {
                    let (frame, _) = self.frame.unwrap();
                    let r = self.rt("ot_await_result", &[frame]);
                    self.set(*d, r);
                }
                IrOp::MathCall(d, func, args) => {
                    let r = self.math_call(*func, args)?;
                    self.set(*d, r);
                }
                IrOp::CheckType(d, src, kinds) => {
                    let v = self.get(*src)?;
                    let r = self.check_type(v, *kinds);
                    self.set(*d, r);
                }
                IrOp::Intrinsic(d, id, args) => {
                    let vals: Vec<LLVMValueRef> = args
                        .iter()
                        .map(|a| self.get(*a))
                        .collect::<Result<_, _>>()?;
                    let (argv, argc) = self.arg_array(&vals);
                    let v = self.rt("ot_intrinsic", &[self.c(*id as u64), argv, argc]);
                    self.set(*d, v);
                }
                IrOp::CallMethod(d, recv, name, args) => {
                    let r = self.get(*recv)?;
                    let vals: Vec<LLVMValueRef> = args
                        .iter()
                        .map(|a| self.get(*a))
                        .collect::<Result<_, _>>()?;
                    let (argv, argc) = self.arg_array(&vals);
                    let v = self.call_method(r, name, argv, argc);
                    self.set(*d, v);
                }

                other => {
                    return Err(BackendError::UnsupportedOp(format!(
                        "the LLVM backend does not support `{}` yet",
                        other
                    )));
                }
            }
            Ok(())
        }
    }

    pub(super) unsafe fn terminator(&mut self, term: &Terminator) -> Result<(), BackendError> {
        unsafe {
            match term {
                Terminator::Jump(t) => {
                    LLVMBuildBr(self.builder, self.blocks[t]);
                }
                Terminator::Branch(cond, t, f) => {
                    let c = self.truthy(*cond)?;
                    LLVMBuildCondBr(self.builder, c, self.blocks[t], self.blocks[f]);
                }
                Terminator::Return(v) => {
                    let r = match v {
                        Some(v) => self.get(*v)?,
                        None => self.c(UNDEFINED),
                    };
                    LLVMBuildRet(self.builder, r);
                }
                Terminator::Unreachable => {
                    LLVMBuildUnreachable(self.builder);
                }
            }
            Ok(())
        }
    }
}
