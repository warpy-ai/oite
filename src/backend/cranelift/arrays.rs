//! Arrays in generated code: element reads and writes with an index in
//! bounds, and `length`, without calling the runtime
//! (docs/docs/object-model.md, section 4).

use super::*;
use crate::runtime::heap::{NativeArray, ObjectKind};
use std::mem::offset_of;

impl Fx<'_> {
    /// Whether `v` is a heap pointer, and its address.
    pub(super) fn heap_pointer(b: &mut FunctionBuilder, v: Value) -> (Value, Value) {
        let tag = b.ins().band_imm(v, 0x7FFF_0000_0000_0000);
        let tagged = b.ins().icmp_imm(IntCC::Equal, tag, QNAN as i64);
        let addr = b.ins().band_imm(v, 0x0000_FFFF_FFFF_FFFF);
        let nonnull = b.ins().icmp_imm(IntCC::NotEqual, addr, 0);
        (b.ins().band(tagged, nonnull), addr)
    }

    /// `o[key]` (or `o[key] = store`): when `o` is an array and `key` an
    /// integer below its length, load or store the element here; otherwise
    /// call the runtime (other receivers and keys, holes past the end,
    /// growth).
    pub(super) fn element(
        &mut self,
        b: &mut FunctionBuilder,
        o: Value,
        key: Value,
        store: Option<Value>,
    ) -> Value {
        let flags = MemFlags::trusted();
        let ptr_blk = b.create_block();
        let arr_blk = b.create_block();
        let hit_blk = b.create_block();
        let miss_blk = b.create_block();
        let join = b.create_block();
        b.set_cold_block(miss_blk);
        b.append_block_param(join, types::I64);

        let (is_ptr, addr) = Self::heap_pointer(b, o);
        b.ins().brif(is_ptr, ptr_blk, &[], miss_blk, &[]);

        b.switch_to_block(ptr_blk);
        b.seal_block(ptr_blk);
        let kind = b.ins().load(types::I8, flags, addr, 0);
        let is_array = b
            .ins()
            .icmp_imm(IntCC::Equal, kind, ObjectKind::Array as i64);
        b.ins().brif(is_array, arr_blk, &[], miss_blk, &[]);

        // The key as an index: a number that is an integer (a value that
        // is not a number reads as NaN and fails the comparison), below
        // the length (a negative one is huge as unsigned).
        b.switch_to_block(arr_blk);
        b.seal_block(arr_blk);
        let f = Self::to_f64(b, key);
        let i = b.ins().fcvt_to_sint_sat(types::I64, f);
        let back = b.ins().fcvt_from_sint(types::F64, i);
        let exact = b.ins().fcmp(FloatCC::Equal, back, f);
        let len = b
            .ins()
            .load(types::I32, flags, addr, offset_of!(NativeArray, len) as i32);
        let len = b.ins().uextend(types::I64, len);
        let in_bounds = b.ins().icmp(IntCC::UnsignedLessThan, i, len);
        let hit = b.ins().band(exact, in_bounds);
        b.ins().brif(hit, hit_blk, &[], miss_blk, &[]);

        b.switch_to_block(hit_blk);
        b.seal_block(hit_blk);
        let elements = b.ins().load(
            types::I64,
            flags,
            addr,
            offset_of!(NativeArray, elements) as i32,
        );
        let bytes = b.ins().ishl_imm(i, 3);
        let slot = b.ins().iadd(elements, bytes);
        let hit_value = match store {
            Some(v) => {
                b.ins().store(flags, v, slot, 0);
                Self::c(b, UNDEFINED)
            }
            None => b.ins().load(types::I64, flags, slot, 0),
        };
        b.ins().jump(join, &[hit_value]);

        b.switch_to_block(miss_blk);
        b.seal_block(miss_blk);
        let miss = match store {
            Some(v) => self.rt_v(b, "ot_set_index", &[o, key, v]),
            None => self.rt_v(b, "ot_get_index", &[o, key]),
        };
        let unwind = self.unwind;
        self.check_exception(b, miss, unwind);
        self.last_call = None;
        b.ins().jump(join, &[miss]);

        b.switch_to_block(join);
        b.seal_block(join);
        b.block_params(join)[0]
    }

    /// `o.length`: the length field of an array or a string, read here;
    /// for anything else, the property `length` through its cache.
    pub(super) fn length(&mut self, b: &mut FunctionBuilder, o: Value) -> Value {
        let flags = MemFlags::trusted();
        let ptr_blk = b.create_block();
        let hit_blk = b.create_block();
        let miss_blk = b.create_block();
        let join = b.create_block();
        b.append_block_param(join, types::I64);

        let (is_ptr, addr) = Self::heap_pointer(b, o);
        b.ins().brif(is_ptr, ptr_blk, &[], miss_blk, &[]);

        // Arrays and strings both keep their length at the same offset.
        b.switch_to_block(ptr_blk);
        b.seal_block(ptr_blk);
        let kind = b.ins().load(types::I8, flags, addr, 0);
        let is_array = b
            .ins()
            .icmp_imm(IntCC::Equal, kind, ObjectKind::Array as i64);
        let is_string = b
            .ins()
            .icmp_imm(IntCC::Equal, kind, ObjectKind::String as i64);
        let has_len = b.ins().bor(is_array, is_string);
        b.ins().brif(has_len, hit_blk, &[], miss_blk, &[]);

        b.switch_to_block(hit_blk);
        b.seal_block(hit_blk);
        let len = b
            .ins()
            .load(types::I32, flags, addr, offset_of!(NativeArray, len) as i32);
        let len = b.ins().fcvt_from_uint(types::F64, len);
        let hit_value = Self::from_f64(b, len);
        b.ins().jump(join, &[hit_value]);

        b.switch_to_block(miss_blk);
        b.seal_block(miss_blk);
        let miss = self.cached_prop(b, o, "length", None);
        b.ins().jump(join, &[miss]);

        b.switch_to_block(join);
        b.seal_block(join);
        b.block_params(join)[0]
    }
}
