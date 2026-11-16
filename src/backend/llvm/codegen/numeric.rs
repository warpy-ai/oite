//! Arithmetic, comparison, `Math` and intrinsics in generated code:
//! inline on numbers, through the runtime on anything else.

use super::*;

impl Fx<'_> {
    pub(super) unsafe fn num_binop(
        &mut self,
        dst: ValueId,
        a: ValueId,
        b: ValueId,
        build: unsafe extern "C" fn(
            LLVMBuilderRef,
            LLVMValueRef,
            LLVMValueRef,
            *const c_char,
        ) -> LLVMValueRef,
    ) -> Result<(), BackendError> {
        unsafe {
            let x = self.to_f64(self.get(a)?);
            let y = self.to_f64(self.get(b)?);
            let r = build(self.builder, x, y, NONAME);
            let r = self.from_f64(r);
            self.set(dst, r);
            Ok(())
        }
    }

    pub(super) unsafe fn rt_binop(
        &mut self,
        dst: ValueId,
        name: &str,
        a: ValueId,
        b: ValueId,
    ) -> Result<(), BackendError> {
        unsafe {
            let args = [self.get(a)?, self.get(b)?];
            let r = self.rt(name, &args);
            self.set(dst, r);
            Ok(())
        }
    }

    /// Call the LLVM intrinsic `name`, overloaded on `overload`.
    pub(super) unsafe fn call_intrinsic(
        &mut self,
        name: &[u8],
        overload: &[LLVMTypeRef],
        ret: LLVMTypeRef,
        args: &mut [LLVMValueRef],
    ) -> LLVMValueRef {
        unsafe {
            let id = LLVMLookupIntrinsicID(name.as_ptr() as _, name.len());
            let mut tys = overload.to_vec();
            let f = LLVMGetIntrinsicDeclaration(self.cg.module, id, tys.as_mut_ptr(), tys.len());
            let mut params: Vec<LLVMTypeRef> = args.iter().map(|a| LLVMTypeOf(*a)).collect();
            let fty = LLVMFunctionType(ret, params.as_mut_ptr(), params.len() as u32, 0);
            LLVMBuildCall2(
                self.builder,
                fty,
                f,
                args.as_mut_ptr(),
                args.len() as u32,
                NONAME,
            )
        }
    }

    /// JavaScript `x % y` on doubles. When both are integers below 2^53 and
    /// `y` is not 0 it is an integer remainder with the sign of `x`
    /// (`copysign` also gives `-0`); otherwise `fmod` (`frem`).
    pub(super) unsafe fn js_mod(&mut self, x: LLVMValueRef, y: LLVMValueRef) -> LLVMValueRef {
        unsafe {
            let ctx = self.cg.context;
            let i64t = self.i64_ty();
            let f64t = self.f64_ty();
            let limit = 1i64 << 53;
            let exact = |this: &mut Self, v: LLVMValueRef| -> (LLVMValueRef, LLVMValueRef) {
                let vi = this.call_intrinsic(b"llvm.fptosi.sat", &[i64t, f64t], i64t, &mut [v]);
                let back = LLVMBuildSIToFP(this.builder, vi, f64t, NONAME);
                let same = LLVMBuildFCmp(
                    this.builder,
                    LLVMRealPredicate::LLVMRealOEQ,
                    back,
                    v,
                    NONAME,
                );
                let lo = LLVMBuildICmp(
                    this.builder,
                    LLVMIntPredicate::LLVMIntSGT,
                    vi,
                    LLVMConstInt(i64t, (-limit) as u64, 1),
                    NONAME,
                );
                let hi = LLVMBuildICmp(
                    this.builder,
                    LLVMIntPredicate::LLVMIntSLT,
                    vi,
                    LLVMConstInt(i64t, limit as u64, 0),
                    NONAME,
                );
                let ok = LLVMBuildAnd(this.builder, same, lo, NONAME);
                (vi, LLVMBuildAnd(this.builder, ok, hi, NONAME))
            };
            let (xi, xok) = exact(self, x);
            let (yi, yok) = exact(self, y);
            let nonzero = LLVMBuildICmp(
                self.builder,
                LLVMIntPredicate::LLVMIntNE,
                yi,
                LLVMConstInt(i64t, 0, 0),
                NONAME,
            );
            let ints = LLVMBuildAnd(self.builder, xok, yok, NONAME);
            let ints = LLVMBuildAnd(self.builder, ints, nonzero, NONAME);
            let int_bb =
                LLVMAppendBasicBlockInContext(ctx, self.function, b"mod_int\0".as_ptr() as _);
            let flt_bb =
                LLVMAppendBasicBlockInContext(ctx, self.function, b"mod_float\0".as_ptr() as _);
            let join =
                LLVMAppendBasicBlockInContext(ctx, self.function, b"mod_join\0".as_ptr() as _);
            LLVMBuildCondBr(self.builder, ints, int_bb, flt_bb);

            LLVMPositionBuilderAtEnd(self.builder, int_bb);
            let r = LLVMBuildSRem(self.builder, xi, yi, NONAME);
            let rf = LLVMBuildSIToFP(self.builder, r, f64t, NONAME);
            let int_value = self.call_intrinsic(b"llvm.copysign", &[f64t], f64t, &mut [rf, x]);
            LLVMBuildBr(self.builder, join);

            LLVMPositionBuilderAtEnd(self.builder, flt_bb);
            let flt_value = LLVMBuildFRem(self.builder, x, y, NONAME);
            LLVMBuildBr(self.builder, join);

            LLVMPositionBuilderAtEnd(self.builder, join);
            let phi = LLVMBuildPhi(self.builder, f64t, NONAME);
            let mut values = [int_value, flt_value];
            let mut blocks = [int_bb, flt_bb];
            LLVMAddIncoming(phi, values.as_mut_ptr(), blocks.as_mut_ptr(), 2);
            phi
        }
    }

    /// A number's `ToInt32` as an i32, and whether the fast conversion
    /// holds: the number is below 2^63 in magnitude (so its integer part
    /// fits an i64; NaN and the infinities do not pass).
    pub(super) unsafe fn int32_of(&mut self, f: LLVMValueRef) -> (LLVMValueRef, LLVMValueRef) {
        unsafe {
            let i64t = self.i64_ty();
            let f64t = self.f64_ty();
            let limit = 9_223_372_036_854_775_808.0_f64;
            let i = self.call_intrinsic(b"llvm.fptosi.sat", &[i64t, f64t], i64t, &mut [f]);
            let lo = LLVMBuildFCmp(
                self.builder,
                LLVMRealPredicate::LLVMRealOGT,
                f,
                LLVMConstReal(f64t, -limit),
                NONAME,
            );
            let hi = LLVMBuildFCmp(
                self.builder,
                LLVMRealPredicate::LLVMRealOLT,
                f,
                LLVMConstReal(f64t, limit),
                NONAME,
            );
            let ok = LLVMBuildAnd(self.builder, lo, hi, NONAME);
            let i32t = LLVMInt32TypeInContext(self.cg.context);
            (LLVMBuildTrunc(self.builder, i, i32t, NONAME), ok)
        }
    }

    /// A bitwise operator: inline on 32-bit integers when both operands are
    /// numbers below 2^63, else the runtime (`valueOf` may throw).
    pub(super) unsafe fn int_binop(
        &mut self,
        dst: ValueId,
        a: ValueId,
        b: ValueId,
        op: IntOp,
        stub: &str,
    ) -> Result<(), BackendError> {
        unsafe {
            let (x, y) = (self.get(a)?, self.get(b)?);
            let (xi, xok) = self.int32_of(self.to_f64(x));
            let (yi, yok) = self.int32_of(self.to_f64(y));
            let mut fast_ok = LLVMBuildAnd(self.builder, xok, yok, NONAME);
            if !(self.is_num(a) && self.is_num(b)) {
                let both = LLVMBuildAnd(
                    self.builder,
                    self.is_number_bits(x),
                    self.is_number_bits(y),
                    NONAME,
                );
                fast_ok = LLVMBuildAnd(self.builder, fast_ok, both, NONAME);
            }
            let ctx = self.cg.context;
            let fast = LLVMAppendBasicBlockInContext(ctx, self.function, b"int\0".as_ptr() as _);
            let slow =
                LLVMAppendBasicBlockInContext(ctx, self.function, b"int_any\0".as_ptr() as _);
            let join =
                LLVMAppendBasicBlockInContext(ctx, self.function, b"int_join\0".as_ptr() as _);
            LLVMBuildCondBr(self.builder, fast_ok, fast, slow);

            LLVMPositionBuilderAtEnd(self.builder, fast);
            let i32t = LLVMInt32TypeInContext(ctx);
            let bld = self.builder;
            let shift = || LLVMBuildAnd(bld, yi, LLVMConstInt(i32t, 31, 0), NONAME);
            let r = match op {
                IntOp::And => LLVMBuildAnd(bld, xi, yi, NONAME),
                IntOp::Or => LLVMBuildOr(bld, xi, yi, NONAME),
                IntOp::Xor => LLVMBuildXor(bld, xi, yi, NONAME),
                IntOp::Shl => LLVMBuildShl(bld, xi, shift(), NONAME),
                IntOp::Shr => LLVMBuildAShr(bld, xi, shift(), NONAME),
                IntOp::ShrU => LLVMBuildLShr(bld, xi, shift(), NONAME),
            };
            let rf = if matches!(op, IntOp::ShrU) {
                LLVMBuildUIToFP(bld, r, self.f64_ty(), NONAME)
            } else {
                LLVMBuildSIToFP(bld, r, self.f64_ty(), NONAME)
            };
            let fast_value = self.from_f64(rf);
            let fast_end = LLVMGetInsertBlock(self.builder);
            LLVMBuildBr(self.builder, join);

            LLVMPositionBuilderAtEnd(self.builder, slow);
            let slow_value = self.rt(stub, &[x, y]);
            self.check_exception(slow_value, self.unwind);
            self.last_call = None;
            let slow_end = LLVMGetInsertBlock(self.builder);
            LLVMBuildBr(self.builder, join);

            LLVMPositionBuilderAtEnd(self.builder, join);
            let phi = LLVMBuildPhi(self.builder, self.i64_ty(), NONAME);
            let mut values = [fast_value, slow_value];
            let mut blocks = [fast_end, slow_end];
            LLVMAddIncoming(phi, values.as_mut_ptr(), blocks.as_mut_ptr(), 2);
            self.set(dst, phi);
            Ok(())
        }
    }

    /// `Math.<f>` computed inline on doubles, for the functions with a
    /// machine instruction: `None` for the others, or a wrong argument count.
    pub(super) unsafe fn math_inline(
        &mut self,
        name: &str,
        xs: &[LLVMValueRef],
    ) -> Option<LLVMValueRef> {
        unsafe {
            let f64t = self.f64_ty();
            let unary = |this: &mut Self, intrinsic: &[u8]| {
                this.call_intrinsic(intrinsic, &[f64t], f64t, &mut [xs[0]])
            };
            Some(match (name, xs.len()) {
                ("sqrt", 1) => unary(self, b"llvm.sqrt"),
                ("floor", 1) => unary(self, b"llvm.floor"),
                ("ceil", 1) => unary(self, b"llvm.ceil"),
                ("trunc", 1) => unary(self, b"llvm.trunc"),
                ("abs", 1) => unary(self, b"llvm.fabs"),
                // As the runtime: halves round up, towards +Infinity.
                ("round", 1) => {
                    let f = unary(self, b"llvm.floor");
                    let diff = LLVMBuildFSub(self.builder, xs[0], f, NONAME);
                    let up = LLVMBuildFCmp(
                        self.builder,
                        LLVMRealPredicate::LLVMRealOGE,
                        diff,
                        LLVMConstReal(f64t, 0.5),
                        NONAME,
                    );
                    let f1 = LLVMBuildFAdd(self.builder, f, LLVMConstReal(f64t, 1.0), NONAME);
                    LLVMBuildSelect(self.builder, up, f1, f, NONAME)
                }
                // NaN wins and -0 is below +0, as in JavaScript.
                ("min", 2) => {
                    self.call_intrinsic(b"llvm.minimum", &[f64t], f64t, &mut [xs[0], xs[1]])
                }
                ("max", 2) => {
                    self.call_intrinsic(b"llvm.maximum", &[f64t], f64t, &mut [xs[0], xs[1]])
                }
                _ => return None,
            })
        }
    }

    /// `Math.<f>(args)` (`IrOp::MathCall`): inline when every argument is a
    /// number and `f` has an instruction, else `ot_math_call`.
    pub(super) unsafe fn math_call(
        &mut self,
        func: u32,
        args: &[ValueId],
    ) -> Result<LLVMValueRef, BackendError> {
        unsafe {
            let name = crate::runtime::builtins::MATH_FUNCTIONS
                .get(func as usize)
                .copied()
                .unwrap_or("");
            let vals: Vec<LLVMValueRef> = args
                .iter()
                .map(|a| self.get(*a))
                .collect::<Result<_, _>>()?;
            let call_runtime = |this: &mut Self| {
                let (argv, argc) = this.arg_array(&vals);
                let r = this.rt("ot_math_call", &[this.c(func as u64), argv, argc]);
                this.check_exception(r, this.unwind);
                this.last_call = None;
                r
            };
            let can_inline = matches!(
                (name, vals.len()),
                ("sqrt" | "floor" | "ceil" | "trunc" | "abs" | "round", 1) | ("min" | "max", 2)
            );
            if !can_inline {
                return Ok(call_runtime(self));
            }
            let all_numbers = args.iter().all(|a| self.is_num(*a));
            let floats: Vec<LLVMValueRef> = vals.iter().map(|v| self.to_f64(*v)).collect();
            if all_numbers {
                let r = self
                    .math_inline(name, &floats)
                    .expect("inline Math function");
                return Ok(self.from_f64(r));
            }
            let mut ok = self.is_number_bits(vals[0]);
            for v in &vals[1..] {
                let n = self.is_number_bits(*v);
                ok = LLVMBuildAnd(self.builder, ok, n, NONAME);
            }
            let ctx = self.cg.context;
            let fast = LLVMAppendBasicBlockInContext(ctx, self.function, b"math\0".as_ptr() as _);
            let slow =
                LLVMAppendBasicBlockInContext(ctx, self.function, b"math_any\0".as_ptr() as _);
            let join =
                LLVMAppendBasicBlockInContext(ctx, self.function, b"math_join\0".as_ptr() as _);
            LLVMBuildCondBr(self.builder, ok, fast, slow);
            LLVMPositionBuilderAtEnd(self.builder, fast);
            let r = self
                .math_inline(name, &floats)
                .expect("inline Math function");
            let fast_value = self.from_f64(r);
            let fast_end = LLVMGetInsertBlock(self.builder);
            LLVMBuildBr(self.builder, join);
            LLVMPositionBuilderAtEnd(self.builder, slow);
            let slow_value = call_runtime(self);
            let slow_end = LLVMGetInsertBlock(self.builder);
            LLVMBuildBr(self.builder, join);
            LLVMPositionBuilderAtEnd(self.builder, join);
            let phi = LLVMBuildPhi(self.builder, self.i64_ty(), NONAME);
            let mut values = [fast_value, slow_value];
            let mut blocks = [fast_end, slow_end];
            LLVMAddIncoming(phi, values.as_mut_ptr(), blocks.as_mut_ptr(), 2);
            Ok(phi)
        }
    }

    /// Whether a value (i64) is a number: not one of the boxed tags.
    pub(super) unsafe fn is_number_bits(&self, v: LLVMValueRef) -> LLVMValueRef {
        unsafe {
            const QNAN: u64 = 0x7FFC_0000_0000_0000;
            let masked = LLVMBuildAnd(self.builder, v, self.c(QNAN), NONAME);
            LLVMBuildICmp(
                self.builder,
                LLVMIntPredicate::LLVMIntNE,
                masked,
                self.c(QNAN),
                NONAME,
            )
        }
    }

    /// `a op b` on values of unknown type: inline floating point when both
    /// turn out to be numbers, else the runtime (which may throw).
    pub(super) unsafe fn any_arith(
        &mut self,
        dst: ValueId,
        a: ValueId,
        b: ValueId,
        build: unsafe extern "C" fn(
            LLVMBuilderRef,
            LLVMValueRef,
            LLVMValueRef,
            *const c_char,
        ) -> LLVMValueRef,
        stub: &str,
    ) -> Result<(), BackendError> {
        unsafe {
            if self.is_num(a) && self.is_num(b) && stub != "ot_mod_any" {
                return self.num_binop(dst, a, b, build);
            }
            let (x, y) = (self.get(a)?, self.get(b)?);
            let ctx = self.cg.context;
            let fast = LLVMAppendBasicBlockInContext(ctx, self.function, b"num\0".as_ptr() as _);
            let slow = LLVMAppendBasicBlockInContext(ctx, self.function, b"any\0".as_ptr() as _);
            let join = LLVMAppendBasicBlockInContext(ctx, self.function, b"join\0".as_ptr() as _);
            let both = LLVMBuildAnd(
                self.builder,
                self.is_number_bits(x),
                self.is_number_bits(y),
                NONAME,
            );
            LLVMBuildCondBr(self.builder, both, fast, slow);

            LLVMPositionBuilderAtEnd(self.builder, fast);
            let r = if stub == "ot_mod_any" {
                self.js_mod(self.to_f64(x), self.to_f64(y))
            } else {
                build(self.builder, self.to_f64(x), self.to_f64(y), NONAME)
            };
            let fast_value = self.from_f64(r);
            let fast_end = LLVMGetInsertBlock(self.builder);
            LLVMBuildBr(self.builder, join);

            LLVMPositionBuilderAtEnd(self.builder, slow);
            let slow_value = self.rt(stub, &[x, y]);
            self.check_exception(slow_value, self.unwind);
            self.last_call = None;
            let slow_end = LLVMGetInsertBlock(self.builder);
            LLVMBuildBr(self.builder, join);

            LLVMPositionBuilderAtEnd(self.builder, join);
            let phi = LLVMBuildPhi(self.builder, self.i64_ty(), NONAME);
            let mut values = [fast_value, slow_value];
            let mut blocks = [fast_end, slow_end];
            LLVMAddIncoming(phi, values.as_mut_ptr(), blocks.as_mut_ptr(), 2);
            self.set(dst, phi);
            Ok(())
        }
    }

    /// A comparison: inline when both operands are numbers, else the runtime.
    pub(super) unsafe fn compare(
        &mut self,
        dst: ValueId,
        a: ValueId,
        b: ValueId,
        pred: LLVMRealPredicate,
        stub: &str,
    ) -> Result<(), BackendError> {
        unsafe {
            if self.is_num(a) && self.is_num(b) {
                let x = self.to_f64(self.get(a)?);
                let y = self.to_f64(self.get(b)?);
                let cmp = LLVMBuildFCmp(self.builder, pred, x, y, NONAME);
                let r = self.box_bool(cmp);
                self.set(dst, r);
                Ok(())
            } else {
                // Inline when both turn out to be numbers.
                let (x, y) = (self.get(a)?, self.get(b)?);
                let ctx = self.cg.context;
                let fast =
                    LLVMAppendBasicBlockInContext(ctx, self.function, b"num\0".as_ptr() as _);
                let slow =
                    LLVMAppendBasicBlockInContext(ctx, self.function, b"any\0".as_ptr() as _);
                let join =
                    LLVMAppendBasicBlockInContext(ctx, self.function, b"join\0".as_ptr() as _);
                let both = LLVMBuildAnd(
                    self.builder,
                    self.is_number_bits(x),
                    self.is_number_bits(y),
                    NONAME,
                );
                LLVMBuildCondBr(self.builder, both, fast, slow);
                LLVMPositionBuilderAtEnd(self.builder, fast);
                let cmp = LLVMBuildFCmp(self.builder, pred, self.to_f64(x), self.to_f64(y), NONAME);
                let fast_value = self.box_bool(cmp);
                let fast_end = LLVMGetInsertBlock(self.builder);
                LLVMBuildBr(self.builder, join);
                LLVMPositionBuilderAtEnd(self.builder, slow);
                let slow_value = self.rt(stub, &[x, y]);
                // `valueOf` may throw.
                self.check_exception(slow_value, self.unwind);
                self.last_call = None;
                let slow_end = LLVMGetInsertBlock(self.builder);
                LLVMBuildBr(self.builder, join);
                LLVMPositionBuilderAtEnd(self.builder, join);
                let phi = LLVMBuildPhi(self.builder, self.i64_ty(), NONAME);
                let mut values = [fast_value, slow_value];
                let mut blocks = [fast_end, slow_end];
                LLVMAddIncoming(phi, values.as_mut_ptr(), blocks.as_mut_ptr(), 2);
                self.set(dst, phi);
                Ok(())
            }
        }
    }

    /// Flip a boxed boolean.
    pub(super) unsafe fn negate(&mut self, v: LLVMValueRef) -> LLVMValueRef {
        unsafe { LLVMBuildXor(self.builder, v, self.c(1), NONAME) }
    }
}
