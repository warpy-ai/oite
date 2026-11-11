//! Code generation: SSA IR → LLVM IR.
//!
//! Native calling convention (shared with the Cranelift JIT and the runtime,
//! see `runtime::heap::NativeFn`): every compiled function is
//! `i64 (i64 this, ptr argv, i64 argc, i64 env)`. Parameters are read from `argv`, and
//! one that was not passed reads as `undefined`.
//!
//! Number arithmetic on values the type checker proved are numbers is
//! inlined. Everything else calls the runtime (`src/runtime/stubs/`), which
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
use crate::runtime::abi::{EXCEPTION_BITS, UNBOUND_BITS};
const FALSE: u64 = QNAN | TAG_BOOLEAN;
const TRUE: u64 = QNAN | TAG_BOOLEAN | 1;

const NONAME: *const c_char = b"\0".as_ptr() as *const c_char;

mod arrays;
mod entry;
mod equality;
mod fast;
mod numeric;
mod objects;
mod ops;
mod typecheck;

/// A bitwise operator on 32-bit integers.
#[derive(Clone, Copy)]
enum IntOp {
    And,
    Or,
    Xor,
    Shl,
    Shr,
    ShrU,
}

/// LLVM code generator for AOT compilation
pub struct LlvmCodegen {
    pub context: LLVMContextRef,
    pub module: LLVMModuleRef,
    pub target_triple: String,
    /// Runtime functions: name -> (function, type).
    runtime: BTreeMap<String, (LLVMValueRef, LLVMTypeRef)>,
    /// Compiled functions by IR name (`main`, `func_<addr>`).
    pub functions: BTreeMap<String, LLVMValueRef>,
    /// The fast entry (arguments in registers, see `codegen/fast.rs`) of
    /// the functions that have one, by IR name: (function, type).
    fast: BTreeMap<String, (LLVMValueRef, LLVMTypeRef)>,
    /// Function bytecode address -> global holding its function object.
    fn_objects: BTreeMap<usize, LLVMValueRef>,
    /// The program's global variables (every name it assigns) -> the LLVM
    /// global holding each one's value (`UNBOUND_BITS` until assigned).
    global_slots: BTreeMap<String, LLVMValueRef>,
    /// Interned string constants: contents -> global.
    strings: HashMap<String, LLVMValueRef>,
    /// The native function type `i64 (i64 this, ptr argv, i64 argc, i64 env)`.
    fn_ty: LLVMTypeRef,
    /// `TraceSite` constants of `IrModule::sites` (exception traces).
    sites: Vec<LLVMValueRef>,
    /// Emit DWARF debug info (`--debug`).
    pub debug_info: bool,
    di: Option<super::debug::DebugBuilder>,
    /// `IrModule::sites`, for debug locations.
    site_info: Vec<crate::ir::Site>,
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
                fast: BTreeMap::new(),
                fn_objects: BTreeMap::new(),
                global_slots: BTreeMap::new(),
                strings: HashMap::new(),
                fn_ty,
                sites: Vec::new(),
                debug_info: false,
                di: None,
                site_info: Vec::new(),
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
                if Self::takes_registers(func) {
                    self.declare_fast(func);
                }

