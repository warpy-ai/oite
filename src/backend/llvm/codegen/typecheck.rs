//! A type annotation checked in generated code (`IrOp::CheckType`).

use super::*;
use crate::runtime::builtins::Kind;

impl Fx<'_> {
    /// `v` if its kind is in the set `kinds`, else a TypeError. A number,
    /// a boolean, `null` and `undefined` are recognised here by their
    /// bits, and a string, an array, a function or an object by the kind
    /// in its header; a class type and a failure go to the runtime's
    /// `check_type`.
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
            let heap = Kind::heap_kinds(kinds);
            let rest_bb = if heap == 0 {
                slow_bb
            } else {
                block(self, b"type_heap\0")
            };
            // The blocks that reach `type_ok` with `v` itself.
            let mut passed = Vec::new();
            match tests.split_first() {
                Some((first, rest)) => {
                    let ok = rest
                        .iter()
                        .fold(*first, |acc, t| LLVMBuildOr(b, acc, *t, NONAME));
                    passed.push(LLVMGetInsertBlock(b));
                    LLVMBuildCondBr(b, ok, join_bb, rest_bb);
                }
                None => {
                    LLVMBuildBr(b, rest_bb);
                }
            }
            if heap != 0 {
                let ptr_bb = block(self, b"type_ptr\0");
                LLVMPositionBuilderAtEnd(b, rest_bb);
                let (is_ptr, ptr) = self.heap_pointer(v);
                LLVMBuildCondBr(b, is_ptr, ptr_bb, slow_bb);

                LLVMPositionBuilderAtEnd(b, ptr_bb);
                let i8t = LLVMInt8TypeInContext(ctx);
                let kind = LLVMBuildLoad2(b, i8t, ptr, NONAME);
                let kind = LLVMBuildZExt(b, kind, self.i64_ty(), NONAME);
                let bit = LLVMBuildShl(b, self.c(1), kind, NONAME);
                let accepted = LLVMBuildAnd(b, bit, self.c(heap), NONAME);
                let accepted =
                    LLVMBuildICmp(b, LLVMIntPredicate::LLVMIntNE, accepted, self.c(0), NONAME);
                passed.push(ptr_bb);
                LLVMBuildCondBr(b, accepted, join_bb, slow_bb);
            }

            LLVMPositionBuilderAtEnd(b, slow_bb);
            let checked = self.rt("ot_check_type", &[v, self.c(kinds as u64)]);
            // Only this path can fail, so only it looks for the exception.
            self.check_exception(checked, self.unwind);
            let checked_bb = LLVMGetInsertBlock(b);
            LLVMBuildBr(b, join_bb);

            LLVMPositionBuilderAtEnd(b, join_bb);
            let phi = LLVMBuildPhi(b, self.i64_ty(), NONAME);
            let mut values: Vec<LLVMValueRef> = passed.iter().map(|_| v).collect();
            values.push(checked);
            passed.push(checked_bb);
            LLVMAddIncoming(
                phi,
                values.as_mut_ptr(),
                passed.as_mut_ptr(),
                values.len() as u32,
            );
            self.last_call = None;
            phi
        }
    }
}
