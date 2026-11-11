//! Arithmetic, comparison and `Math` in generated code: inline on
//! numbers, through the runtime on anything else.

use super::*;

impl Fx<'_> {
    pub(super) fn num_binop(
        &mut self,
        b: &mut FunctionBuilder,
        dst: ValueId,
        x: ValueId,
        y: ValueId,
        f: fn(&mut FunctionBuilder, Value, Value) -> Value,
    ) -> Result<(), BackendError> {
        let xv = self.get(x)?;
        let yv = self.get(y)?;
        let xf = Self::to_f64(b, xv);
        let yf = Self::to_f64(b, yv);
        let r = f(b, xf, yf);
        let r = Self::from_f64(b, r);
        self.values.insert(dst, r);
        Ok(())
    }

    pub(super) fn rt_binop(
        &mut self,
        b: &mut FunctionBuilder,
        dst: ValueId,
        name: &str,
        x: ValueId,
        y: ValueId,
    ) -> Result<(), BackendError> {
        let args = [self.get(x)?, self.get(y)?];
        let r = self.rt_v(b, name, &args);
        self.values.insert(dst, r);
        Ok(())
    }

    pub(super) fn rt_unop(
        &mut self,
        b: &mut FunctionBuilder,
        dst: ValueId,
        name: &str,
        x: ValueId,
    ) -> Result<(), BackendError> {
        let args = [self.get(x)?];
        let r = self.rt_v(b, name, &args);
        self.values.insert(dst, r);
        Ok(())
    }

    /// Whether both values are numbers: neither has the boxed tag pattern.
    pub(super) fn both_numbers(b: &mut FunctionBuilder, x: Value, y: Value) -> Value {
        const QNAN: i64 = 0x7FFC_0000_0000_0000;
        let mx = b.ins().band_imm(x, QNAN);
        let my = b.ins().band_imm(y, QNAN);
        let nx = b.ins().icmp_imm(IntCC::NotEqual, mx, QNAN);
        let ny = b.ins().icmp_imm(IntCC::NotEqual, my, QNAN);
        b.ins().band(nx, ny)
    }

    /// `x op y` on values of unknown type: `fast` inline when both turn out
    /// to be numbers, else the runtime function `stub` (checked for an
    /// exception when `throws`).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn any_binop(
        &mut self,
        b: &mut FunctionBuilder,
        dst: ValueId,
        x: ValueId,
        y: ValueId,
        stub: &str,
        throws: bool,
        fast: &dyn Fn(&mut FunctionBuilder, Value, Value) -> Value,
    ) -> Result<(), BackendError> {
        let xv = self.get(x)?;
        let yv = self.get(y)?;
        let num = b.create_block();
        let any = b.create_block();
        let join = b.create_block();
        b.append_block_param(join, types::I64);
        let both = Self::both_numbers(b, xv, yv);
        b.ins().brif(both, num, &[], any, &[]);

        b.switch_to_block(num);
        b.seal_block(num);
        let r = fast(b, xv, yv);
        b.ins().jump(join, &[r]);

        b.switch_to_block(any);
        b.seal_block(any);
        let r = self.rt_v(b, stub, &[xv, yv]);
        if throws {
            let unwind = self.unwind;
            self.check_exception(b, r, unwind);
        }
        self.last_call = None;
        b.ins().jump(join, &[r]);

        b.switch_to_block(join);
        b.seal_block(join);
        let r = b.block_params(join)[0];
        self.values.insert(dst, r);
        Ok(())
    }

    /// Arithmetic on values of unknown type: floating point inline when both
    /// are numbers.
    pub(super) fn any_arith(
        &mut self,
        b: &mut FunctionBuilder,
        dst: ValueId,
        x: ValueId,
        y: ValueId,
        stub: &str,
        f: fn(&mut FunctionBuilder, Value, Value) -> Value,
    ) -> Result<(), BackendError> {
        if self.is_num(x) && self.is_num(y) {
            return self.num_binop(b, dst, x, y, f);
        }
        self.any_binop(b, dst, x, y, stub, true, &|b, p, q| {
            let pf = Self::to_f64(b, p);
            let qf = Self::to_f64(b, q);
            let r = f(b, pf, qf);
            Self::from_f64(b, r)
        })
    }

    /// `Math.<f>` inline on doubles, for the functions with an instruction.
    pub(super) fn math_inline(b: &mut FunctionBuilder, name: &str, xs: &[Value]) -> Option<Value> {
        Some(match (name, xs.len()) {
            ("sqrt", 1) => b.ins().sqrt(xs[0]),
            ("floor", 1) => b.ins().floor(xs[0]),
            ("ceil", 1) => b.ins().ceil(xs[0]),
            ("trunc", 1) => b.ins().trunc(xs[0]),
            ("abs", 1) => b.ins().fabs(xs[0]),
            // As the runtime: halves round up, towards +Infinity.
            ("round", 1) => {
                let f = b.ins().floor(xs[0]);
                let diff = b.ins().fsub(xs[0], f);
                let half = b.ins().f64const(0.5);
                let up = b.ins().fcmp(FloatCC::GreaterThanOrEqual, diff, half);
                let one = b.ins().f64const(1.0);
                let f1 = b.ins().fadd(f, one);
                b.ins().select(up, f1, f)
            }
            // NaN wins and -0 is below +0, as in JavaScript.
            ("min", 2) => b.ins().fmin(xs[0], xs[1]),
            ("max", 2) => b.ins().fmax(xs[0], xs[1]),
            _ => return None,
        })
    }

    /// `Math.<f>(args)` (`IrOp::MathCall`): inline when every argument is a
    /// number and `f` has an instruction, else `ot_math_call`.
    pub(super) fn math_call(
        &mut self,
        b: &mut FunctionBuilder,
        func: u32,
        args: &[ValueId],
    ) -> Result<Value, BackendError> {
        let name = crate::runtime::builtins::MATH_FUNCTIONS
            .get(func as usize)
            .copied()
            .unwrap_or("");
        let vals: Vec<Value> = args
            .iter()
            .map(|a| self.get(*a))
            .collect::<Result<_, _>>()?;
        let call_runtime = |this: &mut Self, b: &mut FunctionBuilder| {
            let (argv, argc) = this.arg_array(b, &vals);
            let id = Self::c(b, func as u64);
            let r = this.rt_v(b, "ot_math_call", &[id, argv, argc]);
            let unwind = this.unwind;
            this.check_exception(b, r, unwind);
            this.last_call = None;
            r
        };
        let can_inline = matches!(
            (name, vals.len()),
            ("sqrt" | "floor" | "ceil" | "trunc" | "abs" | "round", 1) | ("min" | "max", 2)
        );
        if !can_inline {
            return Ok(call_runtime(self, b));
        }
        let floats: Vec<Value> = vals.iter().map(|v| Self::to_f64(b, *v)).collect();
        if args.iter().all(|a| self.is_num(*a)) {
            let r = Self::math_inline(b, name, &floats).expect("inline Math function");
            return Ok(Self::from_f64(b, r));
        }
        let mut ok = Self::both_numbers(b, vals[0], vals[0]);
        for v in &vals[1..] {
            let n = Self::both_numbers(b, *v, *v);
            ok = b.ins().band(ok, n);
        }
        let fast = b.create_block();
        let slow = b.create_block();
        let join = b.create_block();
        b.append_block_param(join, types::I64);
        b.ins().brif(ok, fast, &[], slow, &[]);
        b.switch_to_block(fast);
        b.seal_block(fast);
        let r = Self::math_inline(b, name, &floats).expect("inline Math function");
        let r = Self::from_f64(b, r);
        b.ins().jump(join, &[r]);
        b.switch_to_block(slow);
        b.seal_block(slow);
        let r = call_runtime(self, b);
        b.ins().jump(join, &[r]);
        b.switch_to_block(join);
        b.seal_block(join);
        Ok(b.block_params(join)[0])
    }

    /// A bitwise operator: `f` on the operands' 32-bit integers when both
    /// are numbers below 2^63 (`ToInt32` is then the saturated conversion,
    /// truncated), else the runtime (`valueOf` may throw). The result of
    /// `>>>` is unsigned.
    pub(super) fn int_binop(
        &mut self,
        b: &mut FunctionBuilder,
        dst: ValueId,
        x: ValueId,
        y: ValueId,
        stub: &str,
        f: fn(&mut FunctionBuilder, Value, Value) -> Value,
    ) -> Result<(), BackendError> {
        let xv = self.get(x)?;
        let yv = self.get(y)?;
        let int32 = |b: &mut FunctionBuilder, v: Value| -> (Value, Value) {
            const LIMIT: f64 = 9_223_372_036_854_775_808.0;
            let f = Self::to_f64(b, v);
            let i = b.ins().fcvt_to_sint_sat(types::I64, f);
            let lo = b.ins().f64const(-LIMIT);
            let hi = b.ins().f64const(LIMIT);
            let above = b.ins().fcmp(FloatCC::GreaterThan, f, lo);
            let below = b.ins().fcmp(FloatCC::LessThan, f, hi);
            (b.ins().ireduce(types::I32, i), b.ins().band(above, below))
        };
        let (xi, xok) = int32(b, xv);
        let (yi, yok) = int32(b, yv);
        let mut ok = b.ins().band(xok, yok);
        if !(self.is_num(x) && self.is_num(y)) {
            let both = Self::both_numbers(b, xv, yv);
            ok = b.ins().band(ok, both);
        }
        let int_blk = b.create_block();
        let any_blk = b.create_block();
        let join = b.create_block();
        b.append_block_param(join, types::I64);
        b.ins().brif(ok, int_blk, &[], any_blk, &[]);

        b.switch_to_block(int_blk);
        b.seal_block(int_blk);
        let r = f(b, xi, yi);
        let rf = if stub == "ot_shr_u" {
            b.ins().fcvt_from_uint(types::F64, r)
        } else {
            b.ins().fcvt_from_sint(types::F64, r)
        };
        let r = Self::from_f64(b, rf);
        b.ins().jump(join, &[r]);

        b.switch_to_block(any_blk);
        b.seal_block(any_blk);
        let r = self.rt_v(b, stub, &[xv, yv]);
        let unwind = self.unwind;
        self.check_exception(b, r, unwind);
        self.last_call = None;
        b.ins().jump(join, &[r]);

        b.switch_to_block(join);
        b.seal_block(join);
        let r = b.block_params(join)[0];
        self.values.insert(dst, r);
        Ok(())
    }

    /// JavaScript `x % y`. When both are numbers, both are integers below
    /// 2^53 and `y` is not 0, an integer remainder with the sign of `x`
    /// (`fcopysign` also gives `-0`); otherwise the runtime (Cranelift has no
    /// floating-point remainder).
    pub(super) fn js_mod(
        &mut self,
        b: &mut FunctionBuilder,
        dst: ValueId,
        x: ValueId,
        y: ValueId,
    ) -> Result<(), BackendError> {
        let xv = self.get(x)?;
        let yv = self.get(y)?;
        let xf = Self::to_f64(b, xv);
        let yf = Self::to_f64(b, yv);
        let exact = |b: &mut FunctionBuilder, f: Value| -> (Value, Value) {
            const LIMIT: i64 = 1 << 53;
            let i = b.ins().fcvt_to_sint_sat(types::I64, f);
            let back = b.ins().fcvt_from_sint(types::F64, i);
            let same = b.ins().fcmp(FloatCC::Equal, back, f);
            let lo = b.ins().icmp_imm(IntCC::SignedGreaterThan, i, -LIMIT);
            let hi = b.ins().icmp_imm(IntCC::SignedLessThan, i, LIMIT);
            let ok = b.ins().band(same, lo);
            (i, b.ins().band(ok, hi))
        };
        let (xi, xok) = exact(b, xf);
        let (yi, yok) = exact(b, yf);
        let nonzero = b.ins().icmp_imm(IntCC::NotEqual, yi, 0);
        let mut ints = b.ins().band(xok, yok);
        ints = b.ins().band(ints, nonzero);
        if !(self.is_num(x) && self.is_num(y)) {
            let both = Self::both_numbers(b, xv, yv);
            ints = b.ins().band(ints, both);
        }
        let int_blk = b.create_block();
        let any_blk = b.create_block();
        let join = b.create_block();
        b.append_block_param(join, types::I64);
        b.ins().brif(ints, int_blk, &[], any_blk, &[]);

        b.switch_to_block(int_blk);
        b.seal_block(int_blk);
        let r = b.ins().srem(xi, yi);
        let rf = b.ins().fcvt_from_sint(types::F64, r);
        let rf = b.ins().fcopysign(rf, xf);
        let r = Self::from_f64(b, rf);
        b.ins().jump(join, &[r]);

        b.switch_to_block(any_blk);
        b.seal_block(any_blk);
        let r = self.rt_v(b, "ot_mod_any", &[xv, yv]);
        let unwind = self.unwind;
        self.check_exception(b, r, unwind);
        self.last_call = None;
        b.ins().jump(join, &[r]);

        b.switch_to_block(join);
        b.seal_block(join);
        let r = b.block_params(join)[0];
        self.values.insert(dst, r);
        Ok(())
    }

    pub(super) fn compare(
        &mut self,
        b: &mut FunctionBuilder,
        dst: ValueId,
        x: ValueId,
        y: ValueId,
        cc: FloatCC,
        stub: &str,
    ) -> Result<(), BackendError> {
        if self.is_num(x) && self.is_num(y) {
            let xv = self.get(x)?;
            let yv = self.get(y)?;
            let xf = Self::to_f64(b, xv);
            let yf = Self::to_f64(b, yv);
            let cmp = b.ins().fcmp(cc, xf, yf);
            let r = Self::box_bool(b, cmp);
            self.values.insert(dst, r);
            Ok(())
        } else {
            // Inline when both turn out to be numbers.
            self.any_binop(b, dst, x, y, stub, true, &|b, p, q| {
                let pf = Self::to_f64(b, p);
                let qf = Self::to_f64(b, q);
                let cmp = b.ins().fcmp(cc, pf, qf);
                Self::box_bool(b, cmp)
            })
        }
    }
}