                if let Some(addr) = function_addr(&func.name) {
                    let gname = CString::new(format!("ot_fnobj_{}", addr)).unwrap();
                    let g = LLVMAddGlobal(self.module, i64_ty, gname.as_ptr());
                    LLVMSetInitializer(g, LLVMConstInt(i64_ty, 0, 0));
                    LLVMSetLinkage(g, LLVMLinkage::LLVMInternalLinkage);
                    self.fn_objects.insert(addr, g);
                }
            }

            // A slot for every global the program assigns.
            for func in &ir_module.functions {
                for block in &func.blocks {
                    for op in &block.ops {
                        if let IrOp::StoreGlobal(name, _) = op
                            && !self.global_slots.contains_key(name)
                        {
                            let gname =
                                CString::new(format!("ot_global_{}", self.global_slots.len()))
                                    .unwrap();
                            let g = LLVMAddGlobal(self.module, i64_ty, gname.as_ptr());
                            LLVMSetInitializer(g, LLVMConstInt(i64_ty, UNBOUND_BITS, 0));
                            LLVMSetLinkage(g, LLVMLinkage::LLVMInternalLinkage);
                            self.global_slots.insert(name.clone(), g);
                        }
                    }
                }
            }

            if self.debug_info && !ir_module.sites.is_empty() {
                let main_file = ir_module
                    .sites
                    .iter()
                    .map(|s| s.file.as_str())
                    .find(|f| *f != crate::vm::errors::CORE_ERRORS_PATH)
                    .unwrap_or("main.ot");
                self.di = Some(super::debug::DebugBuilder::new(
                    self.module,
                    self.context,
                    main_file,
                ));
                self.site_info = ir_module.sites.clone();
            }

            // Stack frames for exception traces: `{ ptr, len }` constants.
            let ptr_ty = LLVMPointerTypeInContext(self.context, 0);
            for site in &ir_module.sites {
                let frame = &site.frame;
                let text = LLVMConstStringInContext(
                    self.context,
                    frame.as_ptr() as *const c_char,
                    frame.len() as u32,
                    1,
                );
                let tg = LLVMAddGlobal(self.module, LLVMTypeOf(text), b"site_text\0".as_ptr() as _);
                LLVMSetInitializer(tg, text);
                LLVMSetGlobalConstant(tg, 1);
                LLVMSetLinkage(tg, LLVMLinkage::LLVMPrivateLinkage);
                let mut fields = [tg, LLVMConstInt(i64_ty, frame.len() as u64, 0)];
                let site = LLVMConstStructInContext(self.context, fields.as_mut_ptr(), 2, 0);
                let mut field_tys = [ptr_ty, i64_ty];
                let site_ty = LLVMStructTypeInContext(self.context, field_tys.as_mut_ptr(), 2, 0);
                let g = LLVMAddGlobal(self.module, site_ty, b"site\0".as_ptr() as _);
                LLVMSetInitializer(g, site);
                LLVMSetGlobalConstant(g, 1);
                LLVMSetLinkage(g, LLVMLinkage::LLVMPrivateLinkage);
                self.sites.push(g);
            }

            for func in &ir_module.functions {
                self.compile_function(func)?;
            }

            self.build_c_main()?;
            if let Some(di) = self.di.take() {
                di.finalize();
            }
            self.verify()
        }
    }

    unsafe fn compile_function(&mut self, func: &IrFunction) -> Result<(), BackendError> {
        unsafe {
            // The code goes into the fast entry, if the function has one.
            let fast = self.fast.get(&func.name).map(|(f, _)| *f);
            let f = fast.unwrap_or(self.functions[&func.name]);
            let env_param = match fast {
                Some(_) => func.params.len() as u32 + 1,
                None => 3,
            };
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
                object_slots: crate::ir::alloc_hints::object_slots(func),
                literals: crate::ir::alloc_hints::literal_props(
                    func,
                    crate::runtime::heap::MAX_INLINE_SLOTS,
                ),
                literal_stores: HashMap::new(),
                direct: HashMap::new(),
                this: LLVMGetParam(f, 0),
                env: LLVMGetParam(f, env_param),
                function: f,
                last_call: None,
                exception_return: None,
                // (Not looked at by a function with a fast entry.)
                args: match fast {
                    Some(_) => (
                        LLVMConstNull(LLVMPointerTypeInContext(ctx, 0)),
                        null_i64(ctx),
                    ),
                    None => (LLVMGetParam(f, 1), LLVMGetParam(f, 2)),
                },
                frame: None,
                site: None,
                trace_blocks: HashMap::new(),
                di_scope: None,
                unwind: None,
            };
            // Debug info: the function, and where its code starts.
            let linkage = if func.name == "main" {
                "ot_main".to_string()
            } else {
                func.name.clone()
            };
            if let Some(di) = fx.cg.di.as_mut()
                && let Some(scope) = di.function(func, f, &linkage, &fx.cg.site_info)
            {
                let loc = di.location(&scope, None);
                LLVMSetCurrentDebugLocation2(fx.builder, loc);
                LLVMSetCurrentDebugLocation2(fx.prologue, loc);
                fx.di_scope = Some(scope);
            }
            fx.prologue_setup(func, f, fast.is_some())?;

            for block in &func.blocks {
                LLVMPositionBuilderAtEnd(fx.builder, fx.blocks[&block.id]);
                fx.site = None;
                fx.unwind = block.unwind;
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

            // The prologue falls through to the IR entry block, unless the
            // stack is exhausted: below `OT_STACK_LIMIT`, throw a RangeError.
            if let Some(first) = func.blocks.first() {
                let i64_ty = fx.i64_ty();
                // The address of a slot in this frame stands for the stack
                // pointer (`llvm.frameaddress` would force a frame pointer).
                let probe = LLVMBuildAlloca(fx.prologue, LLVMInt8TypeInContext(ctx), NONAME);
                let sp = LLVMBuildPtrToInt(fx.prologue, probe, i64_ty, NONAME);
                let limit_global = fx.cg.stack_limit_global();
                let limit = LLVMBuildLoad2(fx.prologue, i64_ty, limit_global, NONAME);
                let exhausted =
                    LLVMBuildICmp(fx.prologue, LLVMIntPredicate::LLVMIntULT, sp, limit, NONAME);
                let overflow =
                    LLVMAppendBasicBlockInContext(ctx, f, b"stack_overflow\0".as_ptr() as _);
                LLVMBuildCondBr(fx.prologue, exhausted, overflow, fx.blocks[&first.id]);
                LLVMPositionBuilderAtEnd(fx.prologue, overflow);
                let (so, so_ty) = fx.cg.runtime["ot_stack_overflow"];
                let e = LLVMBuildCall2(fx.prologue, so_ty, so, std::ptr::null_mut(), 0, NONAME);
                LLVMBuildRet(fx.prologue, e);
            } else {
                LLVMBuildRet(fx.prologue, fx.c(UNDEFINED));
            }
            LLVMDisposeBuilder(fx.prologue);
            LLVMDisposeBuilder(fx.builder);
            if fast.is_some() {
                self.build_dynamic_entry(func);
            }
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

fn null_i64(ctx: LLVMContextRef) -> LLVMValueRef {
    unsafe { LLVMConstInt(LLVMInt64TypeInContext(ctx), 0, 0) }
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
    /// How many properties each `NewObject` site is about to store.
    object_slots: HashMap<ValueId, usize>,
    /// The properties each literal site is allocated with
    /// (`alloc_hints::literal_props`), and how many of them have been
    /// stored so far.
    literals: HashMap<ValueId, Vec<String>>,
    literal_stores: HashMap<ValueId, usize>,
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
    /// This function's own `argv` and `argc`.
    args: (LLVMValueRef, LLVMValueRef),
    /// An async function's heap frame (see `ot_async_enter`): as a value
    /// and as a pointer.
    frame: Option<(LLVMValueRef, LLVMValueRef)>,
    /// The site of the ops being compiled (`IrOp::Loc`).
    site: Option<u32>,
    /// Cold blocks that record a site (`ot_trace`), then go to a target.
    trace_blocks: HashMap<(Option<u32>, LLVMBasicBlockRef), LLVMBasicBlockRef>,
    /// This function's debug info, with `--debug`.
    di_scope: Option<super::debug::FunctionScope>,
    /// The unwind target of the block being compiled.
    unwind: Option<BlockId>,
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
            let target = if self.cg.sites.is_empty() {
                target
            } else {
                self.trace_block(target)
            };
            let next = LLVMAppendBasicBlockInContext(ctx, self.function, b"ok\0".as_ptr() as _);
            LLVMBuildCondBr(self.builder, thrown, target, next);
            LLVMPositionBuilderAtEnd(self.builder, next);
        }
    }

    /// A block that records the current site (`ot_trace`), then branches
    /// to `target`.
    unsafe fn trace_block(&mut self, target: LLVMBasicBlockRef) -> LLVMBasicBlockRef {
        unsafe {
            let key = (self.site, target);
            if let Some(&blk) = self.trace_blocks.get(&key) {
                return blk;
            }
            let ctx = self.cg.context;
            let blk = LLVMAppendBasicBlockInContext(ctx, self.function, b"trace\0".as_ptr() as _);
            let b = LLVMCreateBuilderInContext(ctx);
            LLVMPositionBuilderAtEnd(b, blk);
            if self.di_scope.is_some() {
                LLVMSetCurrentDebugLocation2(b, LLVMGetCurrentDebugLocation2(self.builder));
            }
            let site = match self.site {
                Some(s) => self.cg.sites[s as usize],
                None => LLVMConstNull(self.ptr_ty()),
            };
            let (trace, trace_ty) = self.cg.runtime["ot_trace"];
            let mut args = [site];
            LLVMBuildCall2(b, trace_ty, trace, args.as_mut_ptr(), 1, NONAME);
            LLVMBuildBr(b, target);
            LLVMDisposeBuilder(b);
            self.trace_blocks.insert(key, blk);
            blk
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
        fast: bool,
    ) -> Result<(), BackendError> {
        unsafe {
            let b = self.prologue;
            let i64_ty = self.i64_ty();
            let (argv, argc) = self.args;
            if func.is_async {
                // Locals live in a heap frame that survives an `await`.
                let (enter, enter_ty) = self.cg.runtime["ot_async_enter"];
                let slots = self.c(func.locals.len().max(func.params.len()) as u64);
                let mut args = [argv, argc, slots];
                let frame = LLVMBuildCall2(b, enter_ty, enter, args.as_mut_ptr(), 3, NONAME);
                let frame_ptr = LLVMBuildIntToPtr(b, frame, self.ptr_ty(), NONAME);
                self.frame = Some((frame, frame_ptr));
                for i in 0..func.locals.len() {
                    let mut idx = [self.c((crate::runtime::stubs::ASYNC_FRAME_HEADER + i) as u64)];
                    let p = LLVMBuildGEP2(b, i64_ty, frame_ptr, idx.as_mut_ptr(), 1, NONAME);
                    self.locals.push(p);
                }
            } else {
                for _ in &func.locals {
                    let slot = LLVMBuildAlloca(b, i64_ty, b"local\0".as_ptr() as _);
                    LLVMBuildStore(b, self.c(UNDEFINED), slot);
                    self.locals.push(slot);
                }
            }
            let undef_slot = LLVMBuildAlloca(b, i64_ty, b"undef\0".as_ptr() as _);
            LLVMBuildStore(b, self.c(UNDEFINED), undef_slot);
            for i in 0..func.params.len() {
                if fast {
                    self.set(ValueId(i as u32), LLVMGetParam(f, i as u32 + 1));
                    continue;
                }
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
}
