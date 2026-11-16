//! Arrays in generated code: element reads and writes with an index in
//! bounds, and `length`, without calling the runtime
//! (docs/docs/object-model.md, section 4).

use super::*;
use crate::runtime::heap::{NativeArray, ObjectKind};
use std::mem::offset_of;

impl Fx<'_> {
    /// Whether `v` is a heap pointer, and its address. Emitted in the
    /// current block.
    pub(super) unsafe fn heap_pointer(&mut self, v: LLVMValueRef) -> (LLVMValueRef, LLVMValueRef) {
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

    unsafe fn field(&mut self, base: LLVMValueRef, offset: usize) -> LLVMValueRef {
        unsafe {
            let i8t = LLVMInt8TypeInContext(self.cg.context);
            let mut idx = [self.c(offset as u64)];
            LLVMBuildGEP2(self.builder, i8t, base, idx.as_mut_ptr(), 1, NONAME)
        }
    }

    unsafe fn new_block(&mut self, name: &[u8]) -> LLVMBasicBlockRef {
        unsafe { LLVMAppendBasicBlockInContext(self.cg.context, self.function, name.as_ptr() as _) }
    }

    /// `o[key]` (or `o[key] = store`): when `o` is an array and `key` an
    /// integer below its length, load or store the element here; otherwise
    /// call the runtime (other receivers and keys, holes past the end,
    /// growth).
    pub(super) unsafe fn element(
        &mut self,
        o: LLVMValueRef,
        key: LLVMValueRef,
        store: Option<LLVMValueRef>,
    ) -> LLVMValueRef {
        unsafe {
            let b = self.builder;
            let ctx = self.cg.context;
            let (i64t, f64t) = (self.i64_ty(), self.f64_ty());
            let i8t = LLVMInt8TypeInContext(ctx);
            let i32t = LLVMInt32TypeInContext(ctx);
            let ptr_bb = self.new_block(b"elem_ptr\0");
            let arr_bb = self.new_block(b"elem_array\0");
            let hit_bb = self.new_block(b"elem_hit\0");
            let miss_bb = self.new_block(b"elem_miss\0");
            let join_bb = self.new_block(b"elem_join\0");

            let (is_ptr, ptr) = self.heap_pointer(o);
            LLVMBuildCondBr(b, is_ptr, ptr_bb, miss_bb);

            LLVMPositionBuilderAtEnd(b, ptr_bb);
            let kind = LLVMBuildLoad2(b, i8t, ptr, NONAME);
            let array = LLVMConstInt(i8t, ObjectKind::Array as u64, 0);
            let is_array = LLVMBuildICmp(b, LLVMIntPredicate::LLVMIntEQ, kind, array, NONAME);
            LLVMBuildCondBr(b, is_array, arr_bb, miss_bb);

            // The key as an index: a number that is an integer (a value
            // that is not a number reads as NaN and fails the comparison),
            // below the length (a negative one is huge as unsigned).
            LLVMPositionBuilderAtEnd(b, arr_bb);
            let f = self.to_f64(key);
            let i = self.call_intrinsic(b"llvm.fptosi.sat", &[i64t, f64t], i64t, &mut [f]);
            let back = LLVMBuildSIToFP(b, i, f64t, NONAME);
            let exact = LLVMBuildFCmp(b, LLVMRealPredicate::LLVMRealOEQ, back, f, NONAME);
            let len_ptr = self.field(ptr, offset_of!(NativeArray, len));
            let len = LLVMBuildLoad2(b, i32t, len_ptr, NONAME);
            let len = LLVMBuildZExt(b, len, i64t, NONAME);
            let in_bounds = LLVMBuildICmp(b, LLVMIntPredicate::LLVMIntULT, i, len, NONAME);
            let hit = LLVMBuildAnd(b, exact, in_bounds, NONAME);
            LLVMBuildCondBr(b, hit, hit_bb, miss_bb);

            LLVMPositionBuilderAtEnd(b, hit_bb);
            let elements_ptr = self.field(ptr, offset_of!(NativeArray, elements));
            let elements = LLVMBuildLoad2(b, self.ptr_ty(), elements_ptr, NONAME);
            let mut idx = [i];
            let slot = LLVMBuildGEP2(b, i64t, elements, idx.as_mut_ptr(), 1, NONAME);
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
                Some(v) => self.rt("ot_set_index", &[o, key, v]),
                None => self.rt("ot_get_index", &[o, key]),
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
            phi
        }
    }

    /// `o.length`: the length field of an array or a string, read here;
    /// for anything else, the property `length` through its cache.
    pub(super) unsafe fn length(&mut self, o: LLVMValueRef) -> LLVMValueRef {
        unsafe {
            let b = self.builder;
            let ctx = self.cg.context;
            let i8t = LLVMInt8TypeInContext(ctx);
            let i32t = LLVMInt32TypeInContext(ctx);
            let ptr_bb = self.new_block(b"len_ptr\0");
            let hit_bb = self.new_block(b"len_hit\0");
            let miss_bb = self.new_block(b"len_miss\0");
            let join_bb = self.new_block(b"len_join\0");

            let (is_ptr, ptr) = self.heap_pointer(o);
            LLVMBuildCondBr(b, is_ptr, ptr_bb, miss_bb);

            // Arrays and strings both keep their length at the same offset.
            LLVMPositionBuilderAtEnd(b, ptr_bb);
            let kind = LLVMBuildLoad2(b, i8t, ptr, NONAME);
            let is = |k: ObjectKind| {
                let k = LLVMConstInt(i8t, k as u64, 0);
                LLVMBuildICmp(b, LLVMIntPredicate::LLVMIntEQ, kind, k, NONAME)
            };
            let has_len = LLVMBuildOr(b, is(ObjectKind::Array), is(ObjectKind::String), NONAME);
            LLVMBuildCondBr(b, has_len, hit_bb, miss_bb);

            LLVMPositionBuilderAtEnd(b, hit_bb);
            let len_ptr = self.field(ptr, offset_of!(NativeArray, len));
            let len = LLVMBuildLoad2(b, i32t, len_ptr, NONAME);
            let len = LLVMBuildUIToFP(b, len, self.f64_ty(), NONAME);
            let hit_value = self.from_f64(len);
            LLVMBuildBr(b, join_bb);

            LLVMPositionBuilderAtEnd(b, miss_bb);
            let miss_value = self.cached_prop(o, "length", None);
            let miss_end = LLVMGetInsertBlock(b);
            LLVMBuildBr(b, join_bb);

            LLVMPositionBuilderAtEnd(b, join_bb);
            let phi = LLVMBuildPhi(b, self.i64_ty(), NONAME);
            let mut values = [hit_value, miss_value];
            let mut blocks = [hit_bb, miss_end];
            LLVMAddIncoming(phi, values.as_mut_ptr(), blocks.as_mut_ptr(), 2);
            phi
        }
    }
}
