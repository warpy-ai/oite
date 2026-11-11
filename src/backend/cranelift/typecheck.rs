//! A type annotation checked in generated code (`IrOp::CheckType`).

use super::*;
use crate::runtime::builtins::Kind;

impl Fx<'_> {
    /// `v` if its kind is in the set `kinds`, else a TypeError. A number,
    /// a boolean, `null` and `undefined` are recognised here by their
    /// bits; anything else (a kind on the heap, or a failure) goes to the
    /// runtime's `check_type`.
    pub(super) fn check_type(&mut self, b: &mut FunctionBuilder, v: Value, kinds: u32) -> Value {
        let has = |kind: Kind| kinds & kind.bit() != 0;
        let mut ok: Option<Value> = None;
        let mut accept = |b: &mut FunctionBuilder, test: Value| {
            ok = Some(match ok {
                Some(prev) => b.ins().bor(prev, test),
                None => test,
            });
        };
        if has(Kind::Number) {
            let tag = b.ins().band_imm(v, QNAN as i64);
            let test = b.ins().icmp_imm(IntCC::NotEqual, tag, QNAN as i64);
            accept(b, test);
        }
        if has(Kind::Boolean) {
            // `true` is `false` with the lowest bit set.
            let as_true = b.ins().bor_imm(v, 1);
            let test = b.ins().icmp_imm(IntCC::Equal, as_true, TRUE as i64);
            accept(b, test);
        }
        if has(Kind::Null) {
            let test = b.ins().icmp_imm(IntCC::Equal, v, (QNAN | TAG_NULL) as i64);
            accept(b, test);
        }
        if has(Kind::Undefined) {
            let test = b.ins().icmp_imm(IntCC::Equal, v, UNDEFINED as i64);
            accept(b, test);
        }

        let slow_blk = b.create_block();
        let join = b.create_block();
        b.append_block_param(join, types::I64);
        match ok {
            Some(ok) => {
                b.ins().brif(ok, join, &[v], slow_blk, &[]);
            }
            None => {
                b.ins().jump(slow_blk, &[]);
            }
        }

        b.switch_to_block(slow_blk);
        b.seal_block(slow_blk);
        b.set_cold_block(slow_blk);
        let id = crate::runtime::intrinsics::id("check_type").expect("the check_type intrinsic");
        let kinds_value = Self::c(b, (kinds as f64).to_bits());
        let (argv, argc) = self.arg_array(b, &[v, kinds_value]);
        let id = Self::c(b, id as u64);
        let checked = self.rt_v(b, "ot_intrinsic", &[id, argv, argc]);
        b.ins().jump(join, &[checked]);

        b.switch_to_block(join);
        b.seal_block(join);
        let result = b.block_params(join)[0];
        // The exception, if the check failed.
        self.last_call = Some(result);
        result
    }
}
