//! Objects in generated code: property reads and writes through inline
//! caches, object literals allocated in their final shape, and the cache
//! of a `new` site (docs/docs/object-model.md, sections 3 and 5).

use super::*;

impl Fx<'_> {
    /// A new inline cache (`runtime::object::ic::PropCache`) for a site
    /// that accesses the property `name`.
    pub(super) unsafe fn prop_cache(&mut self, name: &str) -> LLVMValueRef {
        unsafe {
            let ctx = self.cg.context;
            let (i64t, ptrt) = (self.i64_ty(), self.ptr_ty());
            let i32t = LLVMInt32TypeInContext(ctx);
            let (name_ptr, _) = self.string(name);
            let int = |t, v: u64| LLVMConstInt(t, v, 0);
            // The fields of `PropCache`, in order.
            let mut fields = [
                int(i64t, crate::runtime::object::ic::NO_SHAPE),
                int(i32t, 0),
                int(i32t, 0),
                name_ptr,
                int(i64t, name.len() as u64),
                int(i64t, crate::runtime::object::ic::NO_SHAPE),
                int(i64t, 0),
                int(i64t, 0),
                int(i32t, 0),
                int(i32t, 0),
                int(i32t, 0),
                int(i32t, 0),
            ];
            let init = LLVMConstStructInContext(ctx, fields.as_mut_ptr(), fields.len() as u32, 0);
            let g = LLVMAddGlobal(
                self.cg.module,
                LLVMTypeOf(init),
                b"prop_cache\0".as_ptr() as _,
            );
            LLVMSetInitializer(g, init);
            LLVMSetLinkage(g, LLVMLinkage::LLVMPrivateLinkage);
            let _ = ptrt;
            g
        }
    }

    /// A new cache (`runtime::object::ic::LiteralCache`) for a literal site
    /// that stores the properties `names` on an object with `slots` slots.
    pub(super) unsafe fn literal_cache(&mut self, names: &[String], slots: usize) -> LLVMValueRef {
        unsafe {
            let ctx = self.cg.context;
            let (i64t, ptrt) = (self.i64_ty(), self.ptr_ty());
            let i32t = LLVMInt32TypeInContext(ctx);
            let private = |g| {
                LLVMSetLinkage(g, LLVMLinkage::LLVMPrivateLinkage);
                g
            };
            // The names: an array of (pointer, length).
            let mut pair_types = [ptrt, i64t];
            let pair_ty = LLVMStructTypeInContext(ctx, pair_types.as_mut_ptr(), 2, 0);
            let mut pairs: Vec<LLVMValueRef> = names
                .iter()
                .map(|name| {
                    let (ptr, _) = self.string(name);
                    let mut fields = [ptr, LLVMConstInt(i64t, name.len() as u64, 0)];
                    LLVMConstNamedStruct(pair_ty, fields.as_mut_ptr(), 2)
                })
                .collect();
            let array = LLVMConstArray2(pair_ty, pairs.as_mut_ptr(), pairs.len() as u64);
            let names_global = private(LLVMAddGlobal(
                self.cg.module,
                LLVMTypeOf(array),
                b"literal_names\0".as_ptr() as _,
            ));
            LLVMSetInitializer(names_global, array);
            LLVMSetGlobalConstant(names_global, 1);
            // The fields of `LiteralCache`, in order.
            let slots = slots.min(crate::runtime::heap::MAX_INLINE_SLOTS);
            let mut fields = [
                LLVMConstInt(i64t, 0, 0),
                LLVMConstInt(i32t, slots as u64, 0),
                LLVMConstInt(i32t, names.len() as u64, 0),
                names_global,
            ];
            let init = LLVMConstStructInContext(ctx, fields.as_mut_ptr(), fields.len() as u32, 0);
            let g = private(LLVMAddGlobal(
                self.cg.module,
                LLVMTypeOf(init),
                b"literal_cache\0".as_ptr() as _,
            ));
            LLVMSetInitializer(g, init);
            g
        }
    }

    /// Store `v` in slot `i` of the object `o`, which has the slot in
    /// itself.
    pub(super) unsafe fn store_slot(&mut self, o: LLVMValueRef, i: usize, v: LLVMValueRef) {
        unsafe {
            let b = self.builder;
            let addr = LLVMBuildAnd(b, o, self.c(0x0000_FFFF_FFFF_FFFF), NONAME);
            let ptr = LLVMBuildIntToPtr(b, addr, self.ptr_ty(), NONAME);
            let offset = crate::runtime::object::ic::slot_offset(i, i + 1) as u64;
            let mut idx = [LLVMConstInt(self.i64_ty(), offset, 0)];
            let i8t = LLVMInt8TypeInContext(self.cg.context);
            let slot = LLVMBuildGEP2(b, i8t, ptr, idx.as_mut_ptr(), 1, NONAME);
            LLVMBuildStore(b, v, slot);
        }
    }

    /// A new cache (`runtime::object::ic::NewCache`) for a `new` site.
    pub(super) unsafe fn new_cache(&mut self) -> LLVMValueRef {
        unsafe {
            let ctx = self.cg.context;
            let i32t = LLVMInt32TypeInContext(ctx);
            let mut fields = [
                LLVMConstInt(self.i64_ty(), crate::runtime::object::ic::NO_SHAPE, 0),
                LLVMConstInt(i32t, 0, 0),
                LLVMConstInt(i32t, 0, 0),
                LLVMConstInt(i32t, 0, 0),
                LLVMConstInt(i32t, 0, 0),
            ];
            let init = LLVMConstStructInContext(ctx, fields.as_mut_ptr(), fields.len() as u32, 0);
            let g = LLVMAddGlobal(
                self.cg.module,
                LLVMTypeOf(init),
                b"new_cache\0".as_ptr() as _,
            );
            LLVMSetInitializer(g, init);
            LLVMSetLinkage(g, LLVMLinkage::LLVMPrivateLinkage);
            g
        }
    }

    /// The cached addition of a property to the object at `ptr`, whose
    /// shape is `shape`: when the shape and the prototype are the cached
    /// ones, no prototype has changed since, the object is not a prototype
    /// itself and the new slot is in the object, store `v` there and move
    /// the object to the next shape. Otherwise branches to `miss`. The
    /// builder is left in the block where the addition is done.
    unsafe fn cached_add(
        &mut self,
        ptr: LLVMValueRef,
        shape: LLVMValueRef,
        cache: LLVMValueRef,
        v: LLVMValueRef,
        miss: LLVMBasicBlockRef,
    ) {
        unsafe {
            use crate::runtime::heap::{NativeObject, ObjectHeader};
            use crate::runtime::object::ic;
            use std::mem::offset_of;
            let ctx = self.cg.context;
            let b = self.builder;
            let (i64t, ptrt) = (self.i64_ty(), self.ptr_ty());
            let i32t = LLVMInt32TypeInContext(ctx);
            let i8t = LLVMInt8TypeInContext(ctx);
            let at = |base: LLVMValueRef, offset: usize| {
                let mut idx = [LLVMConstInt(i64t, offset as u64, 0)];
                LLVMBuildGEP2(b, i8t, base, idx.as_mut_ptr(), 1, NONAME)
            };
            let eq = |x, y| LLVMBuildICmp(b, LLVMIntPredicate::LLVMIntEQ, x, y, NONAME);
            let done =
                LLVMAppendBasicBlockInContext(ctx, self.function, b"ic_added\0".as_ptr() as _);

            let before = LLVMBuildLoad2(b, i64t, at(cache, ic::ADD_SHAPE_OFFSET), NONAME);
            let mut ok = eq(shape, before);

            let proto = LLVMBuildLoad2(b, i64t, at(ptr, offset_of!(NativeObject, proto)), NONAME);
            let cached_proto = LLVMBuildLoad2(b, i64t, at(cache, ic::ADD_PROTO_OFFSET), NONAME);
            ok = LLVMBuildAnd(b, ok, eq(proto, cached_proto), NONAME);

            let epoch_global = self.epoch_global();
            let epoch = LLVMBuildLoad2(b, i32t, epoch_global, NONAME);
            let cached_epoch = LLVMBuildLoad2(b, i32t, at(cache, ic::ADD_EPOCH_OFFSET), NONAME);
            ok = LLVMBuildAnd(b, ok, eq(epoch, cached_epoch), NONAME);

            let flags = LLVMBuildLoad2(b, i8t, at(ptr, offset_of!(ObjectHeader, flags)), NONAME);
            let prototype = LLVMBuildAnd(
                b,
                flags,
                LLVMConstInt(i8t, ObjectHeader::PROTOTYPE as u64, 0),
                NONAME,
            );
            ok = LLVMBuildAnd(b, ok, eq(prototype, LLVMConstInt(i8t, 0, 0)), NONAME);

            let slot = LLVMBuildLoad2(b, i32t, at(cache, ic::ADD_SLOT_OFFSET), NONAME);
            let inline = LLVMBuildICmp(
                b,
                LLVMIntPredicate::LLVMIntSGE,
                slot,
                LLVMConstInt(i32t, 0, 0),
                NONAME,
            );
            ok = LLVMBuildAnd(b, ok, inline, NONAME);
            LLVMBuildCondBr(b, ok, done, miss);

            LLVMPositionBuilderAtEnd(b, done);
            let mut idx = [LLVMBuildZExt(b, slot, i64t, NONAME)];
            let place = LLVMBuildGEP2(b, i8t, ptr, idx.as_mut_ptr(), 1, NONAME);
            LLVMBuildStore(b, v, place);
            let next = LLVMBuildLoad2(b, ptrt, at(cache, ic::ADD_NEXT_OFFSET), NONAME);
            LLVMBuildStore(b, next, at(ptr, offset_of!(NativeObject, shape)));
        }
    }

    /// `o.name` (or `o.name = store`) through an inline cache: when `o` is
    /// an object whose shape is the cached one, load or store its slot
    /// here; otherwise call the runtime, which fills the cache.
    pub(super) unsafe fn cached_prop(
        &mut self,
        o: LLVMValueRef,
        name: &str,
        store: Option<LLVMValueRef>,
    ) -> LLVMValueRef {
        unsafe {
            use crate::runtime::heap::NativeObject;
            use crate::runtime::object::ic;
            use std::mem::offset_of;
            let ctx = self.cg.context;
            let b = self.builder;
            let (i64t, ptrt) = (self.i64_ty(), self.ptr_ty());
            let i32t = LLVMInt32TypeInContext(ctx);
            let i8t = LLVMInt8TypeInContext(ctx);
            let cache = self.prop_cache(name);
            let block = |this: &Self, name: &[u8]| {
                LLVMAppendBasicBlockInContext(ctx, this.function, name.as_ptr() as _)
            };
            let is_ptr_bb = block(self, b"ic_ptr\0");
            let is_obj_bb = block(self, b"ic_obj\0");
            let hit_bb = block(self, b"ic_hit\0");
            let miss_bb = block(self, b"ic_miss\0");
            let join_bb = block(self, b"ic_join\0");
            let at = |base: LLVMValueRef, offset: usize| {
                let mut idx = [LLVMConstInt(i64t, offset as u64, 0)];
                LLVMBuildGEP2(b, i8t, base, idx.as_mut_ptr(), 1, NONAME)
            };

            let mut add_done = None;

            // A heap pointer: the pointer tag, and a payload that is not 0.
            let tag = LLVMBuildAnd(b, o, self.c(0x7FFF_0000_0000_0000), NONAME);
            let tagged = LLVMBuildICmp(b, LLVMIntPredicate::LLVMIntEQ, tag, self.c(QNAN), NONAME);
            let addr = LLVMBuildAnd(b, o, self.c(0x0000_FFFF_FFFF_FFFF), NONAME);
            let nonnull = LLVMBuildICmp(b, LLVMIntPredicate::LLVMIntNE, addr, self.c(0), NONAME);
            let is_ptr = LLVMBuildAnd(b, tagged, nonnull, NONAME);
            LLVMBuildCondBr(b, is_ptr, is_ptr_bb, miss_bb);

            // An object (kind 0 in the header's first byte).
            LLVMPositionBuilderAtEnd(b, is_ptr_bb);
            let ptr = LLVMBuildIntToPtr(b, addr, ptrt, NONAME);
            let kind = LLVMBuildLoad2(b, i8t, ptr, NONAME);
            let is_obj = LLVMBuildICmp(
                b,
                LLVMIntPredicate::LLVMIntEQ,
                kind,
                LLVMConstInt(i8t, 0, 0),
                NONAME,
            );
            LLVMBuildCondBr(b, is_obj, is_obj_bb, miss_bb);

            // With the cached shape.
            LLVMPositionBuilderAtEnd(b, is_obj_bb);
            let shape = LLVMBuildLoad2(b, i64t, at(ptr, offset_of!(NativeObject, shape)), NONAME);
            let cached = LLVMBuildLoad2(b, i64t, at(cache, ic::SHAPE_OFFSET), NONAME);
            let same = LLVMBuildICmp(b, LLVMIntPredicate::LLVMIntEQ, shape, cached, NONAME);
            match store {
                // A write to an object of another shape may still be the
                // cached addition of the property.
                Some(v) => {
                    let add_bb = block(self, b"ic_add\0");
                    LLVMBuildCondBr(b, same, hit_bb, add_bb);
                    LLVMPositionBuilderAtEnd(b, add_bb);
                    self.cached_add(ptr, shape, cache, v, miss_bb);
                    add_done = Some(LLVMGetInsertBlock(b));
                    LLVMBuildBr(b, join_bb);
                }
                None => {
                    LLVMBuildCondBr(b, same, hit_bb, miss_bb);
                }
            }

            // The slot: `offset >= 0` bytes into the object, or `!offset`
            // bytes into its overflow array.
            LLVMPositionBuilderAtEnd(b, hit_bb);
            let offset = LLVMBuildLoad2(b, i32t, at(cache, ic::OFFSET_OFFSET), NONAME);
            let slot = self.slot_addr(ptr, offset);
            let hit_value = match store {
                Some(v) => {
                    LLVMBuildStore(b, v, slot);
                    self.c(UNDEFINED)
                }
                None => LLVMBuildLoad2(b, i64t, slot, NONAME),
            };
            LLVMBuildBr(b, join_bb);

            LLVMPositionBuilderAtEnd(b, miss_bb);
            let miss_value = match store {
                Some(v) => self.rt("ot_set_prop_ic", &[o, cache, v]),
                None => self.rt("ot_get_prop_ic", &[o, cache]),
            };
            self.check_exception(miss_value, self.unwind);
            self.last_call = None;
            let miss_end = LLVMGetInsertBlock(b);
            LLVMBuildBr(b, join_bb);

            LLVMPositionBuilderAtEnd(b, join_bb);
            let phi = LLVMBuildPhi(b, i64t, NONAME);
            let mut values = [hit_value, miss_value];
            let mut blocks = [hit_bb, miss_end];
            LLVMAddIncoming(phi, values.as_mut_ptr(), blocks.as_mut_ptr(), 2);
            if let Some(added) = add_done {
                let mut values = [self.c(UNDEFINED)];
                let mut blocks = [added];
                LLVMAddIncoming(phi, values.as_mut_ptr(), blocks.as_mut_ptr(), 1);
            }
            phi
        }
    }

    /// The address of the slot `offset` (`ic::slot_offset`) of the object
    /// at `ptr`: `offset >= 0` bytes into the object, or `!offset` bytes
    /// into its overflow array.
    unsafe fn slot_addr(&mut self, ptr: LLVMValueRef, offset: LLVMValueRef) -> LLVMValueRef {
        unsafe {
            use crate::runtime::heap::NativeObject;
            let ctx = self.cg.context;
            let b = self.builder;
            let (i64t, ptrt) = (self.i64_ty(), self.ptr_ty());
            let i32t = LLVMInt32TypeInContext(ctx);
            let i8t = LLVMInt8TypeInContext(ctx);
            let inline = LLVMBuildICmp(
                b,
                LLVMIntPredicate::LLVMIntSGE,
                offset,
                LLVMConstInt(i32t, 0, 0),
                NONAME,
            );
            let mut idx = [LLVMConstInt(
                i64t,
                std::mem::offset_of!(NativeObject, overflow) as u64,
                0,
            )];
            let field = LLVMBuildGEP2(b, i8t, ptr, idx.as_mut_ptr(), 1, NONAME);
            let overflow = LLVMBuildLoad2(b, ptrt, field, NONAME);
            let base = LLVMBuildSelect(b, inline, ptr, overflow, NONAME);
            let flipped = LLVMBuildNot(b, offset, NONAME);
            let bytes = LLVMBuildSelect(b, inline, offset, flipped, NONAME);
            let mut idx = [LLVMBuildZExt(b, bytes, i64t, NONAME)];
            LLVMBuildGEP2(b, i8t, base, idx.as_mut_ptr(), 1, NONAME)
        }
    }

    /// Whether `v` is a heap pointer (the pointer tag, and a payload that
    /// is not 0), and the pointer.
    unsafe fn heap_address(&mut self, v: LLVMValueRef) -> (LLVMValueRef, LLVMValueRef) {
        unsafe {
            let b = self.builder;
            let tag = LLVMBuildAnd(b, v, self.c(0x7FFF_0000_0000_0000), NONAME);
            let tagged = LLVMBuildICmp(b, LLVMIntPredicate::LLVMIntEQ, tag, self.c(QNAN), NONAME);
            let addr = LLVMBuildAnd(b, v, self.c(0x0000_FFFF_FFFF_FFFF), NONAME);
            let nonnull = LLVMBuildICmp(b, LLVMIntPredicate::LLVMIntNE, addr, self.c(0), NONAME);
            let is_ptr = LLVMBuildAnd(b, tagged, nonnull, NONAME);
            (is_ptr, LLVMBuildIntToPtr(b, addr, self.ptr_ty(), NONAME))
        }
    }

    /// `recv.name(...args)` through an inline cache. When `recv` is an
    /// object of a cached shape, the method is loaded here: from the
    /// object itself, or from the object on its prototype chain that the
    /// cache names (while the prototype and the epoch are the cached
    /// ones). If it is a function, it is called here. Everything else
    /// goes to the runtime, which fills the cache.
    pub(super) unsafe fn call_method(
        &mut self,
        recv: LLVMValueRef,
        name: &str,
        argv: LLVMValueRef,
        argc: LLVMValueRef,
    ) -> LLVMValueRef {
        unsafe {
            use crate::runtime::heap::{NativeFunction, NativeObject, ObjectKind};
            use crate::runtime::object::ic;
            use std::mem::offset_of;
            let ctx = self.cg.context;
            let b = self.builder;
            let (i64t, ptrt) = (self.i64_ty(), self.ptr_ty());
            let i32t = LLVMInt32TypeInContext(ctx);
            let i8t = LLVMInt8TypeInContext(ctx);
            let cache = self.prop_cache(name);
            let block = |this: &Self, name: &[u8]| {
                LLVMAppendBasicBlockInContext(ctx, this.function, name.as_ptr() as _)
            };
            let is_ptr_bb = block(self, b"call_ptr\0");
            let is_obj_bb = block(self, b"call_obj\0");
            let own_bb = block(self, b"call_own\0");
            let chain_bb = block(self, b"call_chain\0");
            let chain_hit_bb = block(self, b"call_chain_hit\0");
            let found_bb = block(self, b"call_found\0");
            let heap_bb = block(self, b"call_heap\0");
            let call_bb = block(self, b"call_direct\0");
            let miss_bb = block(self, b"call_miss\0");
            let join_bb = block(self, b"call_join\0");
            let at = |base: LLVMValueRef, offset: usize| {
                let mut idx = [LLVMConstInt(i64t, offset as u64, 0)];
                LLVMBuildGEP2(b, i8t, base, idx.as_mut_ptr(), 1, NONAME)
            };
            let eq = |x, y| LLVMBuildICmp(b, LLVMIntPredicate::LLVMIntEQ, x, y, NONAME);
            let kind_is = |ptr: LLVMValueRef, kind: ObjectKind| {
                let k = LLVMBuildLoad2(b, i8t, ptr, NONAME);
                eq(k, LLVMConstInt(i8t, kind as u64, 0))
            };

            let (is_ptr, ptr) = self.heap_address(recv);
            LLVMBuildCondBr(b, is_ptr, is_ptr_bb, miss_bb);

            LLVMPositionBuilderAtEnd(b, is_ptr_bb);
            LLVMBuildCondBr(b, kind_is(ptr, ObjectKind::Object), is_obj_bb, miss_bb);

            // The method is the object's own property.
            LLVMPositionBuilderAtEnd(b, is_obj_bb);
            let shape = LLVMBuildLoad2(b, i64t, at(ptr, offset_of!(NativeObject, shape)), NONAME);
            let cached = LLVMBuildLoad2(b, i64t, at(cache, ic::SHAPE_OFFSET), NONAME);
            LLVMBuildCondBr(b, eq(shape, cached), own_bb, chain_bb);

            LLVMPositionBuilderAtEnd(b, own_bb);
            let offset = LLVMBuildLoad2(b, i32t, at(cache, ic::OFFSET_OFFSET), NONAME);
            let slot = self.slot_addr(ptr, offset);
            let own_method = LLVMBuildLoad2(b, i64t, slot, NONAME);
            LLVMBuildBr(b, found_bb);

            // Or it is on the prototype chain, where the cache says.
            LLVMPositionBuilderAtEnd(b, chain_bb);
            let chain_shape = LLVMBuildLoad2(b, i64t, at(cache, ic::CHAIN_SHAPE_OFFSET), NONAME);
            let mut ok = eq(shape, chain_shape);
            let proto = LLVMBuildLoad2(b, i64t, at(ptr, offset_of!(NativeObject, proto)), NONAME);
            let cached_proto = LLVMBuildLoad2(b, i64t, at(cache, ic::CHAIN_PROTO_OFFSET), NONAME);
            ok = LLVMBuildAnd(b, ok, eq(proto, cached_proto), NONAME);
            let epoch = LLVMBuildLoad2(b, i32t, self.epoch_global(), NONAME);
            let cached_epoch = LLVMBuildLoad2(b, i32t, at(cache, ic::CHAIN_EPOCH_OFFSET), NONAME);
            ok = LLVMBuildAnd(b, ok, eq(epoch, cached_epoch), NONAME);
            LLVMBuildCondBr(b, ok, chain_hit_bb, miss_bb);

            LLVMPositionBuilderAtEnd(b, chain_hit_bb);
            let holder = LLVMBuildLoad2(b, ptrt, at(cache, ic::CHAIN_HOLDER_OFFSET), NONAME);
            let offset = LLVMBuildLoad2(b, i32t, at(cache, ic::CHAIN_SLOT_OFFSET), NONAME);
            let slot = self.slot_addr(holder, offset);
            let chain_method = LLVMBuildLoad2(b, i64t, slot, NONAME);
            LLVMBuildBr(b, found_bb);

            // A function: call its code with its environment.
            LLVMPositionBuilderAtEnd(b, found_bb);
            let method = LLVMBuildPhi(b, i64t, NONAME);
            let mut values = [own_method, chain_method];
            let mut blocks = [own_bb, chain_hit_bb];
            LLVMAddIncoming(method, values.as_mut_ptr(), blocks.as_mut_ptr(), 2);
            let (is_heap, function) = self.heap_address(method);
            LLVMBuildCondBr(b, is_heap, heap_bb, miss_bb);

            LLVMPositionBuilderAtEnd(b, heap_bb);
            LLVMBuildCondBr(b, kind_is(function, ObjectKind::Function), call_bb, miss_bb);

            LLVMPositionBuilderAtEnd(b, call_bb);
            let code = LLVMBuildLoad2(
                b,
                ptrt,
                at(function, offset_of!(NativeFunction, code)),
                NONAME,
            );
            let env = LLVMBuildLoad2(
                b,
                i64t,
                at(function, offset_of!(NativeFunction, env)),
                NONAME,
            );
            let mut args = [recv, argv, argc, env];
            let direct = LLVMBuildCall2(b, self.cg.fn_ty, code, args.as_mut_ptr(), 4, NONAME);
            LLVMBuildBr(b, join_bb);

            LLVMPositionBuilderAtEnd(b, miss_bb);
            let miss = self.rt("ot_call_method_ic", &[recv, cache, argv, argc]);
            LLVMBuildBr(b, join_bb);

            LLVMPositionBuilderAtEnd(b, join_bb);
            let result = LLVMBuildPhi(b, i64t, NONAME);
            let mut values = [direct, miss];
            let mut blocks = [call_bb, miss_bb];
            LLVMAddIncoming(result, values.as_mut_ptr(), blocks.as_mut_ptr(), 2);
            self.last_call = Some(result);
            result
        }
    }

    /// The global `OT_EPOCH` (`runtime::object::ic`), declared on first use.
    unsafe fn epoch_global(&mut self) -> LLVMValueRef {
        unsafe {
            let i32t = LLVMInt32TypeInContext(self.cg.context);
            let name = b"OT_EPOCH\0".as_ptr() as _;
            let global = LLVMGetNamedGlobal(self.cg.module, name);
            if global.is_null() {
                LLVMAddGlobal(self.cg.module, i32t, name)
            } else {
                global
            }
        }
    }
}
