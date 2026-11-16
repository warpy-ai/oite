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
                    object_slots: crate::ir::alloc_hints::object_slots(func),
                    literals: crate::ir::alloc_hints::literal_props(
                        func,
                        crate::runtime::heap::MAX_INLINE_SLOTS,
                    ),
                    literal_stores: HashMap::new(),
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
    /// How many properties each `NewObject` site is about to store.
    object_slots: HashMap<ValueId, usize>,
    /// The properties each literal site is allocated with
    /// (`alloc_hints::literal_props`), and how many of them have been
    /// stored so far.
    literals: HashMap<ValueId, Vec<String>>,
    literal_stores: HashMap<ValueId, usize>,
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
mod arrays;
mod equality;
mod numeric;
mod objects;
mod ops;
mod symbols;
mod typecheck;

use symbols::runtime_symbol;

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
