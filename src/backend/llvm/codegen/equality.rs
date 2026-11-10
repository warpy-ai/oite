//! `===` in generated code.

use super::*;

impl Fx<'_> {
    /// `x === y` on values of unknown type. When either is `null`,
    /// `undefined` or a boolean, the values are equal exactly when their
    /// bits are, so they are compared here; numbers and strings go to the
    /// runtime.
    pub(super) unsafe fn strict_eq(&mut self, x: LLVMValueRef, y: LLVMValueRef) -> LLVMValueRef {
        unsafe {
            let ctx = self.cg.context;
            let b = self.builder;
            let block = |this: &Self, name: &[u8]| {
                LLVMAppendBasicBlockInContext(ctx, this.function, name.as_ptr() as _)
            };
            let same_bb = block(self, b"eq_same\0");
            let slow_bb = block(self, b"eq_slow\0");
            let join_bb = block(self, b"eq_join\0");

            // Not a number and not a pointer: a tag above the pointer's.
            let tag_mask = self.c(0x7FFF_0000_0000_0000);
            let pointer_tag = self.c(QNAN);
            let immediate = |v: LLVMValueRef| {
                let tag = LLVMBuildAnd(b, v, tag_mask, NONAME);
                LLVMBuildICmp(b, LLVMIntPredicate::LLVMIntUGT, tag, pointer_tag, NONAME)
            };
            let either = LLVMBuildOr(b, immediate(x), immediate(y), NONAME);
            LLVMBuildCondBr(b, either, same_bb, slow_bb);

            LLVMPositionBuilderAtEnd(b, same_bb);
            let same = LLVMBuildICmp(b, LLVMIntPredicate::LLVMIntEQ, x, y, NONAME);
            let same = self.box_bool(same);
            LLVMBuildBr(b, join_bb);

            LLVMPositionBuilderAtEnd(b, slow_bb);
            let slow = self.rt("ot_eq_strict", &[x, y]);
            LLVMBuildBr(b, join_bb);

            LLVMPositionBuilderAtEnd(b, join_bb);
            let result = LLVMBuildPhi(b, self.i64_ty(), NONAME);
            let mut values = [same, slow];
            let mut blocks = [same_bb, slow_bb];
            LLVMAddIncoming(result, values.as_mut_ptr(), blocks.as_mut_ptr(), 2);
            // `===` does not throw.
            self.last_call = None;
            result
        }
    }
}
