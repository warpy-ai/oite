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

            // Tell the runtime where each global lives, so a lookup by name
            // sees what compiled code stored.
            let (register, register_ty) = self.runtime["ot_register_global"];
            for (name, slot) in &self.global_slots {
                let init = LLVMConstStringInContext(
                    ctx,
                    name.as_ptr() as *const c_char,
                    name.len() as u32,
                    1,
                );
                let text = LLVMAddGlobal(
                    self.module,
                    LLVMTypeOf(init),
                    b"global_name\0".as_ptr() as _,
                );
                LLVMSetInitializer(text, init);
                LLVMSetGlobalConstant(text, 1);
                LLVMSetLinkage(text, LLVMLinkage::LLVMPrivateLinkage);
                let mut args = [text, LLVMConstInt(i64_ty, name.len() as u64, 0), *slot];
                LLVMBuildCall2(b, register_ty, register, args.as_mut_ptr(), 3, NONAME);
            }

            let (init, init_ty) = self.runtime["ot_stack_init"];
            LLVMBuildCall2(b, init_ty, init, std::ptr::null_mut(), 0, NONAME);
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
            // Promise reactions and timers queued by `main`.
            let (event_loop, event_loop_ty) = self.runtime["ot_run_event_loop"];
            LLVMBuildCall2(
                b,
                event_loop_ty,
                event_loop,
                std::ptr::null_mut(),
                0,
                NONAME,
            );
            let (exit, exit_ty) = self.runtime["ot_runtime_exit"];
            LLVMBuildCall2(b, exit_ty, exit, std::ptr::null_mut(), 0, NONAME);
            LLVMBuildRet(b, LLVMConstInt(i32_ty, 0, 0));
            LLVMDisposeBuilder(b);
            Ok(())
        }
    }

    /// The runtime's `OT_STACK_LIMIT`, declared once in the module.
    unsafe fn stack_limit_global(&mut self) -> LLVMValueRef {
        unsafe {
            let name = b"OT_STACK_LIMIT\0";
            let existing = LLVMGetNamedGlobal(self.module, name.as_ptr() as _);
            if !existing.is_null() {
                return existing;
            }
            let i64_ty = LLVMInt64TypeInContext(self.context);
            LLVMAddGlobal(self.module, i64_ty, name.as_ptr() as _)
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
                args: (LLVMGetParam(f, 1), LLVMGetParam(f, 2)),
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
            fx.prologue_setup(func, f)?;

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
    ) -> Result<(), BackendError> {
        unsafe {
            let b = self.prologue;
            let i64_ty = self.i64_ty();
            let argv = LLVMGetParam(f, 1);
            let argc = LLVMGetParam(f, 2);
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

    /// Call the LLVM intrinsic `name`, overloaded on `overload`.
    unsafe fn call_intrinsic(
        &mut self,
        name: &[u8],
        overload: &[LLVMTypeRef],
        ret: LLVMTypeRef,
        args: &mut [LLVMValueRef],
    ) -> LLVMValueRef {
        unsafe {
            let id = LLVMLookupIntrinsicID(name.as_ptr() as _, name.len());
            let mut tys = overload.to_vec();
            let f = LLVMGetIntrinsicDeclaration(self.cg.module, id, tys.as_mut_ptr(), tys.len());
            let mut params: Vec<LLVMTypeRef> = args.iter().map(|a| LLVMTypeOf(*a)).collect();
            let fty = LLVMFunctionType(ret, params.as_mut_ptr(), params.len() as u32, 0);
            LLVMBuildCall2(
                self.builder,
                fty,
                f,
                args.as_mut_ptr(),
                args.len() as u32,
                NONAME,
            )
        }
    }

    /// JavaScript `x % y` on doubles. When both are integers below 2^53 and
    /// `y` is not 0 it is an integer remainder with the sign of `x`
    /// (`copysign` also gives `-0`); otherwise `fmod` (`frem`).
    unsafe fn js_mod(&mut self, x: LLVMValueRef, y: LLVMValueRef) -> LLVMValueRef {
        unsafe {
            let ctx = self.cg.context;
            let i64t = self.i64_ty();
            let f64t = self.f64_ty();
            let limit = 1i64 << 53;
            let exact = |this: &mut Self, v: LLVMValueRef| -> (LLVMValueRef, LLVMValueRef) {
                let vi = this.call_intrinsic(b"llvm.fptosi.sat", &[i64t, f64t], i64t, &mut [v]);
                let back = LLVMBuildSIToFP(this.builder, vi, f64t, NONAME);
                let same = LLVMBuildFCmp(
                    this.builder,
                    LLVMRealPredicate::LLVMRealOEQ,
                    back,
                    v,
                    NONAME,
                );
                let lo = LLVMBuildICmp(
                    this.builder,
                    LLVMIntPredicate::LLVMIntSGT,
                    vi,
                    LLVMConstInt(i64t, (-limit) as u64, 1),
                    NONAME,
                );
                let hi = LLVMBuildICmp(
                    this.builder,
                    LLVMIntPredicate::LLVMIntSLT,
                    vi,
                    LLVMConstInt(i64t, limit as u64, 0),
                    NONAME,
                );
                let ok = LLVMBuildAnd(this.builder, same, lo, NONAME);
                (vi, LLVMBuildAnd(this.builder, ok, hi, NONAME))
            };
            let (xi, xok) = exact(self, x);
            let (yi, yok) = exact(self, y);
            let nonzero = LLVMBuildICmp(
                self.builder,
                LLVMIntPredicate::LLVMIntNE,
                yi,
                LLVMConstInt(i64t, 0, 0),
                NONAME,
            );
            let ints = LLVMBuildAnd(self.builder, xok, yok, NONAME);
            let ints = LLVMBuildAnd(self.builder, ints, nonzero, NONAME);
            let int_bb =
                LLVMAppendBasicBlockInContext(ctx, self.function, b"mod_int\0".as_ptr() as _);
            let flt_bb =
                LLVMAppendBasicBlockInContext(ctx, self.function, b"mod_float\0".as_ptr() as _);
            let join =
                LLVMAppendBasicBlockInContext(ctx, self.function, b"mod_join\0".as_ptr() as _);
            LLVMBuildCondBr(self.builder, ints, int_bb, flt_bb);

            LLVMPositionBuilderAtEnd(self.builder, int_bb);
            let r = LLVMBuildSRem(self.builder, xi, yi, NONAME);
            let rf = LLVMBuildSIToFP(self.builder, r, f64t, NONAME);
            let int_value = self.call_intrinsic(b"llvm.copysign", &[f64t], f64t, &mut [rf, x]);
            LLVMBuildBr(self.builder, join);

            LLVMPositionBuilderAtEnd(self.builder, flt_bb);
            let flt_value = LLVMBuildFRem(self.builder, x, y, NONAME);
            LLVMBuildBr(self.builder, join);

            LLVMPositionBuilderAtEnd(self.builder, join);
            let phi = LLVMBuildPhi(self.builder, f64t, NONAME);
            let mut values = [int_value, flt_value];
            let mut blocks = [int_bb, flt_bb];
            LLVMAddIncoming(phi, values.as_mut_ptr(), blocks.as_mut_ptr(), 2);
            phi
        }
    }

    /// A number's `ToInt32` as an i32, and whether the fast conversion
    /// holds: the number is below 2^63 in magnitude (so its integer part
    /// fits an i64; NaN and the infinities do not pass).
    unsafe fn int32_of(&mut self, f: LLVMValueRef) -> (LLVMValueRef, LLVMValueRef) {
        unsafe {
            let i64t = self.i64_ty();
            let f64t = self.f64_ty();
            let limit = 9_223_372_036_854_775_808.0_f64;
            let i = self.call_intrinsic(b"llvm.fptosi.sat", &[i64t, f64t], i64t, &mut [f]);
            let lo = LLVMBuildFCmp(
                self.builder,
                LLVMRealPredicate::LLVMRealOGT,
                f,
                LLVMConstReal(f64t, -limit),
                NONAME,
            );
            let hi = LLVMBuildFCmp(
                self.builder,
                LLVMRealPredicate::LLVMRealOLT,
                f,
                LLVMConstReal(f64t, limit),
                NONAME,
            );
            let ok = LLVMBuildAnd(self.builder, lo, hi, NONAME);
            let i32t = LLVMInt32TypeInContext(self.cg.context);
            (LLVMBuildTrunc(self.builder, i, i32t, NONAME), ok)
        }
    }

    /// A bitwise operator: inline on 32-bit integers when both operands are
    /// numbers below 2^63, else the runtime (`valueOf` may throw).
    unsafe fn int_binop(
        &mut self,
        dst: ValueId,
        a: ValueId,
        b: ValueId,
        op: IntOp,
        stub: &str,
    ) -> Result<(), BackendError> {
        unsafe {
            let (x, y) = (self.get(a)?, self.get(b)?);
            let (xi, xok) = self.int32_of(self.to_f64(x));
            let (yi, yok) = self.int32_of(self.to_f64(y));
            let mut fast_ok = LLVMBuildAnd(self.builder, xok, yok, NONAME);
            if !(self.is_num(a) && self.is_num(b)) {
                let both = LLVMBuildAnd(
                    self.builder,
                    self.is_number_bits(x),
                    self.is_number_bits(y),
                    NONAME,
                );
                fast_ok = LLVMBuildAnd(self.builder, fast_ok, both, NONAME);
            }
            let ctx = self.cg.context;
            let fast = LLVMAppendBasicBlockInContext(ctx, self.function, b"int\0".as_ptr() as _);
            let slow =
                LLVMAppendBasicBlockInContext(ctx, self.function, b"int_any\0".as_ptr() as _);
            let join =
                LLVMAppendBasicBlockInContext(ctx, self.function, b"int_join\0".as_ptr() as _);
            LLVMBuildCondBr(self.builder, fast_ok, fast, slow);

            LLVMPositionBuilderAtEnd(self.builder, fast);
            let i32t = LLVMInt32TypeInContext(ctx);
            let bld = self.builder;
            let shift = || LLVMBuildAnd(bld, yi, LLVMConstInt(i32t, 31, 0), NONAME);
            let r = match op {
                IntOp::And => LLVMBuildAnd(bld, xi, yi, NONAME),
                IntOp::Or => LLVMBuildOr(bld, xi, yi, NONAME),
                IntOp::Xor => LLVMBuildXor(bld, xi, yi, NONAME),
                IntOp::Shl => LLVMBuildShl(bld, xi, shift(), NONAME),
                IntOp::Shr => LLVMBuildAShr(bld, xi, shift(), NONAME),
                IntOp::ShrU => LLVMBuildLShr(bld, xi, shift(), NONAME),
            };
            let rf = if matches!(op, IntOp::ShrU) {
                LLVMBuildUIToFP(bld, r, self.f64_ty(), NONAME)
            } else {
                LLVMBuildSIToFP(bld, r, self.f64_ty(), NONAME)
            };
            let fast_value = self.from_f64(rf);
            let fast_end = LLVMGetInsertBlock(self.builder);
            LLVMBuildBr(self.builder, join);

            LLVMPositionBuilderAtEnd(self.builder, slow);
            let slow_value = self.rt(stub, &[x, y]);
            self.check_exception(slow_value, self.unwind);
            self.last_call = None;
            let slow_end = LLVMGetInsertBlock(self.builder);
            LLVMBuildBr(self.builder, join);

            LLVMPositionBuilderAtEnd(self.builder, join);
            let phi = LLVMBuildPhi(self.builder, self.i64_ty(), NONAME);
            let mut values = [fast_value, slow_value];
            let mut blocks = [fast_end, slow_end];
            LLVMAddIncoming(phi, values.as_mut_ptr(), blocks.as_mut_ptr(), 2);
            self.set(dst, phi);
            Ok(())
        }
    }

    /// `Math.<f>` computed inline on doubles, for the functions with a
    /// machine instruction: `None` for the others, or a wrong argument count.
    unsafe fn math_inline(&mut self, name: &str, xs: &[LLVMValueRef]) -> Option<LLVMValueRef> {
        unsafe {
            let f64t = self.f64_ty();
            let unary = |this: &mut Self, intrinsic: &[u8]| {
                this.call_intrinsic(intrinsic, &[f64t], f64t, &mut [xs[0]])
            };
            Some(match (name, xs.len()) {
                ("sqrt", 1) => unary(self, b"llvm.sqrt"),
                ("floor", 1) => unary(self, b"llvm.floor"),
                ("ceil", 1) => unary(self, b"llvm.ceil"),
                ("trunc", 1) => unary(self, b"llvm.trunc"),
                ("abs", 1) => unary(self, b"llvm.fabs"),
                // As the runtime: halves round up, towards +Infinity.
                ("round", 1) => {
                    let f = unary(self, b"llvm.floor");
                    let diff = LLVMBuildFSub(self.builder, xs[0], f, NONAME);
                    let up = LLVMBuildFCmp(
                        self.builder,
                        LLVMRealPredicate::LLVMRealOGE,
                        diff,
                        LLVMConstReal(f64t, 0.5),
                        NONAME,
                    );
                    let f1 = LLVMBuildFAdd(self.builder, f, LLVMConstReal(f64t, 1.0), NONAME);
                    LLVMBuildSelect(self.builder, up, f1, f, NONAME)
                }
                // NaN wins and -0 is below +0, as in JavaScript.
                ("min", 2) => {
                    self.call_intrinsic(b"llvm.minimum", &[f64t], f64t, &mut [xs[0], xs[1]])
                }
                ("max", 2) => {
                    self.call_intrinsic(b"llvm.maximum", &[f64t], f64t, &mut [xs[0], xs[1]])
                }
                _ => return None,
            })
        }
    }

    /// `Math.<f>(args)` (`IrOp::MathCall`): inline when every argument is a
    /// number and `f` has an instruction, else `ot_math_call`.
    unsafe fn math_call(
        &mut self,
        func: u32,
        args: &[ValueId],
    ) -> Result<LLVMValueRef, BackendError> {
        unsafe {
            let name = crate::runtime::builtins::MATH_FUNCTIONS
                .get(func as usize)
                .copied()
                .unwrap_or("");
            let vals: Vec<LLVMValueRef> = args
                .iter()
                .map(|a| self.get(*a))
                .collect::<Result<_, _>>()?;
            let call_runtime = |this: &mut Self| {
                let (argv, argc) = this.arg_array(&vals);
                let r = this.rt("ot_math_call", &[this.c(func as u64), argv, argc]);
                this.check_exception(r, this.unwind);
                this.last_call = None;
                r
            };
            let can_inline = matches!(
                (name, vals.len()),
                ("sqrt" | "floor" | "ceil" | "trunc" | "abs" | "round", 1) | ("min" | "max", 2)
            );
            if !can_inline {
                return Ok(call_runtime(self));
            }
            let all_numbers = args.iter().all(|a| self.is_num(*a));
            let floats: Vec<LLVMValueRef> = vals.iter().map(|v| self.to_f64(*v)).collect();
            if all_numbers {
                let r = self
                    .math_inline(name, &floats)
                    .expect("inline Math function");
                return Ok(self.from_f64(r));
            }
            let mut ok = self.is_number_bits(vals[0]);
            for v in &vals[1..] {
                let n = self.is_number_bits(*v);
                ok = LLVMBuildAnd(self.builder, ok, n, NONAME);
            }
            let ctx = self.cg.context;
            let fast = LLVMAppendBasicBlockInContext(ctx, self.function, b"math\0".as_ptr() as _);
            let slow =
                LLVMAppendBasicBlockInContext(ctx, self.function, b"math_any\0".as_ptr() as _);
            let join =
                LLVMAppendBasicBlockInContext(ctx, self.function, b"math_join\0".as_ptr() as _);
            LLVMBuildCondBr(self.builder, ok, fast, slow);
            LLVMPositionBuilderAtEnd(self.builder, fast);
            let r = self
                .math_inline(name, &floats)
                .expect("inline Math function");
            let fast_value = self.from_f64(r);
            let fast_end = LLVMGetInsertBlock(self.builder);
            LLVMBuildBr(self.builder, join);
            LLVMPositionBuilderAtEnd(self.builder, slow);
            let slow_value = call_runtime(self);
            let slow_end = LLVMGetInsertBlock(self.builder);
            LLVMBuildBr(self.builder, join);
            LLVMPositionBuilderAtEnd(self.builder, join);
            let phi = LLVMBuildPhi(self.builder, self.i64_ty(), NONAME);
            let mut values = [fast_value, slow_value];
            let mut blocks = [fast_end, slow_end];
            LLVMAddIncoming(phi, values.as_mut_ptr(), blocks.as_mut_ptr(), 2);
            Ok(phi)
        }
    }

    /// A new inline cache (`runtime::object::ic::PropCache`) for a site
    /// that accesses the property `name`.
    unsafe fn prop_cache(&mut self, name: &str) -> LLVMValueRef {
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

    /// `o.name` (or `o.name = store`) through an inline cache: when `o` is
    /// an object whose shape is the cached one, load or store its slot
    /// here; otherwise call the runtime, which fills the cache.
    unsafe fn cached_prop(
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
            LLVMBuildCondBr(b, same, hit_bb, miss_bb);

            // The slot: `offset >= 0` bytes into the object, or `!offset`
            // bytes into its overflow array.
            LLVMPositionBuilderAtEnd(b, hit_bb);
            let offset = LLVMBuildLoad2(b, i32t, at(cache, ic::OFFSET_OFFSET), NONAME);
            let inline = LLVMBuildICmp(
                b,
                LLVMIntPredicate::LLVMIntSGE,
                offset,
                LLVMConstInt(i32t, 0, 0),
                NONAME,
            );
            let overflow =
                LLVMBuildLoad2(b, ptrt, at(ptr, offset_of!(NativeObject, overflow)), NONAME);
            let base = LLVMBuildSelect(b, inline, ptr, overflow, NONAME);
            let flipped = LLVMBuildNot(b, offset, NONAME);
            let bytes = LLVMBuildSelect(b, inline, offset, flipped, NONAME);
            let bytes = LLVMBuildZExt(b, bytes, i64t, NONAME);
            let mut idx = [bytes];
            let slot = LLVMBuildGEP2(b, i8t, base, idx.as_mut_ptr(), 1, NONAME);
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
            phi
        }
    }

    /// Whether a value (i64) is a number: not one of the boxed tags.
    unsafe fn is_number_bits(&self, v: LLVMValueRef) -> LLVMValueRef {
        unsafe {
            const QNAN: u64 = 0x7FFC_0000_0000_0000;
            let masked = LLVMBuildAnd(self.builder, v, self.c(QNAN), NONAME);
            LLVMBuildICmp(
                self.builder,
                LLVMIntPredicate::LLVMIntNE,
                masked,
                self.c(QNAN),
                NONAME,
            )
        }
    }

    /// `a op b` on values of unknown type: inline floating point when both
    /// turn out to be numbers, else the runtime (which may throw).
    unsafe fn any_arith(
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
        stub: &str,
    ) -> Result<(), BackendError> {
        unsafe {
            if self.is_num(a) && self.is_num(b) && stub != "ot_mod_any" {
                return self.num_binop(dst, a, b, build);
            }
            let (x, y) = (self.get(a)?, self.get(b)?);
            let ctx = self.cg.context;
            let fast = LLVMAppendBasicBlockInContext(ctx, self.function, b"num\0".as_ptr() as _);
            let slow = LLVMAppendBasicBlockInContext(ctx, self.function, b"any\0".as_ptr() as _);
            let join = LLVMAppendBasicBlockInContext(ctx, self.function, b"join\0".as_ptr() as _);
            let both = LLVMBuildAnd(
                self.builder,
                self.is_number_bits(x),
                self.is_number_bits(y),
                NONAME,
            );
            LLVMBuildCondBr(self.builder, both, fast, slow);

            LLVMPositionBuilderAtEnd(self.builder, fast);
            let r = if stub == "ot_mod_any" {
                self.js_mod(self.to_f64(x), self.to_f64(y))
            } else {
                build(self.builder, self.to_f64(x), self.to_f64(y), NONAME)
            };
            let fast_value = self.from_f64(r);
            let fast_end = LLVMGetInsertBlock(self.builder);
            LLVMBuildBr(self.builder, join);

            LLVMPositionBuilderAtEnd(self.builder, slow);
            let slow_value = self.rt(stub, &[x, y]);
            self.check_exception(slow_value, self.unwind);
            self.last_call = None;
            let slow_end = LLVMGetInsertBlock(self.builder);
            LLVMBuildBr(self.builder, join);

            LLVMPositionBuilderAtEnd(self.builder, join);
            let phi = LLVMBuildPhi(self.builder, self.i64_ty(), NONAME);
            let mut values = [fast_value, slow_value];
            let mut blocks = [fast_end, slow_end];
            LLVMAddIncoming(phi, values.as_mut_ptr(), blocks.as_mut_ptr(), 2);
            self.set(dst, phi);
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
                // Inline when both turn out to be numbers.
                let (x, y) = (self.get(a)?, self.get(b)?);
                let ctx = self.cg.context;
                let fast =
                    LLVMAppendBasicBlockInContext(ctx, self.function, b"num\0".as_ptr() as _);
                let slow =
                    LLVMAppendBasicBlockInContext(ctx, self.function, b"any\0".as_ptr() as _);
                let join =
                    LLVMAppendBasicBlockInContext(ctx, self.function, b"join\0".as_ptr() as _);
                let both = LLVMBuildAnd(
                    self.builder,
                    self.is_number_bits(x),
                    self.is_number_bits(y),
                    NONAME,
                );
                LLVMBuildCondBr(self.builder, both, fast, slow);
                LLVMPositionBuilderAtEnd(self.builder, fast);
                let cmp = LLVMBuildFCmp(self.builder, pred, self.to_f64(x), self.to_f64(y), NONAME);
                let fast_value = self.box_bool(cmp);
                let fast_end = LLVMGetInsertBlock(self.builder);
                LLVMBuildBr(self.builder, join);
                LLVMPositionBuilderAtEnd(self.builder, slow);
                let slow_value = self.rt(stub, &[x, y]);
                // `valueOf` may throw.
                self.check_exception(slow_value, self.unwind);
                self.last_call = None;
                let slow_end = LLVMGetInsertBlock(self.builder);
                LLVMBuildBr(self.builder, join);
                LLVMPositionBuilderAtEnd(self.builder, join);
                let phi = LLVMBuildPhi(self.builder, self.i64_ty(), NONAME);
                let mut values = [fast_value, slow_value];
                let mut blocks = [fast_end, slow_end];
                LLVMAddIncoming(phi, values.as_mut_ptr(), blocks.as_mut_ptr(), 2);
                self.set(dst, phi);
                Ok(())
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
                IrOp::ModNum(d, a, b) => {
                    let x = self.to_f64(self.get(*a)?);
                    let y = self.to_f64(self.get(*b)?);
                    let r = self.js_mod(x, y);
                    let r = self.from_f64(r);
                    self.set(*d, r);
                }
                IrOp::NegNum(d, a) => {
                    let x = self.to_f64(self.get(*a)?);
                    let r = LLVMBuildFNeg(self.builder, x, NONAME);
                    let r = self.from_f64(r);
                    self.set(*d, r);
                }

                // Everything else: the runtime.
                IrOp::AddAny(d, a, b) => self.any_arith(*d, *a, *b, LLVMBuildFAdd, "ot_add_any")?,
                IrOp::SubAny(d, a, b) => self.any_arith(*d, *a, *b, LLVMBuildFSub, "ot_sub_any")?,
                IrOp::MulAny(d, a, b) => self.any_arith(*d, *a, *b, LLVMBuildFMul, "ot_mul_any")?,
                IrOp::DivAny(d, a, b) => self.any_arith(*d, *a, *b, LLVMBuildFDiv, "ot_div_any")?,
                IrOp::ModAny(d, a, b) => self.any_arith(*d, *a, *b, LLVMBuildFRem, "ot_mod_any")?,
                IrOp::Pow(d, a, b) => self.rt_binop(*d, "ot_pow", *a, *b)?,
                IrOp::BitAnd(d, a, b) => self.int_binop(*d, *a, *b, IntOp::And, "ot_bit_and")?,
                IrOp::BitOr(d, a, b) => self.int_binop(*d, *a, *b, IntOp::Or, "ot_bit_or")?,
                IrOp::Xor(d, a, b) => self.int_binop(*d, *a, *b, IntOp::Xor, "ot_bit_xor")?,
                IrOp::Shl(d, a, b) => self.int_binop(*d, *a, *b, IntOp::Shl, "ot_shl")?,
                IrOp::Shr(d, a, b) => self.int_binop(*d, *a, *b, IntOp::Shr, "ot_shr")?,
                IrOp::ShrU(d, a, b) => self.int_binop(*d, *a, *b, IntOp::ShrU, "ot_shr_u")?,
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
                IrOp::ToStr(d, a) => {
                    let r = self.rt("ot_to_str", &[self.get(*a)?]);
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
                    let r = match self.cg.global_slots.get(name).copied() {
                        // A global of the program: its slot, unless not yet
                        // assigned (then the runtime: a built-in, or "not
                        // defined").
                        Some(slot) => {
                            let v = LLVMBuildLoad2(self.builder, self.i64_ty(), slot, NONAME);
                            let unbound = LLVMBuildICmp(
                                self.builder,
                                LLVMIntPredicate::LLVMIntEQ,
                                v,
                                self.c(UNBOUND_BITS),
                                NONAME,
                            );
                            let ctx = self.cg.context;
                            let slow = LLVMAppendBasicBlockInContext(
                                ctx,
                                self.function,
                                b"global_unbound\0".as_ptr() as _,
                            );
                            let join = LLVMAppendBasicBlockInContext(
                                ctx,
                                self.function,
                                b"global\0".as_ptr() as _,
                            );
                            let fast_end = LLVMGetInsertBlock(self.builder);
                            LLVMBuildCondBr(self.builder, unbound, slow, join);
                            LLVMPositionBuilderAtEnd(self.builder, slow);
                            let (p, n) = self.string(name);
                            let r = self.rt("ot_get_global", &[p, n]);
                            self.check_exception(r, self.unwind);
                            self.last_call = None;
                            let slow_end = LLVMGetInsertBlock(self.builder);
                            LLVMBuildBr(self.builder, join);
                            LLVMPositionBuilderAtEnd(self.builder, join);
                            let phi = LLVMBuildPhi(self.builder, self.i64_ty(), NONAME);
                            let mut values = [v, r];
                            let mut blocks = [fast_end, slow_end];
                            LLVMAddIncoming(phi, values.as_mut_ptr(), blocks.as_mut_ptr(), 2);
                            phi
                        }
                        None => {
                            let (p, n) = self.string(name);
                            self.rt("ot_get_global", &[p, n])
                        }
                    };
                    self.set(*d, r);
                }
                IrOp::TypeOfGlobal(d, name) => {
                    let (p, n) = self.string(name);
                    let r = self.rt("ot_typeof_global", &[p, n]);
                    self.set(*d, r);
                }
                IrOp::StoreGlobal(name, v) => {
                    let v = self.get(*v)?;
                    match self.cg.global_slots.get(name).copied() {
                        Some(slot) => {
                            LLVMBuildStore(self.builder, v, slot);
                        }
                        None => {
                            let (p, n) = self.string(name);
                            self.rt("ot_set_global", &[p, n, v]);
                        }
                    }
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
                    let r = self.cached_prop(o, name, None);
                    self.set(*d, r);
                }
                IrOp::SetProp(obj, name, v) => {
                    let o = self.get(*obj)?;
                    let v = self.get(*v)?;
                    self.cached_prop(o, name, Some(v));
                }
                IrOp::GetPrivate(d, obj, name) => {
                    let o = self.get(*obj)?;
                    let (p, n) = self.string(name);
                    let r = self.rt("ot_get_private", &[o, p, n]);
                    self.set(*d, r);
                }
                IrOp::SetPrivate(obj, name, v) => {
                    let o = self.get(*obj)?;
                    let v = self.get(*v)?;
                    let (p, n) = self.string(name);
                    self.rt("ot_set_private", &[o, p, n, v]);
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
                IrOp::Construct(d, ctor, args) => {
                    let c = self.get(*ctor)?;
                    let vals: Vec<LLVMValueRef> = args
                        .iter()
                        .map(|a| self.get(*a))
                        .collect::<Result<_, _>>()?;
                    let (argv, argc) = self.arg_array(&vals);
                    let r = self.rt("ot_construct", &[c, argv, argc]);
                    self.set(*d, r);
                }
                IrOp::CallSuper(d, parent, args) => {
                    let p = self.get(*parent)?;
                    let vals: Vec<LLVMValueRef> = args
                        .iter()
                        .map(|a| self.get(*a))
                        .collect::<Result<_, _>>()?;
                    let (argv, argc) = self.arg_array(&vals);
                    let this = self.this;
                    let r = self.rt("ot_call_super", &[p, this, argv, argc]);
                    self.set(*d, r);
                }
                IrOp::SuperForward(d, parent) => {
                    let p = self.get(*parent)?;
                    let (argv, argc) = self.args;
                    let this = self.this;
                    let r = self.rt("ot_call_super", &[p, this, argv, argc]);
                    self.set(*d, r);
                }
                IrOp::CallWithThis(d, func, this, args) => {
                    let f = self.get(*func)?;
                    let t = self.get(*this)?;
                    let vals: Vec<LLVMValueRef> = args
                        .iter()
                        .map(|a| self.get(*a))
                        .collect::<Result<_, _>>()?;
                    let (argv, argc) = self.arg_array(&vals);
                    let r = self.rt("ot_call", &[f, t, argv, argc]);
                    self.set(*d, r);
                }
                IrOp::In(d, k, o) => {
                    let args = [self.get(*k)?, self.get(*o)?];
                    let r = self.rt("ot_in", &args);
                    self.set(*d, r);
                }
                IrOp::InstanceOf(d, v, c) => {
                    let args = [self.get(*v)?, self.get(*c)?];
                    let r = self.rt("ot_instanceof", &args);
                    self.set(*d, r);
                }
                IrOp::GetSuperProp(d, this, base, name) => {
                    let t = self.get(*this)?;
                    let o = self.get(*base)?;
                    let (p, n) = self.string(name);
                    let r = self.rt("ot_get_super_prop", &[t, o, p, n]);
                    self.set(*d, r);
                }
                IrOp::TakeException(d) => {
                    let r = self.rt("ot_take_exception", &[]);
                    self.set(*d, r);
                }
                IrOp::Rethrow(thrown, v) => {
                    let args = [self.get(*thrown)?, self.get(*v)?];
                    self.rt("ot_rethrow", &args);
                }
                IrOp::Loc(n) => {
                    self.site = (*n != crate::ir::lower::NO_SITE).then_some(*n);
                    if let (Some(scope), Some(di)) = (&self.di_scope, self.cg.di.as_mut()) {
                        let site = self.site.and_then(|s| self.cg.site_info.get(s as usize));
                        let loc = di.location(scope, site);
                        LLVMSetCurrentDebugLocation2(self.builder, loc);
                    }
                }
                IrOp::RestArgs(d, n) => {
                    let (argv, argc) = self.args;
                    let r = self.rt("ot_rest_args", &[argv, argc, self.c(*n as u64)]);
                    self.set(*d, r);
                }
                IrOp::AsyncStart(d) => {
                    let r = self.rt("ot_async_start", &[]);
                    self.set(*d, r);
                }
                IrOp::AsyncResolve(d, p, v)
                | IrOp::AsyncReject(d, p, v)
                | IrOp::ArraySpread(d, p, v)
                | IrOp::ObjectSpread(d, p, v) => {
                    let name = match op {
                        IrOp::AsyncResolve(..) => "ot_async_resolve",
                        IrOp::AsyncReject(..) => "ot_async_reject",
                        IrOp::ArraySpread(..) => "ot_array_spread",
                        _ => "ot_object_spread",
                    };
                    let args = [self.get(*p)?, self.get(*v)?];
                    let r = self.rt(name, &args);
                    self.set(*d, r);
                }
                IrOp::IsResume(d) => {
                    let resumed = LLVMBuildICmp(
                        self.builder,
                        LLVMIntPredicate::LLVMIntEQ,
                        self.args.1,
                        self.c(crate::runtime::stubs::ASYNC_RESUME_ARGC as u64),
                        NONAME,
                    );
                    let r = self.box_bool(resumed);
                    self.set(*d, r);
                }
                IrOp::ResumePoint(d) => {
                    let (_, frame) = self.frame.unwrap();
                    let r = LLVMBuildLoad2(self.builder, self.i64_ty(), frame, NONAME);
                    self.set(*d, r);
                }
                IrOp::Await(v, k) => {
                    let (frame, frame_ptr) = self.frame.unwrap();
                    LLVMBuildStore(self.builder, self.c((*k as f64).to_bits()), frame_ptr);
                    let code =
                        LLVMBuildPtrToInt(self.builder, self.function, self.i64_ty(), NONAME);
                    let args = [self.get(*v)?, frame, code, self.this, self.env];
                    self.rt("ot_await", &args);
                }
                IrOp::AwaitResult(d) => {
                    let (frame, _) = self.frame.unwrap();
                    let r = self.rt("ot_await_result", &[frame]);
                    self.set(*d, r);
                }
                IrOp::MathCall(d, func, args) => {
                    let r = self.math_call(*func, args)?;
                    self.set(*d, r);
                }
                IrOp::Intrinsic(d, id, args) => {
                    let vals: Vec<LLVMValueRef> = args
                        .iter()
                        .map(|a| self.get(*a))
                        .collect::<Result<_, _>>()?;
                    let (argv, argc) = self.arg_array(&vals);
                    let v = self.rt("ot_intrinsic", &[self.c(*id as u64), argv, argc]);
                    self.set(*d, v);
                }
                IrOp::CallMethod(d, recv, name, args) => {
                    let r = self.get(*recv)?;
                    let vals: Vec<LLVMValueRef> = args
                        .iter()
                        .map(|a| self.get(*a))
                        .collect::<Result<_, _>>()?;
                    let (argv, argc) = self.arg_array(&vals);
                    let cache = self.prop_cache(name);
                    let v = self.rt("ot_call_method_ic", &[r, cache, argv, argc]);
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
