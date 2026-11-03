//! Code generation: SSA IR → LLVM IR.
//!
//! Native calling convention (shared with the Cranelift JIT and the runtime,
//! see `runtime::heap::NativeFn`): every compiled function is
//! `i64 (i64 this, ptr argv, i64 argc, i64 env)`. Parameters are read from `argv`, and
//! one that was not passed reads as `undefined`.
//!
//! Number arithmetic on values the type checker proved are numbers is
//! inlined. Everything else calls the runtime (`src/runtime/stubs.rs`), which
//! is where the language's semantics live, so that both native backends agree.

#![allow(clippy::manual_c_str_literals)]
#![allow(clippy::missing_safety_doc)]

use llvm_sys::core::*;
use llvm_sys::prelude::*;
use llvm_sys::{LLVMIntPredicate, LLVMLinkage, LLVMRealPredicate};
use std::collections::{BTreeMap, HashMap};
use std::ffi::{CStr, CString, c_char};

use crate::backend::BackendError;
use crate::ir::{BlockId, IrFunction, IrModule, IrOp, IrType, Literal, Terminator, ValueId};

use super::abi;

const QNAN: u64 = 0x7FFC_0000_0000_0000;
const TAG_BOOLEAN: u64 = 0x0001_0000_0000_0000;
const TAG_NULL: u64 = 0x0002_0000_0000_0000;
const TAG_UNDEFINED: u64 = 0x0003_0000_0000_0000;
const UNDEFINED: u64 = QNAN | TAG_UNDEFINED;
use crate::runtime::abi::EXCEPTION_BITS;
const FALSE: u64 = QNAN | TAG_BOOLEAN;
const TRUE: u64 = QNAN | TAG_BOOLEAN | 1;

const NONAME: *const c_char = b"\0".as_ptr() as *const c_char;

/// LLVM code generator for AOT compilation
pub struct LlvmCodegen {
    pub context: LLVMContextRef,
    pub module: LLVMModuleRef,
    pub target_triple: String,
    /// Runtime functions: name -> (function, type).
    runtime: BTreeMap<String, (LLVMValueRef, LLVMTypeRef)>,
    /// Compiled functions by IR name (`main`, `func_<addr>`).
    pub functions: BTreeMap<String, LLVMValueRef>,
    /// Function bytecode address -> global holding its function object.
    fn_objects: BTreeMap<usize, LLVMValueRef>,
    /// Interned string constants: contents -> global.
    strings: HashMap<String, LLVMValueRef>,
    /// The native function type `i64 (i64 this, ptr argv, i64 argc, i64 env)`.
    fn_ty: LLVMTypeRef,
}

impl LlvmCodegen {
    pub fn new(target_triple: String) -> Result<Self, BackendError> {
        unsafe {
            llvm_sys::target::LLVM_InitializeNativeTarget();
            llvm_sys::target::LLVM_InitializeNativeAsmPrinter();
            llvm_sys::target::LLVM_InitializeNativeAsmParser();

            let context = LLVMContextCreate();
            if context.is_null() {
                return Err(BackendError::Llvm("Failed to create LLVM context".into()));
            }
            let module_name = CString::new("ot_module").unwrap();
            let module = LLVMModuleCreateWithNameInContext(module_name.as_ptr(), context);
            if module.is_null() {
                LLVMContextDispose(context);
                return Err(BackendError::Llvm("Failed to create LLVM module".into()));
            }
            let triple = CString::new(target_triple.clone()).unwrap();
            LLVMSetTarget(module, triple.as_ptr());

            let i64_ty = LLVMInt64TypeInContext(context);
            let ptr_ty = LLVMPointerTypeInContext(context, 0);
            let mut params = [i64_ty, ptr_ty, i64_ty, i64_ty];
            let fn_ty = LLVMFunctionType(i64_ty, params.as_mut_ptr(), 4, 0);

            Ok(Self {
                context,
                module,
                target_triple,
                runtime: BTreeMap::new(),
                functions: BTreeMap::new(),
                fn_objects: BTreeMap::new(),
                strings: HashMap::new(),
                fn_ty,
            })
        }
    }

