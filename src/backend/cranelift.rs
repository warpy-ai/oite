//! Cranelift code generation for tscl IR
//!
//! This module translates tscl SSA IR to Cranelift IR, which is then compiled
//! to native machine code. Key design decisions:
//!
//! - All values are 64-bit (NaN-boxed)
//! - Specialized ops (AddNum, etc.) compile to direct FP instructions
//! - Dynamic ops (AddAny, etc.) call runtime stubs
//! - Borrow ops are zero-cost (just pointer copies)

use cranelift::prelude::*;
use cranelift_codegen::ir::{FuncRef, StackSlot};
use cranelift_codegen::settings;
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{FuncId, Linkage, Module};
use std::collections::HashMap;

use super::layout::VALUE_SIZE;
use super::{BackendConfig, BackendError};
use crate::ir::{BasicBlock, BlockId, IrFunction, IrModule, IrOp, Literal, Terminator, ValueId};

/// Cranelift code generator
#[allow(dead_code)]
pub struct CraneliftCodegen {
    /// The JIT module being built
    module: JITModule,
    /// Codegen context (reused for each function)
    ctx: codegen::Context,
    /// Function builder context (reused)
    builder_ctx: FunctionBuilderContext,
    /// Declared runtime stubs (name -> FuncId)
    stubs: HashMap<String, FuncId>,
    /// Compiled function pointers
    compiled_funcs: HashMap<String, *const u8>,
    /// Backend configuration
    config: BackendConfig,
}

impl CraneliftCodegen {
    /// Create a new code generator
    pub fn new(config: &BackendConfig) -> Result<Self, BackendError> {
        // Create JIT builder with appropriate settings for the platform
        let mut flag_builder = settings::builder();

        // Disable PLT on aarch64 since it's not supported in cranelift-jit 0.113
        flag_builder.set("use_colocated_libcalls", "true").unwrap();
        flag_builder.set("is_pic", "false").unwrap();

        let isa_builder = cranelift_native::builder()
            .map_err(|e| BackendError::Cranelift(format!("Failed to create ISA builder: {}", e)))?;

        let isa = isa_builder
            .finish(settings::Flags::new(flag_builder))
            .map_err(|e| BackendError::Cranelift(format!("Failed to create ISA: {}", e)))?;

        let mut builder = JITBuilder::with_isa(isa, cranelift_module::default_libcall_names());

        // Register runtime stubs as symbols
        Self::register_runtime_symbols(&mut builder);

        let module = JITModule::new(builder);

        let ctx = module.make_context();

        // Note: Optimization level is configured via the ISA settings,
        // not on the Context directly. For now, we use defaults.

        Ok(Self {
            module,
            ctx,
            builder_ctx: FunctionBuilderContext::new(),
            stubs: HashMap::new(),
            compiled_funcs: HashMap::new(),
            config: config.clone(),
        })
    }

    /// Register runtime stub functions as symbols for the JIT
    fn register_runtime_symbols(builder: &mut JITBuilder) {
        use crate::runtime::stubs::*;

        // Allocation stubs
        builder.symbol("tscl_alloc_object", tscl_alloc_object as *const u8);
        builder.symbol("tscl_alloc_array", tscl_alloc_array as *const u8);
        builder.symbol("tscl_alloc_string", tscl_alloc_string as *const u8);

        // Property access stubs
        builder.symbol("tscl_get_prop", tscl_get_prop as *const u8);
        builder.symbol("tscl_set_prop", tscl_set_prop as *const u8);
        builder.symbol("tscl_get_element", tscl_get_element as *const u8);
        builder.symbol("tscl_set_element", tscl_set_element as *const u8);

        // Dynamic arithmetic stubs
        builder.symbol("tscl_add_any", tscl_add_any as *const u8);
        builder.symbol("tscl_sub_any", tscl_sub_any as *const u8);
        builder.symbol("tscl_mul_any", tscl_mul_any as *const u8);
        builder.symbol("tscl_div_any", tscl_div_any as *const u8);
        builder.symbol("tscl_mod_any", tscl_mod_any as *const u8);

        // Comparison stubs
        builder.symbol("tscl_eq_strict", tscl_eq_strict as *const u8);
        builder.symbol("tscl_lt", tscl_lt as *const u8);
        builder.symbol("tscl_gt", tscl_gt as *const u8);
        builder.symbol("tscl_not", tscl_not as *const u8);
        builder.symbol("tscl_neg", tscl_neg as *const u8);

        // Type conversion stubs
        builder.symbol("tscl_to_boolean", tscl_to_boolean as *const u8);
        builder.symbol("tscl_to_number", tscl_to_number as *const u8);

        // Console/IO stubs
        builder.symbol("tscl_console_log", tscl_console_log as *const u8);
        builder.symbol("tscl_call", tscl_call as *const u8);

        // Closure stubs
        builder.symbol("tscl_make_closure", tscl_make_closure as *const u8);
    }

    /// Declare a runtime stub function in the module
    #[allow(dead_code)]
    fn declare_stub(&mut self, name: &str, arg_count: usize) -> Result<FuncId, BackendError> {
        if let Some(&id) = self.stubs.get(name) {
            return Ok(id);
        }

        // All stubs use u64 arguments and return u64
        let mut sig = self.module.make_signature();
        for _ in 0..arg_count {
            sig.params.push(AbiParam::new(types::I64));
        }
        sig.returns.push(AbiParam::new(types::I64));

        let id = self
            .module
            .declare_function(name, Linkage::Import, &sig)
            .map_err(|e| {
                BackendError::Cranelift(format!("Failed to declare stub {}: {}", name, e))
            })?;

        self.stubs.insert(name.to_string(), id);
        Ok(id)
    }

