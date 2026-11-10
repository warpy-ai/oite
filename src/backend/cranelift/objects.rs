//! Objects in generated code: property reads and writes through inline
//! caches, object literals allocated in their final shape, and the cache
//! of a `new` site (docs/docs/object-model.md, sections 3 and 5).

use super::*;

impl Fx<'_> {
    /// A new inline cache (`runtime::object::ic::PropCache`) for a site
    /// that accesses the property `name`; it lives as long as the code.
    pub(super) fn prop_cache(b: &mut FunctionBuilder, name: &str) -> Value {
        use crate::runtime::object::ic::PropCache;
        let (p, n) = leak_str(name);
        let cache = Box::leak(Box::new(PropCache::new(p as *const u8, n as usize)));
        b.ins().iconst(types::I64, cache as *mut PropCache as i64)
    }

    /// A new cache (`runtime::object::ic::LiteralCache`) for a literal site
    /// that stores the properties `names` on an object with `slots` slots.
    pub(super) fn literal_cache(b: &mut FunctionBuilder, names: &[String], slots: usize) -> Value {
        use crate::runtime::object::ic::LiteralCache;
        let pairs: Vec<(*const u8, usize)> = names
            .iter()
            .map(|name| {
                let (p, n) = leak_str(name);
                (p as *const u8, n as usize)
            })
            .collect();
        let pairs = Box::leak(pairs.into_boxed_slice());
        let cache = Box::leak(Box::new(LiteralCache {
            shape: 0,
            slots: slots.min(crate::runtime::heap::MAX_INLINE_SLOTS) as u32,
            count: names.len() as u32,
            names: pairs.as_ptr(),
        }));
        b.ins()
            .iconst(types::I64, cache as *mut LiteralCache as i64)
    }

    /// The cached addition of a property to the object at `addr`, whose
    /// shape is `shape`: when the shape and the prototype are the cached
    /// ones, no prototype has changed since, the object is not a prototype
    /// itself and the new slot is in the object, store `v` there and move
    /// the object to the next shape. Jumps to `join` then, and to `miss`
    /// otherwise.
    #[allow(clippy::too_many_arguments)]
    fn cached_add(
        &mut self,
        b: &mut FunctionBuilder,
        addr: Value,
        shape: Value,
        cache: Value,
        v: Value,
        join: Block,
        miss: Block,
    ) {
        use crate::runtime::heap::{NativeObject, ObjectHeader};
        use crate::runtime::object::ic;
        use std::mem::offset_of;
        let flags = MemFlags::trusted();
        let done = b.create_block();
        let cell = |b: &mut FunctionBuilder, ty, offset: usize| {
            b.ins().load(ty, flags, cache, offset as i32)
        };

        let before = cell(b, types::I64, ic::ADD_SHAPE_OFFSET);
        let mut ok = b.ins().icmp(IntCC::Equal, shape, before);

        let proto = b.ins().load(
            types::I64,
            flags,
            addr,
            offset_of!(NativeObject, proto) as i32,
        );
        let cached_proto = cell(b, types::I64, ic::ADD_PROTO_OFFSET);
        let same_proto = b.ins().icmp(IntCC::Equal, proto, cached_proto);
        ok = b.ins().band(ok, same_proto);

        let epoch_addr = b.ins().iconst(types::I64, &ic::OT_EPOCH as *const _ as i64);
        let epoch = b.ins().load(types::I32, flags, epoch_addr, 0);
        let cached_epoch = cell(b, types::I32, ic::ADD_EPOCH_OFFSET);
        let same_epoch = b.ins().icmp(IntCC::Equal, epoch, cached_epoch);
        ok = b.ins().band(ok, same_epoch);

        let object_flags = b.ins().load(
            types::I8,
            flags,
            addr,
            offset_of!(ObjectHeader, flags) as i32,
        );
        let prototype = b
            .ins()
            .band_imm(object_flags, ObjectHeader::PROTOTYPE as i64);
        let plain = b.ins().icmp_imm(IntCC::Equal, prototype, 0);
        ok = b.ins().band(ok, plain);

        let slot = cell(b, types::I32, ic::ADD_SLOT_OFFSET);
        let inline = b.ins().icmp_imm(IntCC::SignedGreaterThanOrEqual, slot, 0);
        ok = b.ins().band(ok, inline);
        b.ins().brif(ok, done, &[], miss, &[]);

        b.switch_to_block(done);
        b.seal_block(done);
        let slot = b.ins().uextend(types::I64, slot);
        let place = b.ins().iadd(addr, slot);
        b.ins().store(flags, v, place, 0);
        let next = cell(b, types::I64, ic::ADD_NEXT_OFFSET);
        b.ins()
            .store(flags, next, addr, offset_of!(NativeObject, shape) as i32);
        let undefined = Self::c(b, UNDEFINED);
        b.ins().jump(join, &[undefined]);
    }

    /// `o.name` (or `o.name = store`) through an inline cache: when `o` is
    /// an object whose shape is the cached one, load or store its slot
    /// here; otherwise call the runtime, which fills the cache.
    pub(super) fn cached_prop(
        &mut self,
        b: &mut FunctionBuilder,
        o: Value,
        name: &str,
        store: Option<Value>,
    ) -> Value {
        use crate::runtime::heap::NativeObject;
        use crate::runtime::object::ic;
        use std::mem::offset_of;
        let cache = Self::prop_cache(b, name);
        let is_ptr_blk = b.create_block();
        let is_obj_blk = b.create_block();
        let hit_blk = b.create_block();
        let miss_blk = b.create_block();
        let join = b.create_block();
        b.set_cold_block(miss_blk);
        b.append_block_param(join, types::I64);
        let flags = MemFlags::trusted();

        // A heap pointer: the pointer tag, and a payload that is not 0.
        let tag = b.ins().band_imm(o, 0x7FFF_0000_0000_0000);
        let tagged = b.ins().icmp_imm(IntCC::Equal, tag, QNAN as i64);
        let addr = b.ins().band_imm(o, 0x0000_FFFF_FFFF_FFFF);
        let nonnull = b.ins().icmp_imm(IntCC::NotEqual, addr, 0);
        let is_ptr = b.ins().band(tagged, nonnull);
        b.ins().brif(is_ptr, is_ptr_blk, &[], miss_blk, &[]);

        // An object (kind 0 in the header's first byte).
        b.switch_to_block(is_ptr_blk);
        b.seal_block(is_ptr_blk);
        let kind = b.ins().load(types::I8, flags, addr, 0);
        let is_obj = b.ins().icmp_imm(IntCC::Equal, kind, 0);
        b.ins().brif(is_obj, is_obj_blk, &[], miss_blk, &[]);

        // With the cached shape. A write to an object of another shape
        // may still be the cached addition of the property.
        b.switch_to_block(is_obj_blk);
        b.seal_block(is_obj_blk);
        let shape = b.ins().load(
            types::I64,
            flags,
            addr,
            offset_of!(NativeObject, shape) as i32,
        );
        let cached = b
            .ins()
            .load(types::I64, flags, cache, ic::SHAPE_OFFSET as i32);
        let same = b.ins().icmp(IntCC::Equal, shape, cached);
        match store {
            Some(v) => {
                let add_blk = b.create_block();
                b.ins().brif(same, hit_blk, &[], add_blk, &[]);
                b.switch_to_block(add_blk);
                b.seal_block(add_blk);
                self.cached_add(b, addr, shape, cache, v, join, miss_blk);
            }
            None => {
                b.ins().brif(same, hit_blk, &[], miss_blk, &[]);
            }
        }

        // The slot: `offset >= 0` bytes into the object, or `!offset`
        // bytes into its overflow array.
        b.switch_to_block(hit_blk);
        b.seal_block(hit_blk);
        let offset = b
            .ins()
            .load(types::I32, flags, cache, ic::OFFSET_OFFSET as i32);
        let slot = Self::slot_addr(b, addr, offset);
        let hit = match store {
            Some(v) => {
                b.ins().store(flags, v, slot, 0);
                Self::c(b, UNDEFINED)
            }
            None => b.ins().load(types::I64, flags, slot, 0),
        };
        b.ins().jump(join, &[hit]);

        b.switch_to_block(miss_blk);
        b.seal_block(miss_blk);
        let miss = match store {
            Some(v) => self.rt_v(b, "ot_set_prop_ic", &[o, cache, v]),
            None => self.rt_v(b, "ot_get_prop_ic", &[o, cache]),
        };
        let unwind = self.unwind;
        self.check_exception(b, miss, unwind);
        self.last_call = None;
        b.ins().jump(join, &[miss]);

        b.switch_to_block(join);
        b.seal_block(join);
        b.block_params(join)[0]
    }

    /// The address of the slot `offset` (`ic::slot_offset`) of the object
    /// at `addr`: `offset >= 0` bytes into the object, or `!offset` bytes
    /// into its overflow array.
    fn slot_addr(b: &mut FunctionBuilder, addr: Value, offset: Value) -> Value {
        use crate::runtime::heap::NativeObject;
        let inline = b.ins().icmp_imm(IntCC::SignedGreaterThanOrEqual, offset, 0);
        let overflow = b.ins().load(
            types::I64,
            MemFlags::trusted(),
            addr,
            std::mem::offset_of!(NativeObject, overflow) as i32,
        );
        let base = b.ins().select(inline, addr, overflow);
        let flipped = b.ins().bnot(offset);
        let bytes = b.ins().select(inline, offset, flipped);
        let bytes = b.ins().uextend(types::I64, bytes);
        b.ins().iadd(base, bytes)
    }

    /// `recv.name(...args)` through an inline cache. When `recv` is an
    /// object of a cached shape, the method is loaded here: from the
    /// object itself, or from the object on its prototype chain that the
    /// cache names (while the prototype and the epoch are the cached
    /// ones). If it is a function, it is called here. Everything else
    /// goes to the runtime, which fills the cache.
    pub(super) fn call_method(
        &mut self,
        b: &mut FunctionBuilder,
        recv: Value,
        name: &str,
        argv: Value,
        argc: Value,
    ) -> Value {
        use crate::runtime::heap::{NativeFunction, NativeObject, ObjectKind};
        use crate::runtime::object::ic;
        use std::mem::offset_of;
        let cache = Self::prop_cache(b, name);
        let flags = MemFlags::trusted();
        let is_ptr_blk = b.create_block();
        let is_obj_blk = b.create_block();
        let own_blk = b.create_block();
        let chain_blk = b.create_block();
        let chain_hit_blk = b.create_block();
        let found_blk = b.create_block();
        let heap_blk = b.create_block();
        let call_blk = b.create_block();
        let miss_blk = b.create_block();
        let join = b.create_block();
        b.set_cold_block(miss_blk);
        b.append_block_param(found_blk, types::I64);
        b.append_block_param(join, types::I64);
        let cell = |b: &mut FunctionBuilder, ty, offset: usize| {
            b.ins().load(ty, flags, cache, offset as i32)
        };

        let (is_ptr, addr) = Self::heap_address(b, recv);
        b.ins().brif(is_ptr, is_ptr_blk, &[], miss_blk, &[]);

        b.switch_to_block(is_ptr_blk);
        b.seal_block(is_ptr_blk);
        let kind = b.ins().load(types::I8, flags, addr, 0);
        let is_obj = b
            .ins()
            .icmp_imm(IntCC::Equal, kind, ObjectKind::Object as i64);
        b.ins().brif(is_obj, is_obj_blk, &[], miss_blk, &[]);

        // The method is the object's own property.
        b.switch_to_block(is_obj_blk);
        b.seal_block(is_obj_blk);
        let shape = b.ins().load(
            types::I64,
            flags,
            addr,
            offset_of!(NativeObject, shape) as i32,
        );
        let cached = cell(b, types::I64, ic::SHAPE_OFFSET);
        let same = b.ins().icmp(IntCC::Equal, shape, cached);
        b.ins().brif(same, own_blk, &[], chain_blk, &[]);

        b.switch_to_block(own_blk);
        b.seal_block(own_blk);
        let offset = cell(b, types::I32, ic::OFFSET_OFFSET);
        let slot = Self::slot_addr(b, addr, offset);
        let method = b.ins().load(types::I64, flags, slot, 0);
        b.ins().jump(found_blk, &[method]);

        // Or it is on the prototype chain, where the cache says.
        b.switch_to_block(chain_blk);
        b.seal_block(chain_blk);
        let chain_shape = cell(b, types::I64, ic::CHAIN_SHAPE_OFFSET);
        let mut ok = b.ins().icmp(IntCC::Equal, shape, chain_shape);
        let proto = b.ins().load(
            types::I64,
            flags,
            addr,
            offset_of!(NativeObject, proto) as i32,
        );
        let cached_proto = cell(b, types::I64, ic::CHAIN_PROTO_OFFSET);
        let same_proto = b.ins().icmp(IntCC::Equal, proto, cached_proto);
        ok = b.ins().band(ok, same_proto);
        let epoch_addr = b.ins().iconst(types::I64, &ic::OT_EPOCH as *const _ as i64);
        let epoch = b.ins().load(types::I32, flags, epoch_addr, 0);
        let cached_epoch = cell(b, types::I32, ic::CHAIN_EPOCH_OFFSET);
        let same_epoch = b.ins().icmp(IntCC::Equal, epoch, cached_epoch);
        ok = b.ins().band(ok, same_epoch);
        b.ins().brif(ok, chain_hit_blk, &[], miss_blk, &[]);

        b.switch_to_block(chain_hit_blk);
        b.seal_block(chain_hit_blk);
        let holder = cell(b, types::I64, ic::CHAIN_HOLDER_OFFSET);
        let offset = cell(b, types::I32, ic::CHAIN_SLOT_OFFSET);
        let slot = Self::slot_addr(b, holder, offset);
        let method = b.ins().load(types::I64, flags, slot, 0);
        b.ins().jump(found_blk, &[method]);

        // A function: call its code with its environment.
        b.switch_to_block(found_blk);
        b.seal_block(found_blk);
        let method = b.block_params(found_blk)[0];
        let (is_heap, function) = Self::heap_address(b, method);
        b.ins().brif(is_heap, heap_blk, &[], miss_blk, &[]);

        b.switch_to_block(heap_blk);
        b.seal_block(heap_blk);
        let kind = b.ins().load(types::I8, flags, function, 0);
        let is_function = b
            .ins()
            .icmp_imm(IntCC::Equal, kind, ObjectKind::Function as i64);
        b.ins().brif(is_function, call_blk, &[], miss_blk, &[]);

        b.switch_to_block(call_blk);
        b.seal_block(call_blk);
        let code = b.ins().load(
            types::I64,
            flags,
            function,
            offset_of!(NativeFunction, code) as i32,
        );
        let env = b.ins().load(
            types::I64,
            flags,
            function,
            offset_of!(NativeFunction, env) as i32,
        );
        let mut sig = self.module.make_signature();
        for _ in 0..4 {
            sig.params.push(AbiParam::new(types::I64));
        }
        sig.returns.push(AbiParam::new(types::I64));
        let sig = b.import_signature(sig);
        let call = b.ins().call_indirect(sig, code, &[recv, argv, argc, env]);
        let result = b.inst_results(call)[0];
        b.ins().jump(join, &[result]);

        b.switch_to_block(miss_blk);
        b.seal_block(miss_blk);
        let miss = self.rt_v(b, "ot_call_method_ic", &[recv, cache, argv, argc]);
        b.ins().jump(join, &[miss]);

        b.switch_to_block(join);
        b.seal_block(join);
        let result = b.block_params(join)[0];
        self.last_call = Some(result);
        result
    }

    /// Whether `v` is a heap pointer (the pointer tag, and a payload that
    /// is not 0), and its address.
    fn heap_address(b: &mut FunctionBuilder, v: Value) -> (Value, Value) {
        let tag = b.ins().band_imm(v, 0x7FFF_0000_0000_0000);
        let tagged = b.ins().icmp_imm(IntCC::Equal, tag, QNAN as i64);
        let addr = b.ins().band_imm(v, 0x0000_FFFF_FFFF_FFFF);
        let nonnull = b.ins().icmp_imm(IntCC::NotEqual, addr, 0);
        (b.ins().band(tagged, nonnull), addr)
    }
}
