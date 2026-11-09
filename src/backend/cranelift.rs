//! Cranelift JIT code generation.
//!
//! Mirrors the LLVM backend (`llvm/codegen.rs`): the same calling convention
//! (`i64 (i64 this, ptr argv, i64 argc, i64 env)`, see `runtime::heap::NativeFn`), the
//! same inlined number arithmetic, and the same runtime calls for everything
//! else. The runtime is linked in-process: the JIT resolves `ot_*` symbols to
//! the functions in `crate::runtime::stubs`.

use cranelift::prelude::*;
use cranelift_codegen::ir::{FuncRef, StackSlot};
use cranelift_codegen::settings;
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{DataDescription, DataId, FuncId, Linkage, Module};
use std::collections::HashMap;

use super::runtime_abi::RUNTIME;
use super::{BackendConfig, BackendError};
use crate::ir::{BlockId, IrFunction, IrModule, IrOp, IrType, Literal, Terminator, ValueId};
use crate::runtime::abi::{EXCEPTION_BITS, UNBOUND_BITS};

const QNAN: u64 = 0x7FFC_0000_0000_0000;
const TAG_BOOLEAN: u64 = 0x0001_0000_0000_0000;
const TAG_NULL: u64 = 0x0002_0000_0000_0000;
const TAG_UNDEFINED: u64 = 0x0003_0000_0000_0000;
const UNDEFINED: u64 = QNAN | TAG_UNDEFINED;
const FALSE: u64 = QNAN | TAG_BOOLEAN;
const TRUE: u64 = QNAN | TAG_BOOLEAN | 1;

/// The address of a runtime function, for the JIT's symbol table.
fn runtime_symbol(name: &str) -> Option<*const u8> {
    use crate::runtime::stubs::*;
    macro_rules! syms {
        ($($f:ident),* $(,)?) => {
            match name {
                $(stringify!($f) => Some($f as *const u8),)*
                _ => None,
            }
        };
    }
    syms!(
        ot_alloc_object,
        ot_alloc_array,
        ot_alloc_string,
        ot_make_function,
        ot_make_closure,
        ot_get_prop,
        ot_set_prop,
        ot_get_index,
        ot_set_index,
        ot_delete_prop,
        ot_array_len,
        ot_array_push,
        ot_get_global,
        ot_typeof_global,
        ot_set_global,
        ot_call,
        ot_call_method,
        ot_intrinsic,
        ot_register_global,
        ot_math_call,
        ot_array_spread,
        ot_object_spread,
        ot_add_any,
        ot_sub_any,
        ot_mul_any,
        ot_div_any,
        ot_mod_any,
        ot_pow,
        ot_neg,
        ot_bit_and,
        ot_bit_or,
        ot_bit_xor,
        ot_shl,
        ot_shr,
        ot_shr_u,
        ot_eq_strict,
        ot_eq_loose,
        ot_lt,
        ot_gt,
        ot_lte,
        ot_gte,
        ot_not,
        ot_truthy,
        ot_typeof,
        ot_to_boolean,
        ot_to_number,
        ot_runtime_exit,
        ot_throw,
        ot_take_exception,
        ot_rethrow,
        ot_report_uncaught,
        ot_construct,
        ot_call_super,
        ot_get_super_prop,
        ot_instanceof,
        ot_get_private,
        ot_set_private,
        ot_to_str,
        ot_in,
        ot_stack_init,
        ot_stack_overflow,
        ot_async_start,
        ot_async_resolve,
        ot_async_reject,
        ot_async_enter,
        ot_await,
        ot_await_result,
        ot_run_microtasks,
        ot_run_event_loop,
        ot_trace,
        ot_rest_args,
    )
}

/// Cranelift code generator
pub struct CraneliftCodegen {
    module: JITModule,
    ctx: codegen::Context,
    builder_ctx: FunctionBuilderContext,
    /// Runtime functions: name -> (id, returns a value?)
    runtime: HashMap<String, (FuncId, bool)>,
    /// Compiled function pointers by IR name.
    compiled_funcs: HashMap<String, *const u8>,
}

impl CraneliftCodegen {
    pub fn new(_config: &BackendConfig) -> Result<Self, BackendError> {
        let mut flag_builder = settings::builder();
        // Disable PLT on aarch64 since it's not supported in cranelift-jit 0.113
        flag_builder.set("use_colocated_libcalls", "true").unwrap();
        flag_builder.set("is_pic", "false").unwrap();
        flag_builder.set("opt_level", "speed").unwrap();

        let isa_builder = cranelift_native::builder()
            .map_err(|e| BackendError::Cranelift(format!("Failed to create ISA builder: {}", e)))?;
        let isa = isa_builder
            .finish(settings::Flags::new(flag_builder))
            .map_err(|e| BackendError::Cranelift(format!("Failed to create ISA: {}", e)))?;

        let mut builder = JITBuilder::with_isa(isa, cranelift_module::default_libcall_names());
        for (name, _, _) in RUNTIME {
            let ptr = runtime_symbol(name).expect("runtime function missing a JIT symbol");
            builder.symbol(*name, ptr);
        }
        let mut module = JITModule::new(builder);

        let mut runtime = HashMap::new();
        for (name, returns, params) in RUNTIME {
            let mut sig = module.make_signature();
            for _ in params.iter() {
                sig.params.push(AbiParam::new(types::I64));
            }
            if *returns {
                sig.returns.push(AbiParam::new(types::I64));
            }
            let id = module
                .declare_function(name, Linkage::Import, &sig)
                .map_err(|e| BackendError::Cranelift(format!("declare {}: {}", name, e)))?;
            runtime.insert(name.to_string(), (id, *returns));
        }

        let ctx = module.make_context();
        Ok(Self {
            module,
            ctx,
            builder_ctx: FunctionBuilderContext::new(),
            runtime,
            compiled_funcs: HashMap::new(),
        })
    }

    fn native_sig(&self) -> Signature {
        let mut sig = self.module.make_signature();
        for _ in 0..4 {
            sig.params.push(AbiParam::new(types::I64));
        }
        sig.returns.push(AbiParam::new(types::I64));
        sig
    }