    /// Compile an entire IR module (all functions at once)
    ///
    /// This is the preferred method as it allows inter-function calls to be resolved.
    pub fn compile_module(&mut self, ir_module: &IrModule) -> Result<(), BackendError> {
        // Step 1: Declare all functions first (so we can reference them from each other)
        let mut func_ids: HashMap<String, FuncId> = HashMap::new();
        let mut func_sigs: HashMap<String, Signature> = HashMap::new();

        for func in &ir_module.functions {
            let func_name = if func.name.is_empty() {
                "anonymous".to_string()
            } else {
                func.name.clone()
            };

            // Create function signature
            let mut sig = self.module.make_signature();
            for _ in &func.params {
                sig.params.push(AbiParam::new(types::I64));
            }
            sig.returns.push(AbiParam::new(types::I64));

            let func_id = self
                .module
                .declare_function(&func_name, Linkage::Export, &sig)
                .map_err(|e| {
                    BackendError::Cranelift(format!(
                        "Failed to declare function {}: {}",
                        func_name, e
                    ))
                })?;

            func_ids.insert(func_name.clone(), func_id);
            func_sigs.insert(func_name, sig);
        }

        // Step 2: Compile each function
        for func in &ir_module.functions {
            let func_name = if func.name.is_empty() {
                "anonymous".to_string()
            } else {
                func.name.clone()
            };

            self.ctx.clear();
            self.ctx.func.signature = func_sigs[&func_name].clone();

            // Build the function body with access to all declared functions
            {
                let mut builder = FunctionBuilder::new(&mut self.ctx.func, &mut self.builder_ctx);
                translate_function(&mut builder, &mut self.module, func, ir_module, &func_ids)?;
                builder.finalize();
            }

            // Define the function
            let func_id = func_ids[&func_name];
            self.module
                .define_function(func_id, &mut self.ctx)
                .map_err(|e| {
                    BackendError::Cranelift(format!(
                        "Failed to compile function {}: {}",
                        func_name, e
                    ))
                })?;
        }

        // Step 3: Finalize all definitions
        self.module
            .finalize_definitions()
            .map_err(|e| BackendError::Cranelift(format!("Failed to finalize: {}", e)))?;

        // Step 4: Get function pointers
        for (name, func_id) in &func_ids {
            let ptr = self.module.get_finalized_function(*func_id);
            self.compiled_funcs.insert(name.clone(), ptr);
        }

        Ok(())
    }

