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
use crate::runtime::abi::EXCEPTION_BITS;

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
                    stubs: HashMap::new(),
                    values: HashMap::new(),
                    blocks: HashMap::new(),
                    locals: Vec::new(),
                    types: &func.value_types,
                    direct: HashMap::new(),
                    env: None,
                    this: None,
                    last_call: None,
                    exception_return: None,
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
    stubs: HashMap<String, FuncRef>,
    values: HashMap<ValueId, Value>,
    blocks: HashMap<BlockId, Block>,
    locals: Vec<StackSlot>,
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
    /// Block returning `EXCEPTION_BITS`, for exceptions no `catch` in this
    /// function handles.
    exception_return: Option<Block>,
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
        let next = b.create_block();
        b.ins().brif(thrown, target, &[], next, &[]);
        b.switch_to_block(next);
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

        for _ in &func.locals {
            let slot =
                b.create_sized_stack_slot(StackSlotData::new(StackSlotKind::ExplicitSlot, 8, 3));
            let u = Self::c(b, UNDEFINED);
            b.ins().stack_store(u, slot, 0);
            self.locals.push(slot);
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
            self.rt_binop(b, dst, stub, x, y)
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
            // Cranelift has no frem; the runtime computes `%` exactly as LLVM's frem.
            IrOp::ModNum(d, x, y) => self.rt_binop(b, *d, "ot_mod_any", *x, *y)?,
            IrOp::NegNum(d, x) => {
                let v = self.get(*x)?;
                let f = Self::to_f64(b, v);
                let r = b.ins().fneg(f);
                let r = Self::from_f64(b, r);
                self.values.insert(*d, r);
            }

            IrOp::AddAny(d, x, y) => self.rt_binop(b, *d, "ot_add_any", *x, *y)?,
            IrOp::SubAny(d, x, y) => self.rt_binop(b, *d, "ot_sub_any", *x, *y)?,
            IrOp::MulAny(d, x, y) => self.rt_binop(b, *d, "ot_mul_any", *x, *y)?,
            IrOp::DivAny(d, x, y) => self.rt_binop(b, *d, "ot_div_any", *x, *y)?,
            IrOp::ModAny(d, x, y) => self.rt_binop(b, *d, "ot_mod_any", *x, *y)?,
            IrOp::Pow(d, x, y) => self.rt_binop(b, *d, "ot_pow", *x, *y)?,
            IrOp::BitAnd(d, x, y) => self.rt_binop(b, *d, "ot_bit_and", *x, *y)?,
            IrOp::BitOr(d, x, y) => self.rt_binop(b, *d, "ot_bit_or", *x, *y)?,
            IrOp::Xor(d, x, y) => self.rt_binop(b, *d, "ot_bit_xor", *x, *y)?,
            IrOp::Shl(d, x, y) => self.rt_binop(b, *d, "ot_shl", *x, *y)?,
            IrOp::Shr(d, x, y) => self.rt_binop(b, *d, "ot_shr", *x, *y)?,
            IrOp::ShrU(d, x, y) => self.rt_binop(b, *d, "ot_shr_u", *x, *y)?,
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
            IrOp::TypeOf(d, x) => self.rt_unop(b, *d, "ot_typeof", *x)?,

            IrOp::LoadLocal(d, slot) => {
                let s = *self.locals.get(*slot as usize).ok_or_else(|| {
                    BackendError::Cranelift(format!("internal error: bad local slot {}", slot))
                })?;
                let v = b.ins().stack_load(types::I64, s, 0);
                self.values.insert(*d, v);
            }
            IrOp::StoreLocal(slot, v) => {
                let s = *self.locals.get(*slot as usize).ok_or_else(|| {
                    BackendError::Cranelift(format!("internal error: bad local slot {}", slot))
                })?;
                let v = self.get(*v)?;
                b.ins().stack_store(v, s, 0);
            }
            IrOp::LoadGlobal(d, name) => {
                let (p, n) = self.name(b, name);
                let r = self.rt_v(b, "ot_get_global", &[p, n]);
                self.values.insert(*d, r);
            }
            IrOp::TypeOfGlobal(d, name) => {
                let (p, n) = self.name(b, name);
                let r = self.rt_v(b, "ot_typeof_global", &[p, n]);
                self.values.insert(*d, r);
            }
            IrOp::StoreGlobal(name, v) => {
                let v = self.get(*v)?;
                let (p, n) = self.name(b, name);
                self.rt(b, "ot_set_global", &[p, n, v]);
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
            IrOp::TakeException(d) => {
                let r = self.rt_v(b, "ot_take_exception", &[]);
                self.values.insert(*d, r);
            }
            IrOp::Rethrow(thrown, v) => {
                let args = [self.get(*thrown)?, self.get(*v)?];
                self.rt(b, "ot_rethrow", &args);
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