    /// Compile an entire IR module. Afterwards every function object has
    /// been created, so `main` can be called.
    pub fn compile_module(&mut self, ir_module: &IrModule) -> Result<(), BackendError> {
        let sig = self.native_sig();
        let mut func_ids: HashMap<String, FuncId> = HashMap::new();
        let mut fn_objects: HashMap<usize, DataId> = HashMap::new();

        for func in &ir_module.functions {
            let id = self
                .module
                .declare_function(&func.name, Linkage::Local, &sig)
                .map_err(|e| BackendError::Cranelift(format!("declare {}: {}", func.name, e)))?;
            func_ids.insert(func.name.clone(), id);
            if let Some(addr) = function_addr(&func.name) {
                let data = self
                    .module
                    .declare_data(&format!("ot_fnobj_{}", addr), Linkage::Local, true, false)
                    .map_err(|e| BackendError::Cranelift(format!("declare data: {}", e)))?;
                let mut desc = DataDescription::new();
                desc.define_zeroinit(8);
                self.module
                    .define_data(data, &desc)
                    .map_err(|e| BackendError::Cranelift(format!("define data: {}", e)))?;
                fn_objects.insert(addr, data);
            }
        }

        // A slot for every global the program assigns (`UNBOUND_BITS` until
        // assigned); compiled code reads and writes it directly.
        let mut global_slots: HashMap<String, DataId> = HashMap::new();
        for func in &ir_module.functions {
            for block in &func.blocks {
                for op in &block.ops {
                    if let IrOp::StoreGlobal(name, _) = op
                        && !global_slots.contains_key(name)
                    {
                        let data = self
                            .module
                            .declare_data(
                                &format!("ot_global_{}", global_slots.len()),
                                Linkage::Local,
                                true,
                                false,
                            )
                            .map_err(|e| BackendError::Cranelift(format!("declare data: {}", e)))?;
                        let mut desc = DataDescription::new();
                        desc.define(Box::new(UNBOUND_BITS.to_le_bytes()));
                        self.module
                            .define_data(data, &desc)
                            .map_err(|e| BackendError::Cranelift(format!("define data: {}", e)))?;
                        global_slots.insert(name.clone(), data);
                    }
                }
            }
        }

        // Stack frames for exception traces, alive as long as the code.
        let sites: Vec<u64> = ir_module
            .sites
            .iter()
            .map(|site| {
                let text: &'static str = Box::leak(site.frame.clone().into_boxed_str());
                let site = Box::leak(Box::new(crate::runtime::stubs::TraceSite {
                    ptr: text.as_ptr(),
                    len: text.len(),
                }));
                site as *const _ as u64
            })
            .collect();
        let tracing = !sites.is_empty();

        for func in &ir_module.functions {
            self.ctx.clear();
            self.ctx.func.signature = sig.clone();
            {
                let mut builder = FunctionBuilder::new(&mut self.ctx.func, &mut self.builder_ctx);
                let mut fx = Fx {
                    module: &mut self.module,
                    runtime: &self.runtime,
                    func_ids: &func_ids,
                    fn_objects: &fn_objects,
                    global_slots: &global_slots,
                    stubs: HashMap::new(),
                    values: HashMap::new(),
                    blocks: HashMap::new(),
                    locals: Vec::new(),
                    types: &func.value_types,
                    direct: HashMap::new(),
                    env: None,
                    this: None,
                    last_call: None,
                    unwind: None,
                    exception_return: None,
                    args: None,
                    frame: None,
                    own_name: &func.name,
                    sites: &sites,
                    tracing,
                    site: None,
                    trace_blocks: HashMap::new(),
                };
                fx.function(&mut builder, func)?;
                builder.seal_all_blocks();
                builder.finalize();
            }
            self.module
                .define_function(func_ids[&func.name], &mut self.ctx)
                .map_err(|e| {
                    BackendError::Cranelift(format!(
                        "Failed to compile function {}: {:?}",
                        func.name, e
                    ))
                })?;
        }

        self.module
            .finalize_definitions()
            .map_err(|e| BackendError::Cranelift(format!("Failed to finalize: {}", e)))?;

        for (name, id) in &func_ids {
            let ptr = self.module.get_finalized_function(*id);
            self.compiled_funcs.insert(name.clone(), ptr);
        }

        // Tell the runtime where each global lives, so a lookup by name sees
        // what compiled code stored.
        for (name, data) in &global_slots {
            let (slot, _) = self.module.get_finalized_data(*data);
            crate::runtime::stubs::ot_register_global(name.as_ptr(), name.len(), slot as *mut u64);
        }

        // Create the function objects that `Literal::Function` loads.
        for (addr, data) in &fn_objects {
            let code = self.compiled_funcs[&format!("func_{}", addr)];
            let (slot, _) = self.module.get_finalized_data(*data);
            let obj = crate::runtime::stubs::ot_make_function(code);
            unsafe { (slot as *mut u64).write(obj) };
        }

        Ok(())
    }

    /// Get a compiled function by name
    pub fn get_func(&self, name: &str) -> Option<*const u8> {
        self.compiled_funcs.get(name).copied()
    }

    /// Get all compiled functions
    pub fn get_all_funcs(&self) -> HashMap<String, *const u8> {
        self.compiled_funcs.clone()
    }
}

/// `func_123` -> 123
fn function_addr(name: &str) -> Option<usize> {
    name.strip_prefix("func_")?.parse().ok()
}

/// Per-function translation state.
struct Fx<'a> {
    module: &'a mut JITModule,
    runtime: &'a HashMap<String, (FuncId, bool)>,
    func_ids: &'a HashMap<String, FuncId>,
    fn_objects: &'a HashMap<usize, DataId>,
    /// The program's global variables -> their slots.
    global_slots: &'a HashMap<String, DataId>,
    stubs: HashMap<String, FuncRef>,
    values: HashMap<ValueId, Value>,
    blocks: HashMap<BlockId, Block>,
    locals: Vec<Local>,
    types: &'a HashMap<ValueId, IrType>,
    /// Values known to be a specific compiled function, with the closure
    /// environment they carry, if any.
    direct: HashMap<ValueId, (usize, Option<ValueId>)>,
    this: Option<Value>,
    /// This function's own closure environment.
    env: Option<Value>,
    /// Result of the last call emitted: what an op that can throw is
    /// checked against `EXCEPTION_BITS`.
    last_call: Option<Value>,
    /// The unwind target of the block being compiled.
    unwind: Option<BlockId>,
    /// Block returning `EXCEPTION_BITS`, for exceptions no `catch` in this
    /// function handles.
    exception_return: Option<Block>,
    /// This function's own `argv` and `argc`.
    args: Option<(Value, Value)>,
    /// An async function's heap frame (see `ot_async_enter`).
    frame: Option<Value>,
    /// This function's IR name (an `await` resumes it).
    own_name: &'a str,
    /// `TraceSite` addresses of `IrModule::sites`.
    sites: &'a [u64],
    /// Whether exceptions record stack frames (the module has sites).
    tracing: bool,
    /// The site of the ops being compiled (`IrOp::Loc`).
    site: Option<u32>,
    /// Cold blocks that record a site, then go to a target.
    trace_blocks: HashMap<(Option<u32>, Block), Block>,
}

/// Where a local lives: a stack slot, or a word of an async function's
/// heap frame (byte offset).
#[derive(Clone, Copy)]
enum Local {
    Slot(StackSlot),
    Frame(i32),
}

/// Names passed to the runtime live for the whole process.
fn leak_str(s: &str) -> (i64, i64) {
    let leaked: &'static str = Box::leak(s.to_string().into_boxed_str());
    (leaked.as_ptr() as i64, leaked.len() as i64)
}

