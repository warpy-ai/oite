//! `===` in generated code.

use super::*;

impl Fx<'_> {
    /// `x === y` on values of unknown type. When either is `null`,
    /// `undefined` or a boolean, the values are equal exactly when their
    /// bits are, so they are compared here; numbers and strings go to the
    /// runtime.
    pub(super) fn strict_eq(&mut self, b: &mut FunctionBuilder, x: Value, y: Value) -> Value {
        let same_blk = b.create_block();
        let slow_blk = b.create_block();
        let join = b.create_block();
        b.append_block_param(join, types::I64);

        // Not a number and not a pointer: a tag above the pointer's.
        let immediate = |b: &mut FunctionBuilder, v: Value| {
            let tag = b.ins().band_imm(v, 0x7FFF_0000_0000_0000);
            b.ins()
                .icmp_imm(IntCC::UnsignedGreaterThan, tag, QNAN as i64)
        };
        let x_immediate = immediate(b, x);
        let y_immediate = immediate(b, y);
        let either = b.ins().bor(x_immediate, y_immediate);
        b.ins().brif(either, same_blk, &[], slow_blk, &[]);

        b.switch_to_block(same_blk);
        b.seal_block(same_blk);
        let same = b.ins().icmp(IntCC::Equal, x, y);
        let same = Self::box_bool(b, same);
        b.ins().jump(join, &[same]);

        b.switch_to_block(slow_blk);
        b.seal_block(slow_blk);
        let slow = self.rt_v(b, "ot_eq_strict", &[x, y]);
        b.ins().jump(join, &[slow]);

        b.switch_to_block(join);
        b.seal_block(join);
        // `===` does not throw.
        self.last_call = None;
        b.block_params(join)[0]
    }
}