    /// Compile a single function (legacy method - use compile_module for better inter-function calls)
    pub fn compile_function(
        &mut self,
        func: &IrFunction,
        ir_module: &IrModule,
    ) -> Result<*const u8, BackendError> {
        // Clear previous context
        self.ctx.clear();

        // Create function signature
        let mut sig = self.module.make_signature();

        // Add parameters (all values are i64 / NaN-boxed)
        for _ in &func.params {
            sig.params.push(AbiParam::new(types::I64));
        }

        // Add return value
        sig.returns.push(AbiParam::new(types::I64));

        self.ctx.func.signature = sig.clone();

        // Declare the function
        let func_name = if func.name.is_empty() {
            "anonymous"
        } else {
            &func.name
        };

        let func_id = self
            .module
            .declare_function(func_name, Linkage::Export, &sig)
            .map_err(|e| {
                BackendError::Cranelift(format!("Failed to declare function {}: {}", func_name, e))
            })?;

        // Build the function body
        {
            let mut builder = FunctionBuilder::new(&mut self.ctx.func, &mut self.builder_ctx);

            // Create empty func_ids map for single-function compilation
            let func_ids = HashMap::new();
            translate_function(&mut builder, &mut self.module, func, ir_module, &func_ids)?;

            builder.finalize();
        }

        // Compile the function
        self.module
            .define_function(func_id, &mut self.ctx)
            .map_err(|e| {
                BackendError::Cranelift(format!("Failed to compile function {}: {}", func_name, e))
            })?;

        // Finalize and get the function pointer
        self.module
            .finalize_definitions()
            .map_err(|e| BackendError::Cranelift(format!("Failed to finalize: {}", e)))?;

        let ptr = self.module.get_finalized_function(func_id);
        self.compiled_funcs.insert(func_name.to_string(), ptr);

        Ok(ptr)
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

/// Translate a function from tscl IR to Cranelift IR
fn translate_function(
    builder: &mut FunctionBuilder,
    module: &mut JITModule,
    ir_func: &IrFunction,
    ir_module: &IrModule,
    func_ids: &HashMap<String, FuncId>,
) -> Result<(), BackendError> {
    let mut ctx = TranslationContext {
        values: HashMap::new(),
        blocks: HashMap::new(),
        locals: Vec::new(),
        stubs: HashMap::new(),
        module_func_ids: func_ids.clone(),
        ir_module_ref: ir_module,
        constants: HashMap::new(),
        local_stores: HashMap::new(),
        phi_params: HashMap::new(),
        block_phis: HashMap::new(),
    };

    // Create Cranelift blocks for each IR block
    for block in &ir_func.blocks {
        let cl_block = builder.create_block();
        ctx.blocks.insert(block.id, cl_block);
    }

    // Scan for phi nodes and set up block parameters
    for block in &ir_func.blocks {
        let cl_block = ctx.blocks[&block.id];
        let mut param_idx = 0;
        let mut phis = Vec::new();

        for op in &block.ops {
            if let IrOp::Phi(dst, entries) = op {
                // Add a block parameter for this phi
                builder.append_block_param(cl_block, types::I64);
                ctx.phi_params.insert(*dst, (block.id, param_idx));
                phis.push((*dst, entries.clone()));
                param_idx += 1;
            }
        }

        if !phis.is_empty() {
            ctx.block_phis.insert(block.id, phis);
        }
    }

    // Create stack slots for locals
    for _ in &ir_func.locals {
        let slot = builder.create_sized_stack_slot(StackSlotData::new(
            StackSlotKind::ExplicitSlot,
            VALUE_SIZE,
            0,
        ));
        ctx.locals.push(slot);
    }

    // Set up entry block with function parameters
    let entry_block = ctx.blocks[&BlockId(0)];
    builder.switch_to_block(entry_block);
    builder.append_block_params_for_function_params(entry_block);

    // Map function parameters to values (skip phi params which are handled separately)
    let func_param_count = ir_func.params.len();
    let params = builder.block_params(entry_block).to_vec();
    // The last func_param_count params are function params (phi params come first)
    let phi_param_count = params.len().saturating_sub(func_param_count);
    for (i, param) in params.iter().skip(phi_param_count).enumerate() {
        ctx.values.insert(ValueId(i as u32), *param);
    }

    // Translate each block
    for (i, block) in ir_func.blocks.iter().enumerate() {
        if i > 0 {
            let cl_block = ctx.blocks[&block.id];
            builder.switch_to_block(cl_block);

            // Map phi block parameters to values
            let block_params = builder.block_params(cl_block).to_vec();
            if let Some(phis) = ctx.block_phis.get(&block.id) {
                for (idx, (dst, _)) in phis.iter().enumerate() {
                    if idx < block_params.len() {
                        ctx.values.insert(*dst, block_params[idx]);
                    }
                }
            }
        }

        translate_block(builder, module, &mut ctx, block)?;
    }

    // Seal all blocks
    builder.seal_all_blocks();

    Ok(())
}

/// Translation context holding state during function translation
struct TranslationContext<'a> {
    /// Map from IR ValueId to Cranelift Value
    values: HashMap<ValueId, Value>,
    /// Map from IR BlockId to Cranelift Block
    blocks: HashMap<BlockId, Block>,
    /// Local variable storage (stack slots)
    locals: Vec<StackSlot>,
    /// Declared stubs in this function
    stubs: HashMap<String, FuncRef>,
    /// Map of module function names to their FuncId (for direct calls)
    module_func_ids: HashMap<String, FuncId>,
    /// Reference to the IR module for function lookups
    ir_module_ref: &'a IrModule,
    /// Track constant values for call resolution
    constants: HashMap<ValueId, Literal>,
    /// Track local slot contents (ValueId that was stored)
    local_stores: HashMap<u32, ValueId>,
    /// Phi node mappings: (dst ValueId) -> (block param index in target block)
    phi_params: HashMap<ValueId, (BlockId, usize)>,
    /// Phi entries for each block: BlockId -> Vec<(dst, entries)>
    block_phis: HashMap<BlockId, Vec<(ValueId, Vec<(BlockId, ValueId)>)>>,
}

/// Translate a single basic block
fn translate_block(
    builder: &mut FunctionBuilder,
    module: &mut JITModule,
    ctx: &mut TranslationContext,
    block: &BasicBlock,
) -> Result<(), BackendError> {
    // Translate each operation
    for op in &block.ops {
        translate_op(builder, module, ctx, op)?;
    }

    // Translate terminator, passing current block ID for phi argument resolution
    translate_terminator(builder, ctx, &block.terminator, block.id)?;

    Ok(())
}

/// Translate a single IR operation
fn translate_op(
    builder: &mut FunctionBuilder,
    module: &mut JITModule,
    ctx: &mut TranslationContext,
    op: &IrOp,
) -> Result<(), BackendError> {
    match op {
        // === Constants ===
        IrOp::Const(dst, lit) => {
            let val = translate_literal(builder, lit);
            ctx.values.insert(*dst, val);
            // Track the literal for call resolution
            ctx.constants.insert(*dst, lit.clone());
        }

        // === Specialized Numeric Operations (inline) ===
        IrOp::AddNum(dst, a, b) => {
            let va = get_value(ctx, *a)?;
            let vb = get_value(ctx, *b)?;
            // Bitcast to f64, add, bitcast back
            let fa = builder.ins().bitcast(types::F64, MemFlags::new(), va);
            let fb = builder.ins().bitcast(types::F64, MemFlags::new(), vb);
            let result = builder.ins().fadd(fa, fb);
            let result_i64 = builder.ins().bitcast(types::I64, MemFlags::new(), result);
            ctx.values.insert(*dst, result_i64);
        }

        IrOp::SubNum(dst, a, b) => {
            let va = get_value(ctx, *a)?;
            let vb = get_value(ctx, *b)?;
            let fa = builder.ins().bitcast(types::F64, MemFlags::new(), va);
            let fb = builder.ins().bitcast(types::F64, MemFlags::new(), vb);
            let result = builder.ins().fsub(fa, fb);
            let result_i64 = builder.ins().bitcast(types::I64, MemFlags::new(), result);
            ctx.values.insert(*dst, result_i64);
        }

        IrOp::MulNum(dst, a, b) => {
            let va = get_value(ctx, *a)?;
            let vb = get_value(ctx, *b)?;
            let fa = builder.ins().bitcast(types::F64, MemFlags::new(), va);
            let fb = builder.ins().bitcast(types::F64, MemFlags::new(), vb);
            let result = builder.ins().fmul(fa, fb);
            let result_i64 = builder.ins().bitcast(types::I64, MemFlags::new(), result);
            ctx.values.insert(*dst, result_i64);
        }

        IrOp::DivNum(dst, a, b) => {
            let va = get_value(ctx, *a)?;
            let vb = get_value(ctx, *b)?;
            let fa = builder.ins().bitcast(types::F64, MemFlags::new(), va);
            let fb = builder.ins().bitcast(types::F64, MemFlags::new(), vb);
            let result = builder.ins().fdiv(fa, fb);
            let result_i64 = builder.ins().bitcast(types::I64, MemFlags::new(), result);
            ctx.values.insert(*dst, result_i64);
        }

        IrOp::ModNum(dst, a, b) => {
            // Cranelift doesn't have fmod, so we compute a - floor(a/b) * b
            let va = get_value(ctx, *a)?;
            let vb = get_value(ctx, *b)?;
            let fa = builder.ins().bitcast(types::F64, MemFlags::new(), va);
            let fb = builder.ins().bitcast(types::F64, MemFlags::new(), vb);
            let div = builder.ins().fdiv(fa, fb);
            let floored = builder.ins().floor(div);
            let prod = builder.ins().fmul(floored, fb);
            let result = builder.ins().fsub(fa, prod);
            let result_i64 = builder.ins().bitcast(types::I64, MemFlags::new(), result);
            ctx.values.insert(*dst, result_i64);
        }

        IrOp::NegNum(dst, a) => {
            let va = get_value(ctx, *a)?;
            let fa = builder.ins().bitcast(types::F64, MemFlags::new(), va);
            let result = builder.ins().fneg(fa);
            let result_i64 = builder.ins().bitcast(types::I64, MemFlags::new(), result);
            ctx.values.insert(*dst, result_i64);
        }

        // === Dynamic Operations (call stubs) ===
        IrOp::AddAny(dst, a, b) => {
            let result = call_stub(builder, module, ctx, "tscl_add_any", &[*a, *b])?;
            ctx.values.insert(*dst, result);
        }

        IrOp::SubAny(dst, a, b) => {
            let result = call_stub(builder, module, ctx, "tscl_sub_any", &[*a, *b])?;
            ctx.values.insert(*dst, result);
        }

        IrOp::MulAny(dst, a, b) => {
            let result = call_stub(builder, module, ctx, "tscl_mul_any", &[*a, *b])?;
            ctx.values.insert(*dst, result);
        }

        IrOp::DivAny(dst, a, b) => {
            let result = call_stub(builder, module, ctx, "tscl_div_any", &[*a, *b])?;
            ctx.values.insert(*dst, result);
        }

        IrOp::ModAny(dst, a, b) => {
            let result = call_stub(builder, module, ctx, "tscl_mod_any", &[*a, *b])?;
            ctx.values.insert(*dst, result);
        }

        IrOp::NegAny(dst, a) => {
            let result = call_stub(builder, module, ctx, "tscl_neg", &[*a])?;
            ctx.values.insert(*dst, result);
        }

        // === Comparison Operations ===
        IrOp::Lt(dst, a, b) => {
            let va = get_value(ctx, *a)?;
            let vb = get_value(ctx, *b)?;
            let fa = builder.ins().bitcast(types::F64, MemFlags::new(), va);
            let fb = builder.ins().bitcast(types::F64, MemFlags::new(), vb);
            let cmp = builder.ins().fcmp(FloatCC::LessThan, fa, fb);
            // Convert bool to NaN-boxed boolean
            let result = bool_to_tscl_value(builder, cmp);
            ctx.values.insert(*dst, result);
        }

        IrOp::LtEq(dst, a, b) => {
            let va = get_value(ctx, *a)?;
            let vb = get_value(ctx, *b)?;
            let fa = builder.ins().bitcast(types::F64, MemFlags::new(), va);
            let fb = builder.ins().bitcast(types::F64, MemFlags::new(), vb);
            let cmp = builder.ins().fcmp(FloatCC::LessThanOrEqual, fa, fb);
            let result = bool_to_tscl_value(builder, cmp);
            ctx.values.insert(*dst, result);
        }

        IrOp::Gt(dst, a, b) => {
            let va = get_value(ctx, *a)?;
            let vb = get_value(ctx, *b)?;
            let fa = builder.ins().bitcast(types::F64, MemFlags::new(), va);
            let fb = builder.ins().bitcast(types::F64, MemFlags::new(), vb);
            let cmp = builder.ins().fcmp(FloatCC::GreaterThan, fa, fb);
            let result = bool_to_tscl_value(builder, cmp);
            ctx.values.insert(*dst, result);
        }

        IrOp::GtEq(dst, a, b) => {
            let va = get_value(ctx, *a)?;
            let vb = get_value(ctx, *b)?;
            let fa = builder.ins().bitcast(types::F64, MemFlags::new(), va);
            let fb = builder.ins().bitcast(types::F64, MemFlags::new(), vb);
            let cmp = builder.ins().fcmp(FloatCC::GreaterThanOrEqual, fa, fb);
            let result = bool_to_tscl_value(builder, cmp);
            ctx.values.insert(*dst, result);
        }

        IrOp::EqStrict(dst, a, b) => {
            let result = call_stub(builder, module, ctx, "tscl_eq_strict", &[*a, *b])?;
            ctx.values.insert(*dst, result);
        }

        IrOp::NeStrict(dst, a, b) => {
            // Call eq_strict then negate
            let eq_result = call_stub(builder, module, ctx, "tscl_eq_strict", &[*a, *b])?;
            let result = call_stub_with_values(builder, module, ctx, "tscl_not", &[eq_result])?;
            ctx.values.insert(*dst, result);
        }

        // === Logical Operations ===
        IrOp::Not(dst, a) => {
            let result = call_stub(builder, module, ctx, "tscl_not", &[*a])?;
            ctx.values.insert(*dst, result);
        }

        IrOp::And(dst, a, _b) | IrOp::Or(dst, a, _b) => {
            // For now, simple evaluation (not short-circuit)
            // Short-circuit would require control flow
            let va = get_value(ctx, *a)?;
            // Return first truthy/falsy value (simplified)
            ctx.values.insert(*dst, va);
        }

        // === Local Variable Operations ===
        IrOp::LoadLocal(dst, slot) => {
            let stack_slot = ctx
                .locals
                .get(*slot as usize)
                .ok_or_else(|| BackendError::Cranelift(format!("Invalid local slot: {}", slot)))?;
            let val = builder.ins().stack_load(types::I64, *stack_slot, 0);
            ctx.values.insert(*dst, val);

            // Propagate constant if the local was stored with a constant
            if let Some(&src_val) = ctx.local_stores.get(slot)
                && let Some(lit) = ctx.constants.get(&src_val)
            {
                ctx.constants.insert(*dst, lit.clone());
            }
        }

        IrOp::StoreLocal(slot, src) => {
            let val = get_value(ctx, *src)?;
            let stack_slot = ctx
                .locals
                .get(*slot as usize)
                .ok_or_else(|| BackendError::Cranelift(format!("Invalid local slot: {}", slot)))?;

            // Track which ValueId was stored in this slot
            ctx.local_stores.insert(*slot, *src);

            if let Some(_lit) = ctx.constants.get(src) {
                // We could track slot -> constant mapping, but for now
                // the existing local_stores -> constants chain should work
            }

            builder.ins().stack_store(val, *stack_slot, 0);
        }

        // === Global Variable Operations ===
        IrOp::LoadGlobal(dst, _name) => {
            // TODO: Implement global variable access
            // For now, return undefined
            let undefined = translate_literal(builder, &Literal::Undefined);
            ctx.values.insert(*dst, undefined);
        }

        IrOp::StoreGlobal(_name, _src) => {
            // TODO: Implement global variable store
        }

        // === Object Operations ===
        IrOp::NewObject(dst) => {
            let result = call_stub_no_args(builder, module, ctx, "tscl_alloc_object")?;
            ctx.values.insert(*dst, result);
        }

        IrOp::GetProp(dst, obj, _name) => {
            // TODO: Pass property name as string pointer
            let result = call_stub(builder, module, ctx, "tscl_get_prop", &[*obj])?;
            ctx.values.insert(*dst, result);
        }

        IrOp::SetProp(obj, _name, val) => {
            // TODO: Pass property name as string pointer
            call_stub(builder, module, ctx, "tscl_set_prop", &[*obj, *val])?;
        }

        IrOp::GetElement(dst, obj, idx) => {
            let result = call_stub(builder, module, ctx, "tscl_get_element", &[*obj, *idx])?;
            ctx.values.insert(*dst, result);
        }

        IrOp::SetElement(obj, idx, val) => {
            call_stub(
                builder,
                module,
                ctx,
                "tscl_set_element",
                &[*obj, *idx, *val],
            )?;
        }

        // === Array Operations ===
        IrOp::NewArray(dst) => {
            let capacity = builder.ins().iconst(types::I64, 8);
            let result =
                call_stub_with_values(builder, module, ctx, "tscl_alloc_array", &[capacity])?;
            ctx.values.insert(*dst, result);
        }

        IrOp::ArrayLen(dst, arr) => {
            // Get length property
            let result = call_stub(builder, module, ctx, "tscl_get_prop", &[*arr])?;
            ctx.values.insert(*dst, result);
        }

        IrOp::ArrayPush(arr, val) => {
            // TODO: Implement proper array push
            call_stub(builder, module, ctx, "tscl_set_element", &[*arr, *val])?;
        }

        // === Copy/Move Operations ===
        IrOp::Copy(dst, src)
        | IrOp::Move(dst, src)
        | IrOp::Borrow(dst, src)
        | IrOp::BorrowMut(dst, src) => {
            let val = get_value(ctx, *src)?;
            ctx.values.insert(*dst, val);
        }

        IrOp::Clone(dst, src) => {
            // For now, just copy (proper clone would allocate)
            let val = get_value(ctx, *src)?;
            ctx.values.insert(*dst, val);
        }

        IrOp::Deref(dst, src) => {
            // Dereference: load through pointer
            let ptr = get_value(ctx, *src)?;
            let val = builder.ins().load(types::I64, MemFlags::new(), ptr, 0);
            ctx.values.insert(*dst, val);
        }

        IrOp::DerefStore(dst, src) => {
            // Store through pointer
            let ptr = get_value(ctx, *dst)?;
            let val = get_value(ctx, *src)?;
            builder.ins().store(MemFlags::new(), val, ptr, 0);
        }

        IrOp::EndBorrow(_) => {
            // No-op at runtime (borrow checking is compile-time)
        }

        // === Type Operations ===
        IrOp::TypeCheck(dst, _val, _ty) => {
            // Type checks are compile-time in typed code
            let one = builder.ins().iconst(types::I8, 1);
            let true_val = bool_to_tscl_value(builder, one);
            ctx.values.insert(*dst, true_val);
        }

        IrOp::TypeGuard(dst, val, _ty) => {
            // Just pass through the value
            let v = get_value(ctx, *val)?;
            ctx.values.insert(*dst, v);
        }

        IrOp::ToBool(dst, val) => {
            let result = call_stub(builder, module, ctx, "tscl_to_boolean", &[*val])?;
            ctx.values.insert(*dst, result);
        }

        IrOp::ToNum(dst, val) => {
            let result = call_stub(builder, module, ctx, "tscl_to_number", &[*val])?;
            ctx.values.insert(*dst, result);
        }

        // === Phi Functions ===
        IrOp::Phi(dst, _entries) => {
            // Phi nodes are handled via block parameters
            // The value was already mapped when we switched to this block
            // If not present (shouldn't happen), use undefined as fallback
            if !ctx.values.contains_key(dst) {
                let undefined = translate_literal(builder, &Literal::Undefined);
                ctx.values.insert(*dst, undefined);
            }
        }

        // === Function Operations ===
        IrOp::Call(dst, func_val, args) => {
            let arg_values: Vec<Value> = args
                .iter()
                .map(|id| get_value(ctx, *id))
                .collect::<Result<_, _>>()?;

            // Try to resolve the function address for a direct call
            let func_addr = resolve_function_address(ctx, *func_val);

            if let Some(addr) = func_addr {
                // Look up the function by bytecode address
                let func_name = format!("func_{}", addr);

                if let Some(&func_id) = ctx.module_func_ids.get(&func_name) {
                    // Make a direct call to the compiled function
                    let func_ref = module.declare_func_in_func(func_id, builder.func);

                    let call = builder.ins().call(func_ref, &arg_values);
                    let results = builder.inst_results(call);

                    if results.is_empty() {
                        // Function returns void, use undefined
                        let undefined = translate_literal(builder, &Literal::Undefined);
                        ctx.values.insert(*dst, undefined);
                    } else {
                        ctx.values.insert(*dst, results[0]);
                    }
                } else {
                    // Function not found - use runtime stub
                    let func_ptr = get_value(ctx, *func_val)?;
                    let result =
                        call_indirect_function(builder, module, ctx, func_ptr, &arg_values)?;
                    ctx.values.insert(*dst, result);
                }
            } else {
                // Dynamic call - use runtime stub
                let func_ptr = get_value(ctx, *func_val)?;
                let result = call_indirect_function(builder, module, ctx, func_ptr, &arg_values)?;
                ctx.values.insert(*dst, result);
            }
        }

        IrOp::CallMethod(dst, _obj, name, args) => {
            // Special case: console.log
            if name == "log" && !args.is_empty() {
                // Get the first argument (the value to log) as a Cranelift Value
                let arg_val = get_value(ctx, args[0])?;

                // Call tscl_console_log with the argument (using call_stub_with_values since we have a Value)
                let result =
                    call_stub_with_values(builder, module, ctx, "tscl_console_log", &[arg_val])?;
                ctx.values.insert(*dst, result);
            } else {
                // Generic method call - for now, return undefined
                // TODO: Implement proper method call dispatch
                let undefined = translate_literal(builder, &Literal::Undefined);
                ctx.values.insert(*dst, undefined);
            }
        }

        IrOp::CallMono(dst, _mono_id, _args) => {
            // TODO: Implement monomorphized calls
            let undefined = translate_literal(builder, &Literal::Undefined);
            ctx.values.insert(*dst, undefined);
        }

        IrOp::MakeClosure(dst, addr, env) => {
            // Create a closure by packing the function address and environment
            let func_addr = builder.ins().iconst(types::I64, *addr as i64);
            let env_val = get_value(ctx, *env)?;

            // Call tscl_make_closure(func_addr, env) to create the closure object
            let result = call_stub_with_values(
                builder,
                module,
                ctx,
                "tscl_make_closure",
                &[func_addr, env_val],
            )?;

            ctx.values.insert(*dst, result);
        }

        IrOp::LoadThis(dst) => {
            // Load 'this' from first parameter or undefined
            let undefined = translate_literal(builder, &Literal::Undefined);
            ctx.values.insert(*dst, undefined);
        }

        // === Struct Operations ===
        IrOp::StructNew(dst, _struct_id) => {
            let result = call_stub_no_args(builder, module, ctx, "tscl_alloc_object")?;
            ctx.values.insert(*dst, result);
        }

        IrOp::StructGetField(dst, src, _field_id) => {
            // TODO: Use field offset for direct access
            let val = get_value(ctx, *src)?;
            ctx.values.insert(*dst, val);
        }

        IrOp::StructSetField(dst, _field_id, val) => {
            // TODO: Use field offset for direct access
            let _ = get_value(ctx, *dst)?;
            let _ = get_value(ctx, *val)?;
        }

        IrOp::StructGetFieldNamed(dst, src, _name) => {
            let val = get_value(ctx, *src)?;
            ctx.values.insert(*dst, val);
        }

        IrOp::StructSetFieldNamed(dst, _name, val) => {
            let _ = get_value(ctx, *dst)?;
            let _ = get_value(ctx, *val)?;
        }

        // Bitwise operations - not implemented in Cranelift backend yet
        IrOp::BitAnd(_, _, _)
        | IrOp::BitOr(_, _, _)
        | IrOp::Xor(_, _, _)
        | IrOp::Shl(_, _, _)
        | IrOp::Shr(_, _, _)
        | IrOp::ShrU(_, _, _)
        | IrOp::Pow(_, _, _) => {
            // TODO: Implement bitwise operations
            return Err(BackendError::UnsupportedOp(
                "Bitwise operations not yet implemented in Cranelift backend".to_string(),
            ));
        }

        // TypeOf - returns type string (not yet implemented)
        IrOp::TypeOf(dst, _val) => {
            // TODO: Implement typeof by calling a runtime stub
            let undefined = translate_literal(builder, &Literal::Undefined);
            ctx.values.insert(*dst, undefined);
        }

        // DeleteProp - deletes property from object (not yet implemented)
        IrOp::DeleteProp(dst, _obj, _prop) => {
            // TODO: Implement delete by calling a runtime stub
            // For now, return true (delete always "succeeds")
            let one = builder.ins().iconst(types::I8, 1);
            let true_val = bool_to_tscl_value(builder, one);
            ctx.values.insert(*dst, true_val);
        }
    }

    Ok(())
}

/// Translate a block terminator
fn translate_terminator(
    builder: &mut FunctionBuilder,
    ctx: &TranslationContext,
    term: &Terminator,
    current_block: BlockId,
) -> Result<(), BackendError> {
    match term {
        Terminator::Jump(target) => {
            let block = ctx.blocks[target];
            let phi_args = get_phi_args_for_jump(ctx, *target, current_block)?;
            builder.ins().jump(block, &phi_args);
        }

        Terminator::Branch(cond, true_block, false_block) => {
            let cond_val = get_value(ctx, *cond)?;
            // Check if truthy (not 0, not NaN, not undefined, etc.)
            // For simplicity, check if the boolean bit is set
            let is_truthy = tscl_value_to_bool(builder, cond_val);

            let true_bl = ctx.blocks[true_block];
            let false_bl = ctx.blocks[false_block];

            let true_args = get_phi_args_for_jump(ctx, *true_block, current_block)?;
            let false_args = get_phi_args_for_jump(ctx, *false_block, current_block)?;

            builder
                .ins()
                .brif(is_truthy, true_bl, &true_args, false_bl, &false_args);
        }

        Terminator::Return(val) => {
            let ret_val = match val {
                Some(v) => get_value(ctx, *v)?,
                None => translate_literal(builder, &Literal::Undefined),
            };
            builder.ins().return_(&[ret_val]);
        }

        Terminator::Unreachable => {
            builder
                .ins()
                .trap(TrapCode::user(1).expect("TrapCode::user(1) should always be valid"));
        }
    }

    Ok(())
}

/// Get the phi argument values for a jump to target_block from current_block.
fn get_phi_args_for_jump(
    ctx: &TranslationContext,
    target_block: BlockId,
    current_block: BlockId,
) -> Result<Vec<Value>, BackendError> {
    let mut args = Vec::new();

    if let Some(phis) = ctx.block_phis.get(&target_block) {
        for (_dst, entries) in phis {
            // Find the entry for the current block
            let value = entries
                .iter()
                .find(|(from_block, _)| *from_block == current_block)
                .map(|(_, val)| get_value(ctx, *val))
                .transpose()?
                .unwrap_or_else(|| translate_literal_static(&Literal::Undefined));

            args.push(value);
        }
    }

    Ok(args)
}

/// Translate a literal to a constant value (for use in contexts without builder)
fn translate_literal_static(lit: &Literal) -> Value {
    // This is a placeholder - we need a builder to create values
    // In practice, phi nodes should always have an entry for the current block
    panic!(
        "translate_literal_static called - phi node missing entry for current block. Literal: {:?}",
        lit
    );
}

/// Translate a literal to a Cranelift value
fn translate_literal(builder: &mut FunctionBuilder, lit: &Literal) -> Value {
    match lit {
        Literal::Number(n) => {
            let bits = n.to_bits();
            builder.ins().iconst(types::I64, bits as i64)
        }
        Literal::Boolean(b) => {
            // NaN-boxed boolean: QNAN | TAG_BOOLEAN | (b as u64)
            const QNAN: u64 = 0x7FFC_0000_0000_0000;
            const TAG_BOOLEAN: u64 = 0x0001_0000_0000_0000;
            let bits = QNAN | TAG_BOOLEAN | (*b as u64);
            builder.ins().iconst(types::I64, bits as i64)
        }
        Literal::Null => {
            const QNAN: u64 = 0x7FFC_0000_0000_0000;
            const TAG_NULL: u64 = 0x0002_0000_0000_0000;
            let bits = QNAN | TAG_NULL;
            builder.ins().iconst(types::I64, bits as i64)
        }
        Literal::Undefined => {
            const QNAN: u64 = 0x7FFC_0000_0000_0000;
            const TAG_UNDEFINED: u64 = 0x0003_0000_0000_0000;
            let bits = QNAN | TAG_UNDEFINED;
            builder.ins().iconst(types::I64, bits as i64)
        }
        Literal::String(_s) => {
            // TODO: Allocate string and return pointer
            // For now, return undefined
            const QNAN: u64 = 0x7FFC_0000_0000_0000;
            const TAG_UNDEFINED: u64 = 0x0003_0000_0000_0000;
            let bits = QNAN | TAG_UNDEFINED;
            builder.ins().iconst(types::I64, bits as i64)
        }
    }
}

/// Get a Cranelift value for an IR value
fn get_value(ctx: &TranslationContext, id: ValueId) -> Result<Value, BackendError> {
    ctx.values
        .get(&id)
        .copied()
        .ok_or_else(|| BackendError::Cranelift(format!("Undefined value: {:?}", id)))
}

/// Try to resolve a function address from a ValueId.
/// Returns Some(address) if the value is a constant function address.
fn resolve_function_address(ctx: &TranslationContext, func_val: ValueId) -> Option<usize> {
    // Check if this value is a known constant
    if let Some(lit) = ctx.constants.get(&func_val)
        && let Literal::Number(n) = lit
    {
        let addr = *n as usize;
        // Verify this is a known function
        if ctx.ir_module_ref.function_addrs.contains_key(&addr) {
            return Some(addr);
        }
    }

    None
}

/// Call a function indirectly using the tscl_call runtime stub.
fn call_indirect_function(
    builder: &mut FunctionBuilder,
    module: &mut JITModule,
    ctx: &mut TranslationContext,
    func_ptr: Value,
    args: &[Value],
) -> Result<Value, BackendError> {
    // For now, return undefined since tscl_call is not fully implemented
    // TODO: Implement proper indirect calls

    // Prepare arguments array if needed
    if args.is_empty() {
        // Simple case: no arguments
        let argc = builder.ins().iconst(types::I64, 0);
        let null_ptr = builder.ins().iconst(types::I64, 0);
        call_stub_with_values(
            builder,
            module,
            ctx,
            "tscl_call",
            &[func_ptr, argc, null_ptr],
        )
    } else {
        // For now, just return undefined for calls with arguments
        // Full implementation would set up argument array
        let undefined = translate_literal(builder, &Literal::Undefined);
        Ok(undefined)
    }
}

/// Call a runtime stub with IR value IDs as arguments
fn call_stub(
    builder: &mut FunctionBuilder,
    module: &mut JITModule,
    ctx: &mut TranslationContext,
    name: &str,
    args: &[ValueId],
) -> Result<Value, BackendError> {
    let arg_values: Vec<Value> = args
        .iter()
        .map(|id| get_value(ctx, *id))
        .collect::<Result<_, _>>()?;

    call_stub_with_values(builder, module, ctx, name, &arg_values)
}

/// Call a runtime stub with Cranelift values as arguments
fn call_stub_with_values(
    builder: &mut FunctionBuilder,
    module: &mut JITModule,
    ctx: &mut TranslationContext,
    name: &str,
    args: &[Value],
) -> Result<Value, BackendError> {
    // Get or declare the stub
    let func_ref = if let Some(&f) = ctx.stubs.get(name) {
        f
    } else {
        // Declare the function
        let mut sig = module.make_signature();
        for _ in args {
            sig.params.push(AbiParam::new(types::I64));
        }
        sig.returns.push(AbiParam::new(types::I64));

        let func_id = module
            .declare_function(name, Linkage::Import, &sig)
            .map_err(|e| {
                BackendError::Cranelift(format!("Failed to declare stub {}: {}", name, e))
            })?;

        let func_ref = module.declare_func_in_func(func_id, builder.func);
        ctx.stubs.insert(name.to_string(), func_ref);
        func_ref
    };

    let call = builder.ins().call(func_ref, args);
    let results = builder.inst_results(call);

    if results.is_empty() {
        return Err(BackendError::Cranelift(format!(
            "Call to stub {} returned empty result",
            name
        )));
    }

    let result = results[0];
    Ok(result)
}

/// Call a runtime stub with no arguments
fn call_stub_no_args(
    builder: &mut FunctionBuilder,
    module: &mut JITModule,
    ctx: &mut TranslationContext,
    name: &str,
) -> Result<Value, BackendError> {
    call_stub_with_values(builder, module, ctx, name, &[])
}

/// Convert a Cranelift boolean to a NaN-boxed boolean
fn bool_to_tscl_value(builder: &mut FunctionBuilder, b: Value) -> Value {
    const QNAN: u64 = 0x7FFC_0000_0000_0000;
    const TAG_BOOLEAN: u64 = 0x0001_0000_0000_0000;
    let base = builder
        .ins()
        .iconst(types::I64, (QNAN | TAG_BOOLEAN) as i64);
    let b_i64 = builder.ins().uextend(types::I64, b);
    builder.ins().bor(base, b_i64)
}

/// Convert a NaN-boxed boolean to a Cranelift boolean
fn tscl_value_to_bool(builder: &mut FunctionBuilder, val: Value) -> Value {
    // Check if the low bit is set (for booleans)
    // This is a simplified check - proper impl would check type tag
    let one = builder.ins().iconst(types::I64, 1);
    let masked = builder.ins().band(val, one);
    builder.ins().icmp_imm(IntCC::NotEqual, masked, 0)
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
}