impl Fx<'_> {
    fn get(&self, id: ValueId) -> Result<Value, BackendError> {
        self.values.get(&id).copied().ok_or_else(|| {
            BackendError::Cranelift(format!(
                "internal error: value {:?} used before definition",
                id
            ))
        })
    }

    fn is_num(&self, id: ValueId) -> bool {
        matches!(self.types.get(&id), Some(IrType::Number))
    }

    fn is_bool(&self, id: ValueId) -> bool {
        matches!(self.types.get(&id), Some(IrType::Boolean))
    }

    fn c(b: &mut FunctionBuilder, bits: u64) -> Value {
        b.ins().iconst(types::I64, bits as i64)
    }

    /// Call a runtime function; `None` result for the void ones.
    fn rt(&mut self, b: &mut FunctionBuilder, name: &str, args: &[Value]) -> Option<Value> {
        let (id, returns) = self.runtime[name];
        let fref = *self
            .stubs
            .entry(name.to_string())
            .or_insert_with(|| self.module.declare_func_in_func(id, b.func));
        let call = b.ins().call(fref, args);
        if returns {
            let r = b.inst_results(call)[0];
            self.last_call = Some(r);
            Some(r)
        } else {
            None
        }
    }

    /// After an op that can throw: if its result is `EXCEPTION_BITS`, go
    /// to the block's unwind target, or return the exception to the caller.
    fn check_exception(&mut self, b: &mut FunctionBuilder, result: Value, unwind: Option<BlockId>) {
        let thrown = b
            .ins()
            .icmp_imm(IntCC::Equal, result, EXCEPTION_BITS as i64);
        let target = match unwind {
            Some(u) => self.blocks[&u],
            None => match self.exception_return {
                Some(blk) => blk,
                None => {
                    let blk = b.create_block();
                    b.set_cold_block(blk);
                    self.exception_return = Some(blk);
                    blk
                }
            },
        };
        let target = if self.tracing {
            self.trace_block(b, target)
        } else {
            target
        };
        let next = b.create_block();
        b.ins().brif(thrown, target, &[], next, &[]);
        b.switch_to_block(next);
    }

    /// A cold block that records the current site (`ot_trace`), then jumps
    /// to `target`.
    fn trace_block(&mut self, b: &mut FunctionBuilder, target: Block) -> Block {
        let key = (self.site, target);
        if let Some(&blk) = self.trace_blocks.get(&key) {
            return blk;
        }
        let current = b.current_block().expect("in a block");
        let blk = b.create_block();
        b.set_cold_block(blk);
        b.switch_to_block(blk);
        let site = self.site.map_or(0, |s| self.sites[s as usize]);
        let site = Self::c(b, site);
        self.rt(b, "ot_trace", &[site]);
        b.ins().jump(target, &[]);
        b.switch_to_block(current);
        self.trace_blocks.insert(key, blk);
        blk
    }

    fn rt_v(&mut self, b: &mut FunctionBuilder, name: &str, args: &[Value]) -> Value {
        self.rt(b, name, args)
            .expect("runtime function has no result")
    }

    fn name(&mut self, b: &mut FunctionBuilder, s: &str) -> (Value, Value) {
        let (p, n) = leak_str(s);
        (b.ins().iconst(types::I64, p), b.ins().iconst(types::I64, n))
    }

    fn arg_array(&mut self, b: &mut FunctionBuilder, vals: &[Value]) -> (Value, Value) {
        if vals.is_empty() {
            return (Self::c(b, 0), Self::c(b, 0));
        }
        let slot = b.create_sized_stack_slot(StackSlotData::new(
            StackSlotKind::ExplicitSlot,
            8 * vals.len() as u32,
            3,
        ));
        for (i, v) in vals.iter().enumerate() {
            b.ins().stack_store(*v, slot, 8 * i as i32);
        }
        let addr = b.ins().stack_addr(types::I64, slot, 0);
        (addr, Self::c(b, vals.len() as u64))
    }

    fn to_f64(b: &mut FunctionBuilder, v: Value) -> Value {
        b.ins().bitcast(types::F64, MemFlags::new(), v)
    }

    fn from_f64(b: &mut FunctionBuilder, v: Value) -> Value {
        b.ins().bitcast(types::I64, MemFlags::new(), v)
    }

    /// i8 condition -> boxed boolean
    fn box_bool(b: &mut FunctionBuilder, cond: Value) -> Value {
        let ext = b.ins().uextend(types::I64, cond);
        b.ins().bor_imm(ext, FALSE as i64)
    }

    /// Truthiness as an i8 condition.
    fn truthy(&mut self, b: &mut FunctionBuilder, id: ValueId) -> Result<Value, BackendError> {
        let v = self.get(id)?;
        if self.is_bool(id) {
            return Ok(b.ins().icmp_imm(IntCC::Equal, v, TRUE as i64));
        }
        let t = self.rt_v(b, "ot_truthy", &[v]);
        Ok(b.ins().icmp_imm(IntCC::NotEqual, t, 0))
    }

    fn function(&mut self, b: &mut FunctionBuilder, func: &IrFunction) -> Result<(), BackendError> {
        let prologue = b.create_block();
        b.append_block_params_for_function_params(prologue);
        for block in &func.blocks {
            let blk = b.create_block();
            self.blocks.insert(block.id, blk);
        }

        b.switch_to_block(prologue);
        let params = b.block_params(prologue).to_vec();
        let (this, argv, argc) = (params[0], params[1], params[2]);
        self.this = Some(this);
        self.env = Some(params[3]);
        self.args = Some((argv, argc));

        if func.is_async {
            // Locals live in a heap frame that survives an `await`.
            let slots = Self::c(b, func.locals.len().max(func.params.len()) as u64);
            let frame = self.rt_v(b, "ot_async_enter", &[argv, argc, slots]);
            self.frame = Some(frame);
            for i in 0..func.locals.len() {
                let offset = 8 * (crate::runtime::stubs::ASYNC_FRAME_HEADER + i) as i32;
                self.locals.push(Local::Frame(offset));
            }
        } else {
            for _ in &func.locals {
                let slot = b.create_sized_stack_slot(StackSlotData::new(
                    StackSlotKind::ExplicitSlot,
                    8,
                    3,
                ));
                let u = Self::c(b, UNDEFINED);
                b.ins().stack_store(u, slot, 0);
                self.locals.push(Local::Slot(slot));
            }
        }
        let undef_slot =
            b.create_sized_stack_slot(StackSlotData::new(StackSlotKind::ExplicitSlot, 8, 3));
        let u = Self::c(b, UNDEFINED);
        b.ins().stack_store(u, undef_slot, 0);
        let undef_addr = b.ins().stack_addr(types::I64, undef_slot, 0);
        for i in 0..func.params.len() {
            // argv[i] if i < argc, else undefined (branch-free).
            let present = b.ins().icmp_imm(IntCC::UnsignedGreaterThan, argc, i as i64);
            let p = b.ins().iadd_imm(argv, 8 * i as i64);
            let src = b.ins().select(present, p, undef_addr);
            let v = b.ins().load(types::I64, MemFlags::trusted(), src, 0);
            self.values.insert(ValueId(i as u32), v);
        }

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

        // Stack overflow: below `OT_STACK_LIMIT`, throw a RangeError.
        let limit_addr = Self::c(b, &crate::runtime::stubs::OT_STACK_LIMIT as *const _ as u64);
        let limit = b.ins().load(types::I64, MemFlags::trusted(), limit_addr, 0);
        let sp = b.ins().get_stack_pointer(types::I64);
        let exhausted = b.ins().icmp(IntCC::UnsignedLessThan, sp, limit);
        let overflow = b.create_block();
        b.set_cold_block(overflow);
        let body = b.create_block();
        b.ins().brif(exhausted, overflow, &[], body, &[]);
        b.switch_to_block(overflow);
        let e = self.rt_v(b, "ot_stack_overflow", &[]);
        b.ins().return_(&[e]);
        b.switch_to_block(body);

        match func.blocks.first() {
            Some(first) => {
                b.ins().jump(self.blocks[&first.id], &[]);
            }
            None => {
                let u = Self::c(b, UNDEFINED);
                b.ins().return_(&[u]);
            }
        }

        for block in &func.blocks {
            b.switch_to_block(self.blocks[&block.id]);
            self.site = None;
            self.unwind = block.unwind;
            for op in &block.ops {
                self.last_call = None;
                self.op(b, op)?;
                if op.can_throw()
                    && let Some(r) = self.last_call
                {
                    self.check_exception(b, r, block.unwind);
                }
            }
            self.terminator(b, &block.terminator)?;
        }
        if let Some(blk) = self.exception_return {
            b.switch_to_block(blk);
            let e = Self::c(b, EXCEPTION_BITS);
            b.ins().return_(&[e]);
        }
        Ok(())
    }

    fn literal(&mut self, b: &mut FunctionBuilder, lit: &Literal) -> Result<Value, BackendError> {
        Ok(match lit {
            Literal::Number(n) => Self::c(b, n.to_bits()),
            Literal::Boolean(v) => Self::c(b, if *v { TRUE } else { FALSE }),
            Literal::Null => Self::c(b, QNAN | TAG_NULL),
            Literal::Undefined => Self::c(b, UNDEFINED),
            Literal::String(s) => {
                let (p, n) = self.name(b, s);
                self.rt_v(b, "ot_alloc_string", &[p, n])
            }
            Literal::Function(addr) => {
                let data = *self.fn_objects.get(addr).ok_or_else(|| {
                    BackendError::Cranelift(format!("internal error: no function at {}", addr))
                })?;
                let gv = self.module.declare_data_in_func(data, b.func);
                let p = b.ins().global_value(types::I64, gv);
                b.ins().load(types::I64, MemFlags::trusted(), p, 0)
            }
        })
    }

    fn num_binop(
        &mut self,
        b: &mut FunctionBuilder,
        dst: ValueId,
        x: ValueId,
        y: ValueId,
        f: fn(&mut FunctionBuilder, Value, Value) -> Value,
    ) -> Result<(), BackendError> {
        let xv = self.get(x)?;
        let yv = self.get(y)?;
        let xf = Self::to_f64(b, xv);
        let yf = Self::to_f64(b, yv);
        let r = f(b, xf, yf);
        let r = Self::from_f64(b, r);
        self.values.insert(dst, r);
        Ok(())
    }

    fn rt_binop(
        &mut self,
        b: &mut FunctionBuilder,
        dst: ValueId,
        name: &str,
        x: ValueId,
        y: ValueId,
    ) -> Result<(), BackendError> {
        let args = [self.get(x)?, self.get(y)?];
        let r = self.rt_v(b, name, &args);
        self.values.insert(dst, r);
        Ok(())
    }

    fn rt_unop(
        &mut self,
        b: &mut FunctionBuilder,
        dst: ValueId,
        name: &str,
        x: ValueId,
    ) -> Result<(), BackendError> {
        let args = [self.get(x)?];
        let r = self.rt_v(b, name, &args);
        self.values.insert(dst, r);
        Ok(())
    }

    /// Whether both values are numbers: neither has the boxed tag pattern.
    fn both_numbers(b: &mut FunctionBuilder, x: Value, y: Value) -> Value {
        const QNAN: i64 = 0x7FFC_0000_0000_0000;
        let mx = b.ins().band_imm(x, QNAN);
        let my = b.ins().band_imm(y, QNAN);
        let nx = b.ins().icmp_imm(IntCC::NotEqual, mx, QNAN);
        let ny = b.ins().icmp_imm(IntCC::NotEqual, my, QNAN);
        b.ins().band(nx, ny)
    }

    /// `x op y` on values of unknown type: `fast` inline when both turn out
    /// to be numbers, else the runtime function `stub` (checked for an
    /// exception when `throws`).
    #[allow(clippy::too_many_arguments)]
    fn any_binop(
        &mut self,
        b: &mut FunctionBuilder,
        dst: ValueId,
        x: ValueId,
        y: ValueId,
        stub: &str,
        throws: bool,
        fast: &dyn Fn(&mut FunctionBuilder, Value, Value) -> Value,
    ) -> Result<(), BackendError> {
        let xv = self.get(x)?;
        let yv = self.get(y)?;
        let num = b.create_block();
        let any = b.create_block();
        let join = b.create_block();
        b.append_block_param(join, types::I64);
        let both = Self::both_numbers(b, xv, yv);
        b.ins().brif(both, num, &[], any, &[]);

        b.switch_to_block(num);
        b.seal_block(num);
        let r = fast(b, xv, yv);
        b.ins().jump(join, &[r]);

        b.switch_to_block(any);
        b.seal_block(any);
        let r = self.rt_v(b, stub, &[xv, yv]);
        if throws {
            let unwind = self.unwind;
            self.check_exception(b, r, unwind);
        }
        self.last_call = None;
        b.ins().jump(join, &[r]);

        b.switch_to_block(join);
        b.seal_block(join);
        let r = b.block_params(join)[0];
        self.values.insert(dst, r);
        Ok(())
    }

    /// Arithmetic on values of unknown type: floating point inline when both
    /// are numbers.
    fn any_arith(
        &mut self,
        b: &mut FunctionBuilder,
        dst: ValueId,
        x: ValueId,
        y: ValueId,
        stub: &str,
        f: fn(&mut FunctionBuilder, Value, Value) -> Value,
    ) -> Result<(), BackendError> {
        if self.is_num(x) && self.is_num(y) {
            return self.num_binop(b, dst, x, y, f);
        }
        self.any_binop(b, dst, x, y, stub, true, &|b, p, q| {
            let pf = Self::to_f64(b, p);
            let qf = Self::to_f64(b, q);
            let r = f(b, pf, qf);
            Self::from_f64(b, r)
        })
    }

    /// `Math.<f>` inline on doubles, for the functions with an instruction.
    fn math_inline(b: &mut FunctionBuilder, name: &str, xs: &[Value]) -> Option<Value> {
        Some(match (name, xs.len()) {
            ("sqrt", 1) => b.ins().sqrt(xs[0]),
            ("floor", 1) => b.ins().floor(xs[0]),
            ("ceil", 1) => b.ins().ceil(xs[0]),
            ("trunc", 1) => b.ins().trunc(xs[0]),
            ("abs", 1) => b.ins().fabs(xs[0]),
            // As the runtime: halves round up, towards +Infinity.
            ("round", 1) => {
                let f = b.ins().floor(xs[0]);
                let diff = b.ins().fsub(xs[0], f);
                let half = b.ins().f64const(0.5);
                let up = b.ins().fcmp(FloatCC::GreaterThanOrEqual, diff, half);
                let one = b.ins().f64const(1.0);
                let f1 = b.ins().fadd(f, one);
                b.ins().select(up, f1, f)
            }
            // NaN wins and -0 is below +0, as in JavaScript.
            ("min", 2) => b.ins().fmin(xs[0], xs[1]),
            ("max", 2) => b.ins().fmax(xs[0], xs[1]),
            _ => return None,
        })
    }

    /// `Math.<f>(args)` (`IrOp::MathCall`): inline when every argument is a
    /// number and `f` has an instruction, else `ot_math_call`.
    fn math_call(
        &mut self,
        b: &mut FunctionBuilder,
        func: u32,
        args: &[ValueId],
    ) -> Result<Value, BackendError> {
        let name = crate::runtime::builtins::MATH_FUNCTIONS
            .get(func as usize)
            .copied()
            .unwrap_or("");
        let vals: Vec<Value> = args
            .iter()
            .map(|a| self.get(*a))
            .collect::<Result<_, _>>()?;
        let call_runtime = |this: &mut Self, b: &mut FunctionBuilder| {
            let (argv, argc) = this.arg_array(b, &vals);
            let id = Self::c(b, func as u64);
            let r = this.rt_v(b, "ot_math_call", &[id, argv, argc]);
            let unwind = this.unwind;
            this.check_exception(b, r, unwind);
            this.last_call = None;
            r
        };
        let can_inline = matches!(
            (name, vals.len()),
            ("sqrt" | "floor" | "ceil" | "trunc" | "abs" | "round", 1) | ("min" | "max", 2)
        );
        if !can_inline {
            return Ok(call_runtime(self, b));
        }
        let floats: Vec<Value> = vals.iter().map(|v| Self::to_f64(b, *v)).collect();
        if args.iter().all(|a| self.is_num(*a)) {
            let r = Self::math_inline(b, name, &floats).expect("inline Math function");
            return Ok(Self::from_f64(b, r));
        }
        let mut ok = Self::both_numbers(b, vals[0], vals[0]);
        for v in &vals[1..] {
            let n = Self::both_numbers(b, *v, *v);
            ok = b.ins().band(ok, n);
        }
        let fast = b.create_block();
        let slow = b.create_block();
        let join = b.create_block();
        b.append_block_param(join, types::I64);
        b.ins().brif(ok, fast, &[], slow, &[]);
        b.switch_to_block(fast);
        b.seal_block(fast);
        let r = Self::math_inline(b, name, &floats).expect("inline Math function");
        let r = Self::from_f64(b, r);
        b.ins().jump(join, &[r]);
        b.switch_to_block(slow);
        b.seal_block(slow);
        let r = call_runtime(self, b);
        b.ins().jump(join, &[r]);
        b.switch_to_block(join);
        b.seal_block(join);
        Ok(b.block_params(join)[0])
    }

    /// A bitwise operator: `f` on the operands' 32-bit integers when both
    /// are numbers below 2^63 (`ToInt32` is then the saturated conversion,
    /// truncated), else the runtime (`valueOf` may throw). The result of
    /// `>>>` is unsigned.
    fn int_binop(
        &mut self,
        b: &mut FunctionBuilder,
        dst: ValueId,
        x: ValueId,
        y: ValueId,
        stub: &str,
        f: fn(&mut FunctionBuilder, Value, Value) -> Value,
    ) -> Result<(), BackendError> {
        let xv = self.get(x)?;
        let yv = self.get(y)?;
        let int32 = |b: &mut FunctionBuilder, v: Value| -> (Value, Value) {
            const LIMIT: f64 = 9_223_372_036_854_775_808.0;
            let f = Self::to_f64(b, v);
            let i = b.ins().fcvt_to_sint_sat(types::I64, f);
            let lo = b.ins().f64const(-LIMIT);
            let hi = b.ins().f64const(LIMIT);
            let above = b.ins().fcmp(FloatCC::GreaterThan, f, lo);
            let below = b.ins().fcmp(FloatCC::LessThan, f, hi);
            (b.ins().ireduce(types::I32, i), b.ins().band(above, below))
        };
        let (xi, xok) = int32(b, xv);
        let (yi, yok) = int32(b, yv);
        let mut ok = b.ins().band(xok, yok);
        if !(self.is_num(x) && self.is_num(y)) {
            let both = Self::both_numbers(b, xv, yv);
            ok = b.ins().band(ok, both);
        }
        let int_blk = b.create_block();
        let any_blk = b.create_block();
        let join = b.create_block();
        b.append_block_param(join, types::I64);
        b.ins().brif(ok, int_blk, &[], any_blk, &[]);

        b.switch_to_block(int_blk);
        b.seal_block(int_blk);
        let r = f(b, xi, yi);
        let rf = if stub == "ot_shr_u" {
            b.ins().fcvt_from_uint(types::F64, r)
        } else {
            b.ins().fcvt_from_sint(types::F64, r)
        };
        let r = Self::from_f64(b, rf);
        b.ins().jump(join, &[r]);

        b.switch_to_block(any_blk);
        b.seal_block(any_blk);
        let r = self.rt_v(b, stub, &[xv, yv]);
        let unwind = self.unwind;
        self.check_exception(b, r, unwind);
        self.last_call = None;
        b.ins().jump(join, &[r]);

        b.switch_to_block(join);
        b.seal_block(join);
        let r = b.block_params(join)[0];
        self.values.insert(dst, r);
        Ok(())
    }

    /// JavaScript `x % y`. When both are numbers, both are integers below
    /// 2^53 and `y` is not 0, an integer remainder with the sign of `x`
    /// (`fcopysign` also gives `-0`); otherwise the runtime (Cranelift has no
    /// floating-point remainder).
    fn js_mod(
        &mut self,
        b: &mut FunctionBuilder,
        dst: ValueId,
        x: ValueId,
        y: ValueId,
    ) -> Result<(), BackendError> {
        let xv = self.get(x)?;
        let yv = self.get(y)?;
        let xf = Self::to_f64(b, xv);
        let yf = Self::to_f64(b, yv);
        let exact = |b: &mut FunctionBuilder, f: Value| -> (Value, Value) {
            const LIMIT: i64 = 1 << 53;
            let i = b.ins().fcvt_to_sint_sat(types::I64, f);
            let back = b.ins().fcvt_from_sint(types::F64, i);
            let same = b.ins().fcmp(FloatCC::Equal, back, f);
            let lo = b.ins().icmp_imm(IntCC::SignedGreaterThan, i, -LIMIT);
            let hi = b.ins().icmp_imm(IntCC::SignedLessThan, i, LIMIT);
            let ok = b.ins().band(same, lo);
            (i, b.ins().band(ok, hi))
        };
        let (xi, xok) = exact(b, xf);
        let (yi, yok) = exact(b, yf);
        let nonzero = b.ins().icmp_imm(IntCC::NotEqual, yi, 0);
        let mut ints = b.ins().band(xok, yok);
        ints = b.ins().band(ints, nonzero);
        if !(self.is_num(x) && self.is_num(y)) {
            let both = Self::both_numbers(b, xv, yv);
            ints = b.ins().band(ints, both);
        }
        let int_blk = b.create_block();
        let any_blk = b.create_block();
        let join = b.create_block();
        b.append_block_param(join, types::I64);
        b.ins().brif(ints, int_blk, &[], any_blk, &[]);

        b.switch_to_block(int_blk);
        b.seal_block(int_blk);
        let r = b.ins().srem(xi, yi);
        let rf = b.ins().fcvt_from_sint(types::F64, r);
        let rf = b.ins().fcopysign(rf, xf);
        let r = Self::from_f64(b, rf);
        b.ins().jump(join, &[r]);

        b.switch_to_block(any_blk);
        b.seal_block(any_blk);
        let r = self.rt_v(b, "ot_mod_any", &[xv, yv]);
        let unwind = self.unwind;
        self.check_exception(b, r, unwind);
        self.last_call = None;
        b.ins().jump(join, &[r]);

        b.switch_to_block(join);
        b.seal_block(join);
        let r = b.block_params(join)[0];
        self.values.insert(dst, r);
        Ok(())
    }

    fn compare(
        &mut self,
        b: &mut FunctionBuilder,
        dst: ValueId,
        x: ValueId,
        y: ValueId,
        cc: FloatCC,
        stub: &str,
    ) -> Result<(), BackendError> {
        if self.is_num(x) && self.is_num(y) {
            let xv = self.get(x)?;
            let yv = self.get(y)?;
            let xf = Self::to_f64(b, xv);
            let yf = Self::to_f64(b, yv);
            let cmp = b.ins().fcmp(cc, xf, yf);
            let r = Self::box_bool(b, cmp);
            self.values.insert(dst, r);
            Ok(())
        } else {
            // Inline when both turn out to be numbers.
            self.any_binop(b, dst, x, y, stub, true, &|b, p, q| {
                let pf = Self::to_f64(b, p);
                let qf = Self::to_f64(b, q);
                let cmp = b.ins().fcmp(cc, pf, qf);
                Self::box_bool(b, cmp)
            })
        }
    }

    fn op(&mut self, b: &mut FunctionBuilder, op: &IrOp) -> Result<(), BackendError> {
        match op {
            IrOp::Const(dst, lit) => {
                let v = self.literal(b, lit)?;
                self.values.insert(*dst, v);
            }

            IrOp::AddNum(d, x, y) => self.num_binop(b, *d, *x, *y, |b, p, q| b.ins().fadd(p, q))?,
            IrOp::SubNum(d, x, y) => self.num_binop(b, *d, *x, *y, |b, p, q| b.ins().fsub(p, q))?,
            IrOp::MulNum(d, x, y) => self.num_binop(b, *d, *x, *y, |b, p, q| b.ins().fmul(p, q))?,
            IrOp::DivNum(d, x, y) => self.num_binop(b, *d, *x, *y, |b, p, q| b.ins().fdiv(p, q))?,
            IrOp::ModNum(d, x, y) | IrOp::ModAny(d, x, y) => self.js_mod(b, *d, *x, *y)?,
            IrOp::NegNum(d, x) => {
                let v = self.get(*x)?;
                let f = Self::to_f64(b, v);
                let r = b.ins().fneg(f);
                let r = Self::from_f64(b, r);
                self.values.insert(*d, r);
            }

            IrOp::AddAny(d, x, y) => {
                self.any_arith(b, *d, *x, *y, "ot_add_any", |b, p, q| b.ins().fadd(p, q))?
            }
            IrOp::SubAny(d, x, y) => {
                self.any_arith(b, *d, *x, *y, "ot_sub_any", |b, p, q| b.ins().fsub(p, q))?
            }
            IrOp::MulAny(d, x, y) => {
                self.any_arith(b, *d, *x, *y, "ot_mul_any", |b, p, q| b.ins().fmul(p, q))?
            }
            IrOp::DivAny(d, x, y) => {
                self.any_arith(b, *d, *x, *y, "ot_div_any", |b, p, q| b.ins().fdiv(p, q))?
            }
            IrOp::Pow(d, x, y) => self.rt_binop(b, *d, "ot_pow", *x, *y)?,
            IrOp::BitAnd(d, x, y) => {
                self.int_binop(b, *d, *x, *y, "ot_bit_and", |b, p, q| b.ins().band(p, q))?
            }
            IrOp::BitOr(d, x, y) => {
                self.int_binop(b, *d, *x, *y, "ot_bit_or", |b, p, q| b.ins().bor(p, q))?
            }
            IrOp::Xor(d, x, y) => {
                self.int_binop(b, *d, *x, *y, "ot_bit_xor", |b, p, q| b.ins().bxor(p, q))?
            }
            IrOp::Shl(d, x, y) => self.int_binop(b, *d, *x, *y, "ot_shl", |b, p, q| {
                let n = b.ins().band_imm(q, 31);
                b.ins().ishl(p, n)
            })?,
            IrOp::Shr(d, x, y) => self.int_binop(b, *d, *x, *y, "ot_shr", |b, p, q| {
                let n = b.ins().band_imm(q, 31);
                b.ins().sshr(p, n)
            })?,
            IrOp::ShrU(d, x, y) => self.int_binop(b, *d, *x, *y, "ot_shr_u", |b, p, q| {
                let n = b.ins().band_imm(q, 31);
                b.ins().ushr(p, n)
            })?,
            IrOp::NegAny(d, x) => self.rt_unop(b, *d, "ot_neg", *x)?,

            IrOp::EqStrict(d, x, y) | IrOp::NeStrict(d, x, y) => {
                let r = if self.is_num(*x) && self.is_num(*y) {
                    let xv = self.get(*x)?;
                    let yv = self.get(*y)?;
                    let xf = Self::to_f64(b, xv);
                    let yf = Self::to_f64(b, yv);
                    let cmp = b.ins().fcmp(FloatCC::Equal, xf, yf);
                    Self::box_bool(b, cmp)
                } else {
                    let args = [self.get(*x)?, self.get(*y)?];
                    self.rt_v(b, "ot_eq_strict", &args)
                };
                let r = if matches!(op, IrOp::NeStrict(..)) {
                    b.ins().bxor_imm(r, 1)
                } else {
                    r
                };
                self.values.insert(*d, r);
            }
            IrOp::EqLoose(d, x, y) | IrOp::NeLoose(d, x, y) => {
                let args = [self.get(*x)?, self.get(*y)?];
                let r = self.rt_v(b, "ot_eq_loose", &args);
                let r = if matches!(op, IrOp::NeLoose(..)) {
                    b.ins().bxor_imm(r, 1)
                } else {
                    r
                };
                self.values.insert(*d, r);
            }
            IrOp::Lt(d, x, y) => self.compare(b, *d, *x, *y, FloatCC::LessThan, "ot_lt")?,
            IrOp::LtEq(d, x, y) => {
                self.compare(b, *d, *x, *y, FloatCC::LessThanOrEqual, "ot_lte")?
            }
            IrOp::Gt(d, x, y) => self.compare(b, *d, *x, *y, FloatCC::GreaterThan, "ot_gt")?,
            IrOp::GtEq(d, x, y) => {
                self.compare(b, *d, *x, *y, FloatCC::GreaterThanOrEqual, "ot_gte")?
            }

            IrOp::Not(d, x) => {
                if self.is_bool(*x) {
                    let v = self.get(*x)?;
                    let r = b.ins().bxor_imm(v, 1);
                    self.values.insert(*d, r);
                } else {
                    self.rt_unop(b, *d, "ot_not", *x)?;
                }
            }
            IrOp::And(d, x, y) | IrOp::Or(d, x, y) => {
                let t = self.truthy(b, *x)?;
                let (xv, yv) = (self.get(*x)?, self.get(*y)?);
                let r = if matches!(op, IrOp::And(..)) {
                    b.ins().select(t, yv, xv)
                } else {
                    b.ins().select(t, xv, yv)
                };
                self.values.insert(*d, r);
            }
            IrOp::ToBool(d, x) => self.rt_unop(b, *d, "ot_to_boolean", *x)?,
            IrOp::ToNum(d, x) => self.rt_unop(b, *d, "ot_to_number", *x)?,
            IrOp::ToStr(d, x) => self.rt_unop(b, *d, "ot_to_str", *x)?,
            IrOp::TypeOf(d, x) => self.rt_unop(b, *d, "ot_typeof", *x)?,

            IrOp::LoadLocal(d, slot) => {
                let s = *self.locals.get(*slot as usize).ok_or_else(|| {
                    BackendError::Cranelift(format!("internal error: bad local slot {}", slot))
                })?;
                let v = match s {
                    Local::Slot(s) => b.ins().stack_load(types::I64, s, 0),
                    Local::Frame(o) => {
                        b.ins()
                            .load(types::I64, MemFlags::trusted(), self.frame.unwrap(), o)
                    }
                };
                self.values.insert(*d, v);
            }
            IrOp::StoreLocal(slot, v) => {
                let s = *self.locals.get(*slot as usize).ok_or_else(|| {
                    BackendError::Cranelift(format!("internal error: bad local slot {}", slot))
                })?;
                let v = self.get(*v)?;
                match s {
                    Local::Slot(s) => {
                        b.ins().stack_store(v, s, 0);
                    }
                    Local::Frame(o) => {
                        b.ins()
                            .store(MemFlags::trusted(), v, self.frame.unwrap(), o);
                    }
                }
            }
            IrOp::LoadGlobal(d, name) => {
                let r = match self.global_slots.get(name).copied() {
                    // A global of the program: its slot, unless not yet
                    // assigned (then the runtime: a built-in, or "not
                    // defined").
                    Some(data) => {
                        let gv = self.module.declare_data_in_func(data, b.func);
                        let p = b.ins().global_value(types::I64, gv);
                        let v = b.ins().load(types::I64, MemFlags::trusted(), p, 0);
                        let unbound = b.ins().icmp_imm(IntCC::Equal, v, UNBOUND_BITS as i64);
                        let slow = b.create_block();
                        let join = b.create_block();
                        b.set_cold_block(slow);
                        b.append_block_param(join, types::I64);
                        b.ins().brif(unbound, slow, &[], join, &[v]);
                        b.switch_to_block(slow);
                        b.seal_block(slow);
                        let (np, nn) = self.name(b, name);
                        let r = self.rt_v(b, "ot_get_global", &[np, nn]);
                        let unwind = self.unwind;
                        self.check_exception(b, r, unwind);
                        self.last_call = None;
                        b.ins().jump(join, &[r]);
                        b.switch_to_block(join);
                        b.seal_block(join);
                        b.block_params(join)[0]
                    }
                    None => {
                        let (p, n) = self.name(b, name);
                        self.rt_v(b, "ot_get_global", &[p, n])
                    }
                };
                self.values.insert(*d, r);
            }
            IrOp::TypeOfGlobal(d, name) => {
                let (p, n) = self.name(b, name);
                let r = self.rt_v(b, "ot_typeof_global", &[p, n]);
                self.values.insert(*d, r);
            }
            IrOp::StoreGlobal(name, v) => {
                let v = self.get(*v)?;
                match self.global_slots.get(name).copied() {
                    Some(data) => {
                        let gv = self.module.declare_data_in_func(data, b.func);
                        let p = b.ins().global_value(types::I64, gv);
                        b.ins().store(MemFlags::trusted(), v, p, 0);
                    }
                    None => {
                        let (p, n) = self.name(b, name);
                        self.rt(b, "ot_set_global", &[p, n, v]);
                    }
                }
            }
            IrOp::LoadThis(d) => {
                let t = self.this.expect("this is set in the prologue");
                self.values.insert(*d, t);
            }
            IrOp::LoadEnv(d) => {
                let e = self.env.expect("env is set in the prologue");
                self.values.insert(*d, e);
            }

            IrOp::Copy(d, s) | IrOp::Move(d, s) | IrOp::Borrow(d, s) | IrOp::BorrowMut(d, s) => {
                let v = self.get(*s)?;
                self.values.insert(*d, v);
                if let Some(addr) = self.direct.get(s).copied() {
                    self.direct.insert(*d, addr);
                }
            }

            IrOp::NewObject(d) => {
                let r = self.rt_v(b, "ot_alloc_object", &[]);
                self.values.insert(*d, r);
            }
            IrOp::NewArray(d) => {
                let cap = Self::c(b, 4);
                let r = self.rt_v(b, "ot_alloc_array", &[cap]);
                self.values.insert(*d, r);
            }
            IrOp::GetProp(d, obj, name) => {
                let o = self.get(*obj)?;
                let (p, n) = self.name(b, name);
                let r = self.rt_v(b, "ot_get_prop", &[o, p, n]);
                self.values.insert(*d, r);
            }
            IrOp::SetProp(obj, name, v) => {
                let o = self.get(*obj)?;
                let v = self.get(*v)?;
                let (p, n) = self.name(b, name);
                self.rt(b, "ot_set_prop", &[o, p, n, v]);
            }
            IrOp::GetPrivate(d, obj, name) => {
                let o = self.get(*obj)?;
                let (p, n) = self.name(b, name);
                let r = self.rt_v(b, "ot_get_private", &[o, p, n]);
                self.values.insert(*d, r);
            }
            IrOp::SetPrivate(obj, name, v) => {
                let o = self.get(*obj)?;
                let v = self.get(*v)?;
                let (p, n) = self.name(b, name);
                self.rt(b, "ot_set_private", &[o, p, n, v]);
            }
            IrOp::DeleteProp(d, obj, name) => {
                let o = self.get(*obj)?;
                let (p, n) = self.name(b, name);
                let r = self.rt_v(b, "ot_delete_prop", &[o, p, n]);
                self.values.insert(*d, r);
            }
            IrOp::GetElement(d, obj, key) => self.rt_binop(b, *d, "ot_get_index", *obj, *key)?,
            IrOp::SetElement(obj, key, v) => {
                let args = [self.get(*obj)?, self.get(*key)?, self.get(*v)?];
                self.rt(b, "ot_set_index", &args);
            }
            IrOp::ArrayLen(d, a) => self.rt_unop(b, *d, "ot_array_len", *a)?,
            IrOp::ArrayPush(a, v) => {
                let args = [self.get(*a)?, self.get(*v)?];
                self.rt(b, "ot_array_push", &args);
            }

            IrOp::MakeClosure(d, addr, env) => {
                let name = format!("func_{}", addr);
                let id = *self.func_ids.get(&name).ok_or_else(|| {
                    BackendError::Cranelift(format!("internal error: no function {}", name))
                })?;
                let fref = self.module.declare_func_in_func(id, b.func);
                let code = b.ins().func_addr(types::I64, fref);
                let e = self.get(*env)?;
                let v = self.rt_v(b, "ot_make_closure", &[code, e]);
                self.values.insert(*d, v);
            }
            IrOp::Call(d, callee, args) => {
                let vals: Vec<Value> = args
                    .iter()
                    .map(|a| self.get(*a))
                    .collect::<Result<_, _>>()?;
                let (argv, argc) = self.arg_array(b, &vals);
                let undefined = Self::c(b, UNDEFINED);
                let target = self.direct.get(callee).copied().and_then(|(addr, env)| {
                    self.func_ids
                        .get(&format!("func_{}", addr))
                        .map(|id| (*id, env))
                });
                let r = match target {
                    Some((id, env)) => {
                        let env = match env {
                            Some(e) => self.get(e)?,
                            None => undefined,
                        };
                        let fref = self.module.declare_func_in_func(id, b.func);
                        let call = b.ins().call(fref, &[undefined, argv, argc, env]);
                        let r = b.inst_results(call)[0];
                        self.last_call = Some(r);
                        r
                    }
                    None => {
                        let f = self.get(*callee)?;
                        self.rt_v(b, "ot_call", &[f, undefined, argv, argc])
                    }
                };
                self.values.insert(*d, r);
            }
            IrOp::Throw(v) => {
                let v = self.get(*v)?;
                self.rt(b, "ot_throw", &[v]);
            }
            IrOp::Construct(d, ctor, args) => {
                let c = self.get(*ctor)?;
                let vals: Vec<Value> = args
                    .iter()
                    .map(|a| self.get(*a))
                    .collect::<Result<_, _>>()?;
                let (argv, argc) = self.arg_array(b, &vals);
                let r = self.rt_v(b, "ot_construct", &[c, argv, argc]);
                self.values.insert(*d, r);
            }
            IrOp::CallSuper(d, parent, args) => {
                let p = self.get(*parent)?;
                let vals: Vec<Value> = args
                    .iter()
                    .map(|a| self.get(*a))
                    .collect::<Result<_, _>>()?;
                let (argv, argc) = self.arg_array(b, &vals);
                let this = self.this.unwrap();
                let r = self.rt_v(b, "ot_call_super", &[p, this, argv, argc]);
                self.values.insert(*d, r);
            }
            IrOp::SuperForward(d, parent) => {
                let p = self.get(*parent)?;
                let (argv, argc) = self.args.unwrap();
                let this = self.this.unwrap();
                let r = self.rt_v(b, "ot_call_super", &[p, this, argv, argc]);
                self.values.insert(*d, r);
            }
            IrOp::CallWithThis(d, func, this, args) => {
                let f = self.get(*func)?;
                let t = self.get(*this)?;
                let vals: Vec<Value> = args
                    .iter()
                    .map(|a| self.get(*a))
                    .collect::<Result<_, _>>()?;
                let (argv, argc) = self.arg_array(b, &vals);
                let r = self.rt_v(b, "ot_call", &[f, t, argv, argc]);
                self.values.insert(*d, r);
            }
            IrOp::InstanceOf(d, v, c) => self.rt_binop(b, *d, "ot_instanceof", *v, *c)?,
            IrOp::In(d, k, o) => self.rt_binop(b, *d, "ot_in", *k, *o)?,
            IrOp::GetSuperProp(d, this, base, name) => {
                let t = self.get(*this)?;
                let o = self.get(*base)?;
                let (p, n) = self.name(b, name);
                let r = self.rt_v(b, "ot_get_super_prop", &[t, o, p, n]);
                self.values.insert(*d, r);
            }
            IrOp::TakeException(d) => {
                let r = self.rt_v(b, "ot_take_exception", &[]);
                self.values.insert(*d, r);
            }
            IrOp::Rethrow(thrown, v) => {
                let args = [self.get(*thrown)?, self.get(*v)?];
                self.rt(b, "ot_rethrow", &args);
            }
            IrOp::Loc(n) => {
                self.site = (*n != crate::ir::lower::NO_SITE).then_some(*n);
            }
            IrOp::RestArgs(d, n) => {
                let (argv, argc) = self.args.unwrap();
                let n = Self::c(b, *n as u64);
                let r = self.rt_v(b, "ot_rest_args", &[argv, argc, n]);
                self.values.insert(*d, r);
            }
            IrOp::AsyncStart(d) => {
                let r = self.rt_v(b, "ot_async_start", &[]);
                self.values.insert(*d, r);
            }
            IrOp::AsyncResolve(d, p, v) => self.rt_binop(b, *d, "ot_async_resolve", *p, *v)?,
            IrOp::ArraySpread(d, t, s) => self.rt_binop(b, *d, "ot_array_spread", *t, *s)?,
            IrOp::ObjectSpread(d, t, s) => self.rt_binop(b, *d, "ot_object_spread", *t, *s)?,
            IrOp::AsyncReject(d, p, v) => self.rt_binop(b, *d, "ot_async_reject", *p, *v)?,
            IrOp::IsResume(d) => {
                let (_, argc) = self.args.unwrap();
                let resumed = b.ins().icmp_imm(
                    IntCC::Equal,
                    argc,
                    crate::runtime::stubs::ASYNC_RESUME_ARGC as i64,
                );
                let r = Self::box_bool(b, resumed);
                self.values.insert(*d, r);
            }
            IrOp::ResumePoint(d) => {
                let frame = self.frame.unwrap();
                let r = b.ins().load(types::I64, MemFlags::trusted(), frame, 0);
                self.values.insert(*d, r);
            }
            IrOp::Await(v, k) => {
                let frame = self.frame.unwrap();
                let point = Self::c(b, (*k as f64).to_bits());
                b.ins().store(MemFlags::trusted(), point, frame, 0);
                let fref = self
                    .module
                    .declare_func_in_func(self.func_ids[self.own_name], b.func);
                let code = b.ins().func_addr(types::I64, fref);
                let v = self.get(*v)?;
                let (this, env) = (self.this.unwrap(), self.env.unwrap());
                self.rt(b, "ot_await", &[v, frame, code, this, env]);
            }
            IrOp::AwaitResult(d) => {
                let frame = self.frame.unwrap();
                let r = self.rt_v(b, "ot_await_result", &[frame]);
                self.values.insert(*d, r);
            }
            IrOp::MathCall(d, func, args) => {
                let r = self.math_call(b, *func, args)?;
                self.values.insert(*d, r);
            }
            IrOp::Intrinsic(d, id, args) => {
                let vals: Vec<Value> = args
                    .iter()
                    .map(|a| self.get(*a))
                    .collect::<Result<_, _>>()?;
                let (argv, argc) = self.arg_array(b, &vals);
                let id = Self::c(b, *id as u64);
                let v = self.rt_v(b, "ot_intrinsic", &[id, argv, argc]);
                self.values.insert(*d, v);
            }
            IrOp::CallMethod(d, recv, name, args) => {
                let r = self.get(*recv)?;
                let vals: Vec<Value> = args
                    .iter()
                    .map(|a| self.get(*a))
                    .collect::<Result<_, _>>()?;
                let (argv, argc) = self.arg_array(b, &vals);
                let (p, n) = self.name(b, name);
                let v = self.rt_v(b, "ot_call_method", &[r, p, n, argv, argc]);
                self.values.insert(*d, v);
            }

            other => {
                return Err(BackendError::UnsupportedOp(format!(
                    "the Cranelift backend does not support `{}` yet",
                    other
                )));
            }
        }
        Ok(())
    }

    fn terminator(
        &mut self,
        b: &mut FunctionBuilder,
        term: &Terminator,
    ) -> Result<(), BackendError> {
        match term {
            Terminator::Jump(t) => {
                b.ins().jump(self.blocks[t], &[]);
            }
            Terminator::Branch(cond, t, f) => {
                let c = self.truthy(b, *cond)?;
                b.ins().brif(c, self.blocks[t], &[], self.blocks[f], &[]);
            }
            Terminator::Return(v) => {
                let r = match v {
                    Some(v) => self.get(*v)?,
                    None => Self::c(b, UNDEFINED),
                };
                b.ins().return_(&[r]);
            }
            Terminator::Unreachable => {
                b.ins().trap(TrapCode::user(1).unwrap());
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_codegen_creation() {
        let config = BackendConfig::default();
        let codegen = CraneliftCodegen::new(&config);
        assert!(codegen.is_ok());
    }

    #[test]
    fn every_runtime_function_has_a_jit_symbol() {
        for (name, _, _) in RUNTIME {
            assert!(runtime_symbol(name).is_some(), "{name} has no JIT symbol");
        }
    }
}