    /// Compile an entire IR module, plus the C `main` entry point.
    pub fn compile_module(&mut self, ir_module: &IrModule) -> Result<(), BackendError> {
        unsafe {
            self.runtime = abi::declare_runtime(self.module, self.context)?;
            let i64_ty = LLVMInt64TypeInContext(self.context);

            // Declare every function first, so calls can refer to any of them.
            for func in &ir_module.functions {
                let llvm_name = if func.name == "main" {
                    "ot_main".to_string()
                } else {
                    func.name.clone()
                };
                let cname = CString::new(llvm_name).unwrap();
                let f = LLVMAddFunction(self.module, cname.as_ptr(), self.fn_ty);
                LLVMSetLinkage(f, LLVMLinkage::LLVMInternalLinkage);
                self.functions.insert(func.name.clone(), f);

                if let Some(addr) = function_addr(&func.name) {
                    let gname = CString::new(format!("ot_fnobj_{}", addr)).unwrap();
                    let g = LLVMAddGlobal(self.module, i64_ty, gname.as_ptr());
                    LLVMSetInitializer(g, LLVMConstInt(i64_ty, 0, 0));
                    LLVMSetLinkage(g, LLVMLinkage::LLVMInternalLinkage);
                    self.fn_objects.insert(addr, g);
                }
            }

            for func in &ir_module.functions {
                self.compile_function(func)?;
            }

            self.build_c_main()?;
            self.verify()
        }
    }

    /// `int main(int, char**)`: create the function objects, run top-level
    /// code, flush output.
    unsafe fn build_c_main(&mut self) -> Result<(), BackendError> {
        unsafe {
            let ctx = self.context;
            let i32_ty = LLVMInt32TypeInContext(ctx);
            let i64_ty = LLVMInt64TypeInContext(ctx);
            let ptr_ty = LLVMPointerTypeInContext(ctx, 0);
            let mut params = [i32_ty, ptr_ty];
            let main_ty = LLVMFunctionType(i32_ty, params.as_mut_ptr(), 2, 0);
            let main = LLVMAddFunction(self.module, b"main\0".as_ptr() as _, main_ty);
            let entry = LLVMAppendBasicBlockInContext(ctx, main, b"entry\0".as_ptr() as _);
            let b = LLVMCreateBuilderInContext(ctx);
            LLVMPositionBuilderAtEnd(b, entry);

            for (addr, global) in &self.fn_objects {
                let f = self.functions[&format!("func_{}", addr)];
                let (mk, mk_ty) = self.runtime["ot_make_function"];
                let mut args = [f];
                let obj = LLVMBuildCall2(b, mk_ty, mk, args.as_mut_ptr(), 1, NONAME);
                LLVMBuildStore(b, obj, *global);
            }

            let ot_main = self.functions["main"];
            let mut args = [
                LLVMConstInt(i64_ty, UNDEFINED, 0),
                LLVMConstNull(ptr_ty),
                LLVMConstInt(i64_ty, 0, 0),
                LLVMConstInt(i64_ty, UNDEFINED, 0),
            ];
            let result = LLVMBuildCall2(b, self.fn_ty, ot_main, args.as_mut_ptr(), 4, NONAME);
            // An exception no `catch` handled: report it and exit 1.
            let thrown = LLVMBuildICmp(
                b,
                LLVMIntPredicate::LLVMIntEQ,
                result,
                LLVMConstInt(i64_ty, EXCEPTION_BITS, 0),
                NONAME,
            );
            let uncaught = LLVMAppendBasicBlockInContext(ctx, main, b"uncaught\0".as_ptr() as _);
            let done = LLVMAppendBasicBlockInContext(ctx, main, b"done\0".as_ptr() as _);
            LLVMBuildCondBr(b, thrown, uncaught, done);
            LLVMPositionBuilderAtEnd(b, uncaught);
            let (report, report_ty) = self.runtime["ot_report_uncaught"];
            LLVMBuildCall2(b, report_ty, report, std::ptr::null_mut(), 0, NONAME);
            LLVMBuildUnreachable(b);
            LLVMPositionBuilderAtEnd(b, done);
            let (exit, exit_ty) = self.runtime["ot_runtime_exit"];
            LLVMBuildCall2(b, exit_ty, exit, std::ptr::null_mut(), 0, NONAME);
            LLVMBuildRet(b, LLVMConstInt(i32_ty, 0, 0));
            LLVMDisposeBuilder(b);
            Ok(())
        }
    }

    /// Fail loudly on malformed IR instead of letting LLVM crash later.
    unsafe fn verify(&self) -> Result<(), BackendError> {
        unsafe {
            let mut msg: *mut c_char = std::ptr::null_mut();
            let failed = llvm_sys::analysis::LLVMVerifyModule(
                self.module,
                llvm_sys::analysis::LLVMVerifierFailureAction::LLVMReturnStatusAction,
                &mut msg,
            );
            let text = if msg.is_null() {
                String::new()
            } else {
                let t = CStr::from_ptr(msg).to_string_lossy().into_owned();
                LLVMDisposeMessage(msg);
                t
            };
            if failed != 0 {
                return Err(BackendError::Llvm(format!(
                    "internal error: generated invalid LLVM IR:\n{}",
                    text
                )));
            }
            Ok(())
        }
    }

