//! One IR instruction, or one block terminator, to Cranelift code.

use super::*;

impl Fx<'_> {
    pub(super) fn op(&mut self, b: &mut FunctionBuilder, op: &IrOp) -> Result<(), BackendError> {
        match op {
            IrOp::Const(dst, lit) => {
                let v = self.literal(b, lit)?;
                self.values.insert(*dst, v);
            }

            IrOp::AddNum(d, x, y) => self.num_binop(b, *d, *x, *y, |b, p, q| b.ins().fadd(p, q))?,
            IrOp::SubNum(d, x, y) => self.num_binop(b, *d, *x, *y, |b, p, q| b.ins().fsub(p, q))?,
            IrOp::MulNum(d, x, y) => self.num_binop(b, *d, *x, *y, |b, p, q| b.ins().fmul(p, q))?,
            IrOp::DivNum(d, x, y) => self.num_binop(b, *d, *x, *y, |b, p, q| b.ins().fdiv(p, q))?,
            IrOp::ModNum(d, x, y) | IrOp::ModAny(d, x, y) => self.js_mod(b, *d, *x, *y)?,
            IrOp::NegNum(d, x) => {
                let v = self.get(*x)?;
                let f = Self::to_f64(b, v);
                let r = b.ins().fneg(f);
                let r = Self::from_f64(b, r);
                self.values.insert(*d, r);
            }

            IrOp::AddAny(d, x, y) => {
                self.any_arith(b, *d, *x, *y, "ot_add_any", |b, p, q| b.ins().fadd(p, q))?
            }
            IrOp::SubAny(d, x, y) => {
                self.any_arith(b, *d, *x, *y, "ot_sub_any", |b, p, q| b.ins().fsub(p, q))?
            }
            IrOp::MulAny(d, x, y) => {
                self.any_arith(b, *d, *x, *y, "ot_mul_any", |b, p, q| b.ins().fmul(p, q))?
            }
            IrOp::DivAny(d, x, y) => {
                self.any_arith(b, *d, *x, *y, "ot_div_any", |b, p, q| b.ins().fdiv(p, q))?
            }
            IrOp::Pow(d, x, y) => self.rt_binop(b, *d, "ot_pow", *x, *y)?,
            IrOp::BitAnd(d, x, y) => {
                self.int_binop(b, *d, *x, *y, "ot_bit_and", |b, p, q| b.ins().band(p, q))?
            }
            IrOp::BitOr(d, x, y) => {
                self.int_binop(b, *d, *x, *y, "ot_bit_or", |b, p, q| b.ins().bor(p, q))?
            }
            IrOp::Xor(d, x, y) => {
                self.int_binop(b, *d, *x, *y, "ot_bit_xor", |b, p, q| b.ins().bxor(p, q))?
            }
            IrOp::Shl(d, x, y) => self.int_binop(b, *d, *x, *y, "ot_shl", |b, p, q| {
                let n = b.ins().band_imm(q, 31);
                b.ins().ishl(p, n)
            })?,
            IrOp::Shr(d, x, y) => self.int_binop(b, *d, *x, *y, "ot_shr", |b, p, q| {
                let n = b.ins().band_imm(q, 31);
                b.ins().sshr(p, n)
            })?,
            IrOp::ShrU(d, x, y) => self.int_binop(b, *d, *x, *y, "ot_shr_u", |b, p, q| {
                let n = b.ins().band_imm(q, 31);
                b.ins().ushr(p, n)
            })?,
            IrOp::NegAny(d, x) => self.rt_unop(b, *d, "ot_neg", *x)?,

            IrOp::EqStrict(d, x, y) | IrOp::NeStrict(d, x, y) => {
                let r = if self.is_num(*x) && self.is_num(*y) {
                    let xv = self.get(*x)?;
                    let yv = self.get(*y)?;
                    let xf = Self::to_f64(b, xv);
                    let yf = Self::to_f64(b, yv);
                    let cmp = b.ins().fcmp(FloatCC::Equal, xf, yf);
                    Self::box_bool(b, cmp)
                } else {
                    let (xv, yv) = (self.get(*x)?, self.get(*y)?);
                    self.strict_eq(b, xv, yv)
                };
                let r = if matches!(op, IrOp::NeStrict(..)) {
                    b.ins().bxor_imm(r, 1)
                } else {
                    r
                };
                self.values.insert(*d, r);
            }
            IrOp::EqLoose(d, x, y) | IrOp::NeLoose(d, x, y) => {
                let args = [self.get(*x)?, self.get(*y)?];
                let r = self.rt_v(b, "ot_eq_loose", &args);
                let r = if matches!(op, IrOp::NeLoose(..)) {
                    b.ins().bxor_imm(r, 1)
                } else {
                    r
                };
                self.values.insert(*d, r);
            }
            IrOp::Lt(d, x, y) => self.compare(b, *d, *x, *y, FloatCC::LessThan, "ot_lt")?,
            IrOp::LtEq(d, x, y) => {
                self.compare(b, *d, *x, *y, FloatCC::LessThanOrEqual, "ot_lte")?
            }
            IrOp::Gt(d, x, y) => self.compare(b, *d, *x, *y, FloatCC::GreaterThan, "ot_gt")?,
            IrOp::GtEq(d, x, y) => {
                self.compare(b, *d, *x, *y, FloatCC::GreaterThanOrEqual, "ot_gte")?
            }

            IrOp::Not(d, x) => {
                if self.is_bool(*x) {
                    let v = self.get(*x)?;
                    let r = b.ins().bxor_imm(v, 1);
                    self.values.insert(*d, r);
                } else {
                    self.rt_unop(b, *d, "ot_not", *x)?;
                }
            }
            IrOp::And(d, x, y) | IrOp::Or(d, x, y) => {
                let t = self.truthy(b, *x)?;
                let (xv, yv) = (self.get(*x)?, self.get(*y)?);
                let r = if matches!(op, IrOp::And(..)) {
                    b.ins().select(t, yv, xv)
                } else {
                    b.ins().select(t, xv, yv)
                };
                self.values.insert(*d, r);
            }
            IrOp::ToBool(d, x) => self.rt_unop(b, *d, "ot_to_boolean", *x)?,
            IrOp::ToNum(d, x) => self.rt_unop(b, *d, "ot_to_number", *x)?,
            IrOp::ToStr(d, x) => self.rt_unop(b, *d, "ot_to_str", *x)?,
            IrOp::TypeOf(d, x) => self.rt_unop(b, *d, "ot_typeof", *x)?,

            IrOp::LoadLocal(d, slot) => {
                let s = *self.locals.get(*slot as usize).ok_or_else(|| {
                    BackendError::Cranelift(format!("internal error: bad local slot {}", slot))
                })?;
                let v = match s {
                    Local::Slot(s) => b.ins().stack_load(types::I64, s, 0),
                    Local::Frame(o) => {
                        b.ins()
                            .load(types::I64, MemFlags::trusted(), self.frame.unwrap(), o)
                    }
                };
                self.values.insert(*d, v);
            }
            IrOp::StoreLocal(slot, v) => {
                let s = *self.locals.get(*slot as usize).ok_or_else(|| {
                    BackendError::Cranelift(format!("internal error: bad local slot {}", slot))
                })?;
                let v = self.get(*v)?;
                match s {
                    Local::Slot(s) => {
                        b.ins().stack_store(v, s, 0);
                    }
                    Local::Frame(o) => {
                        b.ins()
                            .store(MemFlags::trusted(), v, self.frame.unwrap(), o);
                    }
                }
            }
            IrOp::LoadGlobal(d, name) => {
                let r = match self.global_slots.get(name).copied() {
                    // A global of the program: its slot, unless not yet
                    // assigned (then the runtime: a built-in, or "not
                    // defined").
                    Some(data) => {
                        let gv = self.module.declare_data_in_func(data, b.func);
                        let p = b.ins().global_value(types::I64, gv);
                        let v = b.ins().load(types::I64, MemFlags::trusted(), p, 0);
                        let unbound = b.ins().icmp_imm(IntCC::Equal, v, UNBOUND_BITS as i64);
                        let slow = b.create_block();
                        let join = b.create_block();
                        b.set_cold_block(slow);
                        b.append_block_param(join, types::I64);
                        b.ins().brif(unbound, slow, &[], join, &[v]);
                        b.switch_to_block(slow);
                        b.seal_block(slow);
                        let (np, nn) = self.name(b, name);
                        let r = self.rt_v(b, "ot_get_global", &[np, nn]);
                        let unwind = self.unwind;
                        self.check_exception(b, r, unwind);
                        self.last_call = None;
                        b.ins().jump(join, &[r]);
                        b.switch_to_block(join);
                        b.seal_block(join);
                        b.block_params(join)[0]
                    }
                    None => {
                        let (p, n) = self.name(b, name);
                        self.rt_v(b, "ot_get_global", &[p, n])
                    }
                };
                self.values.insert(*d, r);
            }
            IrOp::TypeOfGlobal(d, name) => {
                let (p, n) = self.name(b, name);
                let r = self.rt_v(b, "ot_typeof_global", &[p, n]);
                self.values.insert(*d, r);
            }
            IrOp::StoreGlobal(name, v) => {
                let v = self.get(*v)?;
                match self.global_slots.get(name).copied() {
                    Some(data) => {
                        let gv = self.module.declare_data_in_func(data, b.func);
                        let p = b.ins().global_value(types::I64, gv);
                        b.ins().store(MemFlags::trusted(), v, p, 0);
                    }
                    None => {
                        let (p, n) = self.name(b, name);
                        self.rt(b, "ot_set_global", &[p, n, v]);
                    }
                }
            }
            IrOp::LoadThis(d) => {
                let t = self.this.expect("this is set in the prologue");
                self.values.insert(*d, t);
            }
            IrOp::LoadEnv(d) => {
                let e = self.env.expect("env is set in the prologue");
                self.values.insert(*d, e);
            }

            IrOp::Copy(d, s) | IrOp::Move(d, s) | IrOp::Borrow(d, s) | IrOp::BorrowMut(d, s) => {
                let v = self.get(*s)?;
                self.values.insert(*d, v);
                if let Some(addr) = self.direct.get(s).copied() {
                    self.direct.insert(*d, addr);
                }
            }

            IrOp::NewObject(d) => {
                let slots = self.object_slots.get(d).copied();
                let r = match slots {
                    Some(n) if self.literals.contains_key(d) => {
                        let cache = Self::literal_cache(b, &self.literals[d], n);
                        self.rt_v(b, "ot_alloc_literal", &[cache])
                    }
                    Some(n) => {
                        let n = Self::c(b, n as u64);
                        self.rt_v(b, "ot_alloc_object_sized", &[n])
                    }
                    None => self.rt_v(b, "ot_alloc_object", &[]),
                };
                self.values.insert(*d, r);
            }
            IrOp::NewArray(d) => {
                let cap = Self::c(b, 4);
                let r = self.rt_v(b, "ot_alloc_array", &[cap]);
                self.values.insert(*d, r);
            }
            IrOp::GetProp(d, obj, name) => {
                let o = self.get(*obj)?;
                let r = if name == "length" {
                    self.length(b, o)
                } else {
                    self.cached_prop(b, o, name, None)
                };
                self.values.insert(*d, r);
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
                    let addr = b.ins().band_imm(o, 0x0000_FFFF_FFFF_FFFF);
                    let offset = crate::runtime::object::ic::slot_offset(done, done + 1);
                    b.ins().store(MemFlags::trusted(), v, addr, offset);
                } else {
                    self.cached_prop(b, o, name, Some(v));
                }
            }
            IrOp::GetPrivate(d, obj, name) => {
                let o = self.get(*obj)?;
                let (p, n) = self.name(b, name);
                let r = self.rt_v(b, "ot_get_private", &[o, p, n]);
                self.values.insert(*d, r);
            }
            IrOp::SetPrivate(obj, name, v) => {
                let o = self.get(*obj)?;
                let v = self.get(*v)?;
                let (p, n) = self.name(b, name);
                self.rt(b, "ot_set_private", &[o, p, n, v]);
            }
            IrOp::DeleteProp(d, obj, name) => {
                let o = self.get(*obj)?;
                let (p, n) = self.name(b, name);
                let r = self.rt_v(b, "ot_delete_prop", &[o, p, n]);
                self.values.insert(*d, r);
            }
            IrOp::GetElement(d, obj, key) => {
                let (o, k) = (self.get(*obj)?, self.get(*key)?);
                let r = self.element(b, o, k, None);
                self.values.insert(*d, r);
            }
            IrOp::SetElement(obj, key, v) => {
                let (o, k, v) = (self.get(*obj)?, self.get(*key)?, self.get(*v)?);
                self.element(b, o, k, Some(v));
            }
            IrOp::ArrayLen(d, a) => self.rt_unop(b, *d, "ot_array_len", *a)?,
            IrOp::ArrayPush(a, v) => {
                let args = [self.get(*a)?, self.get(*v)?];
                self.rt(b, "ot_array_push", &args);
            }

            IrOp::MakeClosure(d, addr, env) => {
                let name = format!("func_{}", addr);
                let id = *self.func_ids.get(&name).ok_or_else(|| {
                    BackendError::Cranelift(format!("internal error: no function {}", name))
                })?;
                let fref = self.module.declare_func_in_func(id, b.func);
                let code = b.ins().func_addr(types::I64, fref);
                let e = self.get(*env)?;
                let v = self.rt_v(b, "ot_make_closure", &[code, e]);
                self.values.insert(*d, v);
            }
            IrOp::Call(d, callee, args) => {
                let vals: Vec<Value> = args
                    .iter()
                    .map(|a| self.get(*a))
                    .collect::<Result<_, _>>()?;
                let (argv, argc) = self.arg_array(b, &vals);
                let undefined = Self::c(b, UNDEFINED);
                let target = self.direct.get(callee).copied().and_then(|(addr, env)| {
                    self.func_ids
                        .get(&format!("func_{}", addr))
                        .map(|id| (*id, env))
                });
                let r = match target {
                    Some((id, env)) => {
                        let env = match env {
                            Some(e) => self.get(e)?,
                            None => undefined,
                        };
                        let fref = self.module.declare_func_in_func(id, b.func);
                        let call = b.ins().call(fref, &[undefined, argv, argc, env]);
                        let r = b.inst_results(call)[0];
                        self.last_call = Some(r);
                        r
                    }
                    None => {
                        let f = self.get(*callee)?;
                        self.rt_v(b, "ot_call", &[f, undefined, argv, argc])
                    }
                };
                self.values.insert(*d, r);
            }
            IrOp::Throw(v) => {
                let v = self.get(*v)?;
                self.rt(b, "ot_throw", &[v]);
            }
            IrOp::Construct(d, ctor, args) => {
                let c = self.get(*ctor)?;
                let vals: Vec<Value> = args
                    .iter()
                    .map(|a| self.get(*a))
                    .collect::<Result<_, _>>()?;
                let (argv, argc) = self.arg_array(b, &vals);
                let cache = Box::leak(Box::new(crate::runtime::object::ic::NewCache::new()));
                let cache = b.ins().iconst(types::I64, cache as *mut _ as i64);
                let r = self.rt_v(b, "ot_construct_ic", &[c, argv, argc, cache]);
                self.values.insert(*d, r);
            }
            IrOp::CallSuper(d, parent, args) => {
                let p = self.get(*parent)?;
                let vals: Vec<Value> = args
                    .iter()
                    .map(|a| self.get(*a))
                    .collect::<Result<_, _>>()?;
                let (argv, argc) = self.arg_array(b, &vals);
                let this = self.this.unwrap();
                let r = self.rt_v(b, "ot_call_super", &[p, this, argv, argc]);
                self.values.insert(*d, r);
            }
            IrOp::SuperForward(d, parent) => {
                let p = self.get(*parent)?;
                let (argv, argc) = self.args.unwrap();
                let this = self.this.unwrap();
                let r = self.rt_v(b, "ot_call_super", &[p, this, argv, argc]);
                self.values.insert(*d, r);
            }
            IrOp::CallWithThis(d, func, this, args) => {
                let f = self.get(*func)?;
                let t = self.get(*this)?;
                let vals: Vec<Value> = args
                    .iter()
                    .map(|a| self.get(*a))
                    .collect::<Result<_, _>>()?;
                let (argv, argc) = self.arg_array(b, &vals);
                let r = self.rt_v(b, "ot_call", &[f, t, argv, argc]);
                self.values.insert(*d, r);
            }
            IrOp::InstanceOf(d, v, c) => self.rt_binop(b, *d, "ot_instanceof", *v, *c)?,
            IrOp::In(d, k, o) => self.rt_binop(b, *d, "ot_in", *k, *o)?,
            IrOp::GetSuperProp(d, this, base, name) => {
                let t = self.get(*this)?;
                let o = self.get(*base)?;
                let (p, n) = self.name(b, name);
                let r = self.rt_v(b, "ot_get_super_prop", &[t, o, p, n]);
                self.values.insert(*d, r);
            }
            IrOp::TakeException(d) => {
                let r = self.rt_v(b, "ot_take_exception", &[]);
                self.values.insert(*d, r);
            }
            IrOp::Rethrow(thrown, v) => {
                let args = [self.get(*thrown)?, self.get(*v)?];
                self.rt(b, "ot_rethrow", &args);
            }
            IrOp::Loc(n) => {
                self.site = (*n != crate::ir::lower::NO_SITE).then_some(*n);
            }
            IrOp::RestArgs(d, n) => {
                let (argv, argc) = self.args.unwrap();
                let n = Self::c(b, *n as u64);
                let r = self.rt_v(b, "ot_rest_args", &[argv, argc, n]);
                self.values.insert(*d, r);
            }
            IrOp::AsyncStart(d) => {
                let r = self.rt_v(b, "ot_async_start", &[]);
                self.values.insert(*d, r);
            }
            IrOp::AsyncResolve(d, p, v) => self.rt_binop(b, *d, "ot_async_resolve", *p, *v)?,
            IrOp::ArraySpread(d, t, s) => self.rt_binop(b, *d, "ot_array_spread", *t, *s)?,
            IrOp::ObjectSpread(d, t, s) => self.rt_binop(b, *d, "ot_object_spread", *t, *s)?,
            IrOp::AsyncReject(d, p, v) => self.rt_binop(b, *d, "ot_async_reject", *p, *v)?,
            IrOp::IsResume(d) => {
                let (_, argc) = self.args.unwrap();
                let resumed = b.ins().icmp_imm(
                    IntCC::Equal,
                    argc,
                    crate::runtime::stubs::ASYNC_RESUME_ARGC as i64,
                );
                let r = Self::box_bool(b, resumed);
                self.values.insert(*d, r);
            }
            IrOp::ResumePoint(d) => {
                let frame = self.frame.unwrap();
                let r = b.ins().load(types::I64, MemFlags::trusted(), frame, 0);
                self.values.insert(*d, r);
            }
            IrOp::Await(v, k) => {
                let frame = self.frame.unwrap();
                let point = Self::c(b, (*k as f64).to_bits());
                b.ins().store(MemFlags::trusted(), point, frame, 0);
                let fref = self
                    .module
                    .declare_func_in_func(self.func_ids[self.own_name], b.func);
                let code = b.ins().func_addr(types::I64, fref);
                let v = self.get(*v)?;
                let (this, env) = (self.this.unwrap(), self.env.unwrap());
                self.rt(b, "ot_await", &[v, frame, code, this, env]);
            }
            IrOp::AwaitResult(d) => {
                let frame = self.frame.unwrap();
                let r = self.rt_v(b, "ot_await_result", &[frame]);
                self.values.insert(*d, r);
            }
            IrOp::MathCall(d, func, args) => {
                let r = self.math_call(b, *func, args)?;
                self.values.insert(*d, r);
            }
            IrOp::Intrinsic(d, id, args) => {
                let vals: Vec<Value> = args
                    .iter()
                    .map(|a| self.get(*a))
                    .collect::<Result<_, _>>()?;
                let (argv, argc) = self.arg_array(b, &vals);
                let id = Self::c(b, *id as u64);
                let v = self.rt_v(b, "ot_intrinsic", &[id, argv, argc]);
                self.values.insert(*d, v);
            }
            IrOp::CallMethod(d, recv, name, args) => {
                let r = self.get(*recv)?;
                let vals: Vec<Value> = args
                    .iter()
                    .map(|a| self.get(*a))
                    .collect::<Result<_, _>>()?;
                let (argv, argc) = self.arg_array(b, &vals);
                let v = self.call_method(b, r, name, argv, argc);
                self.values.insert(*d, v);
            }

            other => {
                return Err(BackendError::UnsupportedOp(format!(
                    "the Cranelift backend does not support `{}` yet",
                    other
                )));
            }
        }
        Ok(())
    }

    pub(super) fn terminator(
        &mut self,
        b: &mut FunctionBuilder,
        term: &Terminator,
    ) -> Result<(), BackendError> {
        match term {
            Terminator::Jump(t) => {
                b.ins().jump(self.blocks[t], &[]);
            }
            Terminator::Branch(cond, t, f) => {
                let c = self.truthy(b, *cond)?;
                b.ins().brif(c, self.blocks[t], &[], self.blocks[f], &[]);
            }
            Terminator::Return(v) => {
                let r = match v {
                    Some(v) => self.get(*v)?,
                    None => Self::c(b, UNDEFINED),
                };
                b.ins().return_(&[r]);
            }
            Terminator::Unreachable => {
                b.ins().trap(TrapCode::user(1).unwrap());
            }
        }
        Ok(())
    }
}
