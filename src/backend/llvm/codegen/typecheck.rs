//! A type annotation checked in generated code (`IrOp::CheckType`).

use super::*;
use crate::runtime::builtins::Kind;

impl Fx<'_> {
    /// `v` if its kind is in the set `kinds`, else a TypeError. A number,
    /// a boolean, `null` and `undefined` are recognised here by their
    /// bits; anything else (a kind on the heap, or a failure) goes to the
    /// runtime's `check_type`.
    pub(super) unsafe fn check_type(&mut self, v: LLVMValueRef, kinds: u32) -> LLVMValueRef {
        unsafe {
            let b = self.builder;
            let has = |kind: Kind| kinds & kind.bit() != 0;
            let eq = |x: LLVMValueRef, bits: u64| {
                LLVMBuildICmp(b, LLVMIntPredicate::LLVMIntEQ, x, self.c(bits), NONAME)
            };
            let mut tests = Vec::new();
            if has(Kind::Number) {
                tests.push(self.is_number_bits(v));
            }
            if has(Kind::Boolean) {
                // `true` is `false` with the lowest bit set.
                tests.push(eq(LLVMBuildOr(b, v, self.c(1), NONAME), TRUE));
            }
            if has(Kind::Null) {
                tests.push(eq(v, QNAN | TAG_NULL));
            }
            if has(Kind::Undefined) {
                tests.push(eq(v, UNDEFINED));
            }

            let ctx = self.cg.context;
            let block = |this: &Self, name: &[u8]| {
                LLVMAppendBasicBlockInContext(ctx, this.function, name.as_ptr() as _)
            };
            let slow_bb = block(self, b"type_slow\0");
            let join_bb = block(self, b"type_ok\0");
            let fast_bb = LLVMGetInsertBlock(b);
            match tests.split_first() {
                Some((first, rest)) => {
                    let ok = rest
                        .iter()
                        .fold(*first, |acc, t| LLVMBuildOr(b, acc, *t, NONAME));
                    LLVMBuildCondBr(b, ok, join_bb, slow_bb);
                }
                None => {
                    LLVMBuildBr(b, slow_bb);
                }
            }

            LLVMPositionBuilderAtEnd(b, slow_bb);
            let id =
                crate::runtime::intrinsics::id("check_type").expect("the check_type intrinsic");
            let kinds_value = self.c((kinds as f64).to_bits());
            let (argv, argc) = self.arg_array(&[v, kinds_value]);
            let checked = self.rt("ot_intrinsic", &[self.c(id as u64), argv, argc]);
            LLVMBuildBr(b, join_bb);

            LLVMPositionBuilderAtEnd(b, join_bb);
            let result = if tests.is_empty() {
                checked
            } else {
                let phi = LLVMBuildPhi(b, self.i64_ty(), NONAME);
                let mut values = [v, checked];
                let mut blocks = [fast_bb, slow_bb];
                LLVMAddIncoming(phi, values.as_mut_ptr(), blocks.as_mut_ptr(), 2);
                phi
            };
            // The exception, if the check failed.
            self.last_call = Some(result);
            result
        }
    }
}