    unsafe fn compile_function(&mut self, func: &IrFunction) -> Result<(), BackendError> {
        unsafe {
            let f = self.functions[&func.name];
            let ctx = self.context;
            let builder = LLVMCreateBuilderInContext(ctx);
            let prologue_builder = LLVMCreateBuilderInContext(ctx);
            let prologue = LLVMAppendBasicBlockInContext(ctx, f, b"prologue\0".as_ptr() as _);
            LLVMPositionBuilderAtEnd(prologue_builder, prologue);

            let mut blocks = HashMap::new();
            for block in &func.blocks {
                let name = CString::new(format!("bb{}", block.id.0)).unwrap();
                blocks.insert(
                    block.id,
                    LLVMAppendBasicBlockInContext(ctx, f, name.as_ptr()),
                );
            }

            let mut fx = Fx {
                cg: self,
                builder,
                prologue: prologue_builder,
                values: HashMap::new(),
                blocks,
                locals: Vec::new(),
                types: &func.value_types,
                direct: HashMap::new(),
                this: LLVMGetParam(f, 0),
                env: LLVMGetParam(f, 3),
                function: f,
                last_call: None,
                exception_return: None,
            };
            fx.prologue_setup(func, f)?;

            for block in &func.blocks {
                LLVMPositionBuilderAtEnd(fx.builder, fx.blocks[&block.id]);
                for op in &block.ops {
                    fx.last_call = None;
                    fx.op(op)?;
                    if op.can_throw()
                        && let Some(r) = fx.last_call
                    {
                        fx.check_exception(r, block.unwind);
                    }
                }
                fx.terminator(&block.terminator)?;
            }
            if let Some(blk) = fx.exception_return {
                LLVMPositionBuilderAtEnd(fx.builder, blk);
                LLVMBuildRet(fx.builder, fx.c(EXCEPTION_BITS));
            }

            // The prologue falls through to the IR entry block.
            if let Some(first) = func.blocks.first() {
                LLVMBuildBr(fx.prologue, fx.blocks[&first.id]);
            } else {
                LLVMBuildRet(fx.prologue, fx.c(UNDEFINED));
            }
            LLVMDisposeBuilder(fx.prologue);
            LLVMDisposeBuilder(fx.builder);
            Ok(())
        }
    }
}

impl Drop for LlvmCodegen {
    fn drop(&mut self) {
        unsafe {
            LLVMDisposeModule(self.module);
            LLVMContextDispose(self.context);
        }
    }
}

/// `func_123` -> 123
fn function_addr(name: &str) -> Option<usize> {
    name.strip_prefix("func_")?.parse().ok()
}

/// Per-function translation state.
struct Fx<'a> {
    cg: &'a mut LlvmCodegen,
    builder: LLVMBuilderRef,
    /// Appends to the prologue block (allocas, parameter loads).
    prologue: LLVMBuilderRef,
    values: HashMap<ValueId, LLVMValueRef>,
    blocks: HashMap<BlockId, LLVMBasicBlockRef>,
    locals: Vec<LLVMValueRef>,
    types: &'a HashMap<ValueId, IrType>,
    /// Values known to be a specific compiled function (for direct calls),
    /// with the closure environment they carry, if any.
    direct: HashMap<ValueId, (usize, Option<ValueId>)>,
    this: LLVMValueRef,
    /// This function's own closure environment.
    env: LLVMValueRef,
    /// The function being compiled.
    function: LLVMValueRef,
    /// Result of the last call emitted: what an op that can throw is
    /// checked against `EXCEPTION_BITS`.
    last_call: Option<LLVMValueRef>,
    /// Block returning `EXCEPTION_BITS`, for exceptions no `catch` in this
    /// function handles.
    exception_return: Option<LLVMBasicBlockRef>,
}

impl Fx<'_> {
    fn i64_ty(&self) -> LLVMTypeRef {
        unsafe { LLVMInt64TypeInContext(self.cg.context) }
    }

    fn ptr_ty(&self) -> LLVMTypeRef {
        unsafe { LLVMPointerTypeInContext(self.cg.context, 0) }
    }

    fn f64_ty(&self) -> LLVMTypeRef {
        unsafe { LLVMDoubleTypeInContext(self.cg.context) }
    }

    /// A 64-bit constant.
    fn c(&self, bits: u64) -> LLVMValueRef {
        unsafe { LLVMConstInt(self.i64_ty(), bits, 0) }
    }

    fn get(&self, id: ValueId) -> Result<LLVMValueRef, BackendError> {
        self.values.get(&id).copied().ok_or_else(|| {
            BackendError::Llvm(format!(
                "internal error: value {:?} used before definition",
                id
            ))
        })
    }

    fn set(&mut self, id: ValueId, v: LLVMValueRef) {
        self.values.insert(id, v);
    }

    fn is_num(&self, id: ValueId) -> bool {
        matches!(self.types.get(&id), Some(IrType::Number))
    }

    fn is_bool(&self, id: ValueId) -> bool {
        matches!(self.types.get(&id), Some(IrType::Boolean))
    }

    /// Call a runtime function.
    unsafe fn rt(&mut self, name: &str, args: &[LLVMValueRef]) -> LLVMValueRef {
        unsafe {
            let (f, ty) = self.cg.runtime[name];
            let mut args = args.to_vec();
            let r = LLVMBuildCall2(
                self.builder,
                ty,
                f,
                args.as_mut_ptr(),
                args.len() as u32,
                NONAME,
            );
            self.last_call = Some(r);
            r
        }
    }

    /// After an op that can throw: if its result is `EXCEPTION_BITS`, go
    /// to the block's unwind target, or return the exception to the caller.
    unsafe fn check_exception(&mut self, result: LLVMValueRef, unwind: Option<BlockId>) {
        unsafe {
            let ctx = self.cg.context;
            let thrown = LLVMBuildICmp(
                self.builder,
                LLVMIntPredicate::LLVMIntEQ,
                result,
                self.c(EXCEPTION_BITS),
                NONAME,
            );
            // Exceptions are rare: `llvm.expect(thrown, false)` keeps the
            // normal path straight-line.
            let name = b"llvm.expect";
            let id = LLVMLookupIntrinsicID(name.as_ptr() as _, name.len());
            let i1 = LLVMInt1TypeInContext(ctx);
            let mut tys = [i1];
            let expect = LLVMGetIntrinsicDeclaration(self.cg.module, id, tys.as_mut_ptr(), 1);
            let mut params = [i1, i1];
            let expect_ty = LLVMFunctionType(i1, params.as_mut_ptr(), 2, 0);
            let mut args = [thrown, LLVMConstInt(i1, 0, 0)];
            let thrown = LLVMBuildCall2(
                self.builder,
                expect_ty,
                expect,
                args.as_mut_ptr(),
                2,
                NONAME,
            );
            let target = match unwind {
                Some(u) => self.blocks[&u],
                None => match self.exception_return {
                    Some(blk) => blk,
                    None => {
                        let blk = LLVMAppendBasicBlockInContext(
                            ctx,
                            self.function,
                            b"exception_return\0".as_ptr() as _,
                        );
                        self.exception_return = Some(blk);
                        blk
                    }
                },
            };
            let next = LLVMAppendBasicBlockInContext(ctx, self.function, b"ok\0".as_ptr() as _);
            LLVMBuildCondBr(self.builder, thrown, target, next);
            LLVMPositionBuilderAtEnd(self.builder, next);
        }
    }

    /// Pointer and length of an interned string constant.
    unsafe fn string(&mut self, s: &str) -> (LLVMValueRef, LLVMValueRef) {
        unsafe {
            let global = match self.cg.strings.get(s) {
                Some(g) => *g,
                None => {
                    let init = LLVMConstStringInContext(
                        self.cg.context,
                        s.as_ptr() as *const c_char,
                        s.len() as u32,
                        1, // no NUL terminator
                    );
                    let g = LLVMAddGlobal(self.cg.module, LLVMTypeOf(init), b"str\0".as_ptr() as _);
                    LLVMSetInitializer(g, init);
                    LLVMSetGlobalConstant(g, 1);
                    LLVMSetLinkage(g, LLVMLinkage::LLVMPrivateLinkage);
                    self.cg.strings.insert(s.to_string(), g);
                    g
                }
            };
            (global, self.c(s.len() as u64))
        }
    }

    /// Store values into a stack array; returns (argv, argc).
    unsafe fn arg_array(&mut self, vals: &[LLVMValueRef]) -> (LLVMValueRef, LLVMValueRef) {
        unsafe {
            if vals.is_empty() {
                return (LLVMConstNull(self.ptr_ty()), self.c(0));
            }
            let arr_ty = LLVMArrayType2(self.i64_ty(), vals.len() as u64);
            let slot = LLVMBuildAlloca(self.prologue, arr_ty, b"argv\0".as_ptr() as _);
            for (i, v) in vals.iter().enumerate() {
                let mut idx = [self.c(i as u64)];
                let p = LLVMBuildGEP2(
                    self.builder,
                    self.i64_ty(),
                    slot,
                    idx.as_mut_ptr(),
                    1,
                    NONAME,
                );
                LLVMBuildStore(self.builder, *v, p);
            }
            (slot, self.c(vals.len() as u64))
        }
    }

    unsafe fn to_f64(&self, v: LLVMValueRef) -> LLVMValueRef {
        unsafe { LLVMBuildBitCast(self.builder, v, self.f64_ty(), NONAME) }
    }

    unsafe fn from_f64(&self, v: LLVMValueRef) -> LLVMValueRef {
        unsafe { LLVMBuildBitCast(self.builder, v, self.i64_ty(), NONAME) }
    }

    /// i1 -> boxed boolean
    unsafe fn box_bool(&self, b: LLVMValueRef) -> LLVMValueRef {
        unsafe {
            let ext = LLVMBuildZExt(self.builder, b, self.i64_ty(), NONAME);
            LLVMBuildOr(self.builder, self.c(FALSE), ext, NONAME)
        }
    }

    /// Truthiness of a value as i1.
    unsafe fn truthy(&mut self, id: ValueId) -> Result<LLVMValueRef, BackendError> {
        unsafe {
            let v = self.get(id)?;
            if self.is_bool(id) {
                return Ok(LLVMBuildICmp(
                    self.builder,
                    LLVMIntPredicate::LLVMIntEQ,
                    v,
                    self.c(TRUE),
                    NONAME,
                ));
            }
            let t = self.rt("ot_truthy", &[v]);
            Ok(LLVMBuildICmp(
                self.builder,
                LLVMIntPredicate::LLVMIntNE,
                t,
                self.c(0),
                NONAME,
            ))
        }
    }

    /// Parameters, `this`, local slots.
    unsafe fn prologue_setup(
        &mut self,
        func: &IrFunction,
        f: LLVMValueRef,
    ) -> Result<(), BackendError> {
        unsafe {
            let b = self.prologue;
            let i64_ty = self.i64_ty();
            for _ in &func.locals {
                let slot = LLVMBuildAlloca(b, i64_ty, b"local\0".as_ptr() as _);
                LLVMBuildStore(b, self.c(UNDEFINED), slot);
                self.locals.push(slot);
            }
            let argv = LLVMGetParam(f, 1);
            let argc = LLVMGetParam(f, 2);
            let undef_slot = LLVMBuildAlloca(b, i64_ty, b"undef\0".as_ptr() as _);
            LLVMBuildStore(b, self.c(UNDEFINED), undef_slot);
            for i in 0..func.params.len() {
                // argv[i] if i < argc, else undefined (branch-free).
                let present = LLVMBuildICmp(
                    b,
                    LLVMIntPredicate::LLVMIntULT,
                    self.c(i as u64),
                    argc,
                    NONAME,
                );
                let mut idx = [self.c(i as u64)];
                let p = LLVMBuildGEP2(b, i64_ty, argv, idx.as_mut_ptr(), 1, NONAME);
                let src = LLVMBuildSelect(b, present, p, undef_slot, NONAME);
                let v = LLVMBuildLoad2(b, i64_ty, src, NONAME);
                self.set(ValueId(i as u32), v);
            }

            // Values that are a known compiled function, for direct calls.
            for block in &func.blocks {
                for op in &block.ops {
                    match op {
                        IrOp::Const(dst, Literal::Function(addr)) => {
                            self.direct.insert(*dst, (*addr, None));
                        }
                        IrOp::MakeClosure(dst, addr, env) => {
                            self.direct.insert(*dst, (*addr as usize, Some(*env)));
                        }
                        _ => {}
                    }
                }
            }
            Ok(())
        }
    }

    unsafe fn literal(&mut self, lit: &Literal) -> Result<LLVMValueRef, BackendError> {
        unsafe {
            Ok(match lit {
                Literal::Number(n) => self.c(n.to_bits()),
                Literal::Boolean(b) => self.c(if *b { TRUE } else { FALSE }),
                Literal::Null => self.c(QNAN | TAG_NULL),
                Literal::Undefined => self.c(UNDEFINED),
                Literal::String(s) => {
                    let (p, n) = self.string(s);
                    self.rt("ot_alloc_string", &[p, n])
                }
                Literal::Function(addr) => {
                    let g = *self.cg.fn_objects.get(addr).ok_or_else(|| {
                        BackendError::Llvm(format!("internal error: no function at {}", addr))
                    })?;
                    LLVMBuildLoad2(self.builder, self.i64_ty(), g, NONAME)
                }
            })
        }
    }

    unsafe fn num_binop(
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

    unsafe fn rt_binop(
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

    /// A comparison: inline when both operands are numbers, else the runtime.
    unsafe fn compare(
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
                self.rt_binop(dst, stub, a, b)
            }
        }
    }

    /// Flip a boxed boolean.
    unsafe fn negate(&mut self, v: LLVMValueRef) -> LLVMValueRef {
        unsafe { LLVMBuildXor(self.builder, v, self.c(1), NONAME) }
    }

    unsafe fn op(&mut self, op: &IrOp) -> Result<(), BackendError> {
        unsafe {
            match op {
                IrOp::Const(dst, lit) => {
                    let v = self.literal(lit)?;
                    self.set(*dst, v);
                }

                // Numbers the type checker proved: inline floating point.
                IrOp::AddNum(d, a, b) => self.num_binop(*d, *a, *b, LLVMBuildFAdd)?,
                IrOp::SubNum(d, a, b) => self.num_binop(*d, *a, *b, LLVMBuildFSub)?,
                IrOp::MulNum(d, a, b) => self.num_binop(*d, *a, *b, LLVMBuildFMul)?,
                IrOp::DivNum(d, a, b) => self.num_binop(*d, *a, *b, LLVMBuildFDiv)?,
                IrOp::ModNum(d, a, b) => self.num_binop(*d, *a, *b, LLVMBuildFRem)?,
                IrOp::NegNum(d, a) => {
                    let x = self.to_f64(self.get(*a)?);
                    let r = LLVMBuildFNeg(self.builder, x, NONAME);
                    let r = self.from_f64(r);
                    self.set(*d, r);
                }

                // Everything else: the runtime.
                IrOp::AddAny(d, a, b) => self.rt_binop(*d, "ot_add_any", *a, *b)?,
                IrOp::SubAny(d, a, b) => self.rt_binop(*d, "ot_sub_any", *a, *b)?,
                IrOp::MulAny(d, a, b) => self.rt_binop(*d, "ot_mul_any", *a, *b)?,
                IrOp::DivAny(d, a, b) => self.rt_binop(*d, "ot_div_any", *a, *b)?,
                IrOp::ModAny(d, a, b) => self.rt_binop(*d, "ot_mod_any", *a, *b)?,
                IrOp::Pow(d, a, b) => self.rt_binop(*d, "ot_pow", *a, *b)?,
                IrOp::BitAnd(d, a, b) => self.rt_binop(*d, "ot_bit_and", *a, *b)?,
                IrOp::BitOr(d, a, b) => self.rt_binop(*d, "ot_bit_or", *a, *b)?,
                IrOp::Xor(d, a, b) => self.rt_binop(*d, "ot_bit_xor", *a, *b)?,
                IrOp::Shl(d, a, b) => self.rt_binop(*d, "ot_shl", *a, *b)?,
                IrOp::Shr(d, a, b) => self.rt_binop(*d, "ot_shr", *a, *b)?,
                IrOp::ShrU(d, a, b) => self.rt_binop(*d, "ot_shr_u", *a, *b)?,
                IrOp::NegAny(d, a) => {
                    let r = self.rt("ot_neg", &[self.get(*a)?]);
                    self.set(*d, r);
                }

                IrOp::EqStrict(d, a, b) | IrOp::NeStrict(d, a, b) => {
                    let r = if self.is_num(*a) && self.is_num(*b) {
                        let x = self.to_f64(self.get(*a)?);
                        let y = self.to_f64(self.get(*b)?);
                        let cmp = LLVMBuildFCmp(
                            self.builder,
                            LLVMRealPredicate::LLVMRealOEQ,
                            x,
                            y,
                            NONAME,
                        );
                        self.box_bool(cmp)
                    } else {
                        self.rt("ot_eq_strict", &[self.get(*a)?, self.get(*b)?])
                    };
                    let r = if matches!(op, IrOp::NeStrict(..)) {
                        self.negate(r)
                    } else {
                        r
                    };
                    self.set(*d, r);
                }
                IrOp::EqLoose(d, a, b) | IrOp::NeLoose(d, a, b) => {
                    let r = self.rt("ot_eq_loose", &[self.get(*a)?, self.get(*b)?]);
                    let r = if matches!(op, IrOp::NeLoose(..)) {
                        self.negate(r)
                    } else {
                        r
                    };
                    self.set(*d, r);
                }
                IrOp::Lt(d, a, b) => {
                    self.compare(*d, *a, *b, LLVMRealPredicate::LLVMRealOLT, "ot_lt")?
                }
                IrOp::LtEq(d, a, b) => {
                    self.compare(*d, *a, *b, LLVMRealPredicate::LLVMRealOLE, "ot_lte")?
                }
                IrOp::Gt(d, a, b) => {
                    self.compare(*d, *a, *b, LLVMRealPredicate::LLVMRealOGT, "ot_gt")?
                }
                IrOp::GtEq(d, a, b) => {
                    self.compare(*d, *a, *b, LLVMRealPredicate::LLVMRealOGE, "ot_gte")?
                }

                IrOp::Not(d, a) => {
                    let r = if self.is_bool(*a) {
                        let v = self.get(*a)?;
                        self.negate(v)
                    } else {
                        self.rt("ot_not", &[self.get(*a)?])
                    };
                    self.set(*d, r);
                }
                IrOp::And(d, a, b) | IrOp::Or(d, a, b) => {
                    // a && b: a if falsy, else b.  a || b: a if truthy, else b.
                    let t = self.truthy(*a)?;
                    let (x, y) = (self.get(*a)?, self.get(*b)?);
                    let r = if matches!(op, IrOp::And(..)) {
                        LLVMBuildSelect(self.builder, t, y, x, NONAME)
                    } else {
                        LLVMBuildSelect(self.builder, t, x, y, NONAME)
                    };
                    self.set(*d, r);
                }
                IrOp::ToBool(d, a) => {
                    let r = self.rt("ot_to_boolean", &[self.get(*a)?]);
                    self.set(*d, r);
                }
                IrOp::ToNum(d, a) => {
                    let r = self.rt("ot_to_number", &[self.get(*a)?]);
                    self.set(*d, r);
                }
                IrOp::TypeOf(d, a) => {
                    let r = self.rt("ot_typeof", &[self.get(*a)?]);
                    self.set(*d, r);
                }

                IrOp::LoadLocal(d, slot) => {
                    let p = *self.locals.get(*slot as usize).ok_or_else(|| {
                        BackendError::Llvm(format!("internal error: bad local slot {}", slot))
                    })?;
                    let v = LLVMBuildLoad2(self.builder, self.i64_ty(), p, NONAME);
                    self.set(*d, v);
                }
                IrOp::StoreLocal(slot, v) => {
                    let p = *self.locals.get(*slot as usize).ok_or_else(|| {
                        BackendError::Llvm(format!("internal error: bad local slot {}", slot))
                    })?;
                    LLVMBuildStore(self.builder, self.get(*v)?, p);
                }
                IrOp::LoadGlobal(d, name) => {
                    let (p, n) = self.string(name);
                    let r = self.rt("ot_get_global", &[p, n]);
                    self.set(*d, r);
                }
                IrOp::TypeOfGlobal(d, name) => {
                    let (p, n) = self.string(name);
                    let r = self.rt("ot_typeof_global", &[p, n]);
                    self.set(*d, r);
                }
                IrOp::StoreGlobal(name, v) => {
                    let (p, n) = self.string(name);
                    let v = self.get(*v)?;
                    self.rt("ot_set_global", &[p, n, v]);
                }
                IrOp::LoadEnv(d) => {
                    let e = self.env;
                    self.set(*d, e);
                }
                IrOp::LoadThis(d) => {
                    let t = self.this;
                    self.set(*d, t);
                }

                IrOp::Copy(d, s)
                | IrOp::Move(d, s)
                | IrOp::Borrow(d, s)
                | IrOp::BorrowMut(d, s) => {
                    let v = self.get(*s)?;
                    self.set(*d, v);
                    if let Some(addr) = self.direct.get(s).copied() {
                        self.direct.insert(*d, addr);
                    }
                }

                IrOp::NewObject(d) => {
                    let r = self.rt("ot_alloc_object", &[]);
                    self.set(*d, r);
                }
                IrOp::NewArray(d) => {
                    let r = self.rt("ot_alloc_array", &[self.c(4)]);
                    self.set(*d, r);
                }
                IrOp::GetProp(d, obj, name) => {
                    let o = self.get(*obj)?;
                    let (p, n) = self.string(name);
                    let r = self.rt("ot_get_prop", &[o, p, n]);
                    self.set(*d, r);
                }
                IrOp::SetProp(obj, name, v) => {
                    let o = self.get(*obj)?;
                    let v = self.get(*v)?;
                    let (p, n) = self.string(name);
                    self.rt("ot_set_prop", &[o, p, n, v]);
                }
                IrOp::DeleteProp(d, obj, name) => {
                    let o = self.get(*obj)?;
                    let (p, n) = self.string(name);
                    let r = self.rt("ot_delete_prop", &[o, p, n]);
                    self.set(*d, r);
                }
                IrOp::GetElement(d, obj, key) => self.rt_binop(*d, "ot_get_index", *obj, *key)?,
                IrOp::SetElement(obj, key, v) => {
                    let args = [self.get(*obj)?, self.get(*key)?, self.get(*v)?];
                    self.rt("ot_set_index", &args);
                }
                IrOp::ArrayLen(d, a) => {
                    let r = self.rt("ot_array_len", &[self.get(*a)?]);
                    self.set(*d, r);
                }
                IrOp::ArrayPush(a, v) => {
                    let args = [self.get(*a)?, self.get(*v)?];
                    self.rt("ot_array_push", &args);
                }

                IrOp::MakeClosure(d, addr, env) => {
                    let name = format!("func_{}", addr);
                    let f = *self.cg.functions.get(&name).ok_or_else(|| {
                        BackendError::Llvm(format!("internal error: no function {}", name))
                    })?;
                    let e = self.get(*env)?;
                    let v = self.rt("ot_make_closure", &[f, e]);
                    self.set(*d, v);
                }
                IrOp::Call(d, callee, args) => {
                    let vals: Vec<LLVMValueRef> = args
                        .iter()
                        .map(|a| self.get(*a))
                        .collect::<Result<_, _>>()?;
                    let (argv, argc) = self.arg_array(&vals);
                    let undefined = self.c(UNDEFINED);
                    let target = self.direct.get(callee).copied().and_then(|(addr, env)| {
                        self.cg
                            .functions
                            .get(&format!("func_{}", addr))
                            .map(|f| (*f, env))
                    });
                    let r = match target {
                        Some((f, env)) => {
                            let env = match env {
                                Some(e) => self.get(e)?,
                                None => undefined,
                            };
                            let mut a = [undefined, argv, argc, env];
                            let r = LLVMBuildCall2(
                                self.builder,
                                self.cg.fn_ty,
                                f,
                                a.as_mut_ptr(),
                                4,
                                NONAME,
                            );
                            self.last_call = Some(r);
                            r
                        }
                        None => {
                            let f = self.get(*callee)?;
                            self.rt("ot_call", &[f, undefined, argv, argc])
                        }
                    };
                    self.set(*d, r);
                }
                IrOp::Throw(v) => {
                    let v = self.get(*v)?;
                    self.rt("ot_throw", &[v]);
                }
                IrOp::TakeException(d) => {
                    let r = self.rt("ot_take_exception", &[]);
                    self.set(*d, r);
                }
                IrOp::Rethrow(thrown, v) => {
                    let args = [self.get(*thrown)?, self.get(*v)?];
                    self.rt("ot_rethrow", &args);
                }
                IrOp::CallMethod(d, recv, name, args) => {
                    let r = self.get(*recv)?;
                    let vals: Vec<LLVMValueRef> = args
                        .iter()
                        .map(|a| self.get(*a))
                        .collect::<Result<_, _>>()?;
                    let (argv, argc) = self.arg_array(&vals);
                    let (p, n) = self.string(name);
                    let v = self.rt("ot_call_method", &[r, p, n, argv, argc]);
                    self.set(*d, v);
                }

                other => {
                    return Err(BackendError::UnsupportedOp(format!(
                        "the LLVM backend does not support `{}` yet",
                        other
                    )));
                }
            }
            Ok(())
        }
    }

    unsafe fn terminator(&mut self, term: &Terminator) -> Result<(), BackendError> {
        unsafe {
            match term {
                Terminator::Jump(t) => {
                    LLVMBuildBr(self.builder, self.blocks[t]);
                }
                Terminator::Branch(cond, t, f) => {
                    let c = self.truthy(*cond)?;
                    LLVMBuildCondBr(self.builder, c, self.blocks[t], self.blocks[f]);
                }
                Terminator::Return(v) => {
                    let r = match v {
                        Some(v) => self.get(*v)?,
                        None => self.c(UNDEFINED),
                    };
                    LLVMBuildRet(self.builder, r);
                }
                Terminator::Unreachable => {
                    LLVMBuildUnreachable(self.builder);
                }
            }
            Ok(())
        }
    }
}
