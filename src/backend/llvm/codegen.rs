//! Code generation: SSA IR → LLVM IR translation
//!
//! This module translates tscl SSA IR operations to LLVM IR instructions.

// Allow these for LLVM FFI code
#![allow(clippy::manual_c_str_literals)]
#![allow(clippy::missing_safety_doc)]

use llvm_sys::prelude::*;
use std::collections::{BTreeMap, HashMap};
use std::ffi::{CString, c_char};

use crate::backend::BackendError;
use crate::ir::{
    BasicBlock, BlockId, IrFunction, IrModule, IrOp, IrType, Literal, Terminator, ValueId,
};

use super::abi;
use super::types;

/// LLVM code generator for AOT compilation
pub struct LlvmCodegen {
    /// LLVM context
    pub context: LLVMContextRef,
    /// LLVM module being built
    pub module: LLVMModuleRef,
    /// Target triple
    pub target_triple: String,
    /// Declared runtime stubs (name -> function value)
    pub stubs: BTreeMap<String, LLVMValueRef>,
    /// Compiled function values (name -> function value)
    pub functions: BTreeMap<String, LLVMValueRef>,
}

impl LlvmCodegen {
    /// Create a new LLVM code generator
    pub fn new(target_triple: String) -> Result<Self, BackendError> {
        unsafe {
            // Initialize LLVM
            llvm_sys::target::LLVM_InitializeNativeTarget();
            llvm_sys::target::LLVM_InitializeNativeAsmPrinter();
            llvm_sys::target::LLVM_InitializeNativeAsmParser();

            // Create context
            let context = llvm_sys::core::LLVMContextCreate();
            if context.is_null() {
                return Err(BackendError::Llvm("Failed to create LLVM context".into()));
            }

            // Create module
            let module_name = CString::new("ot_module").unwrap();
            let module =
                llvm_sys::core::LLVMModuleCreateWithNameInContext(module_name.as_ptr(), context);
            if module.is_null() {
                llvm_sys::core::LLVMContextDispose(context);
                return Err(BackendError::Llvm("Failed to create LLVM module".into()));
            }

            // Set target triple
            let triple_cstr = CString::new(target_triple.clone()).unwrap();
            llvm_sys::core::LLVMSetTarget(module, triple_cstr.as_ptr());

            Ok(Self {
                context,
                module,
                target_triple,
                stubs: BTreeMap::new(),
                functions: BTreeMap::new(),
            })
        }
    }

    /// Compile an entire IR module
    pub fn compile_module(&mut self, ir_module: &IrModule) -> Result<(), BackendError> {
        unsafe {
            // Declare runtime stubs first
            abi::declare_runtime_stubs(self.module, self.context, &mut self.stubs)?;

            // Build struct types map
            let struct_types = self.build_struct_types(ir_module)?;

            // Declare all functions first (for inter-function calls)
            for func in &ir_module.functions {
                let func_name = if func.name.is_empty() {
                    "anonymous".to_string()
                } else {
                    func.name.clone()
                };
                self.declare_function(&func_name, func, &struct_types)?;
            }

            // Compile each function
            for func in &ir_module.functions {
                self.compile_function(func, ir_module, &struct_types)?;
            }

            // Create C-compatible main wrapper if tscl main exists
            let ot_main_name = std::ffi::CString::new("main").unwrap();
            let ot_main = llvm_sys::core::LLVMGetNamedFunction(self.module, ot_main_name.as_ptr());
            if !ot_main.is_null() && llvm_sys::core::LLVMIsDeclaration(ot_main) == 0 {
                // Rename tscl main to ot_main
                let ot_main_new_name = std::ffi::CString::new("ot_main").unwrap();
                llvm_sys::core::LLVMSetValueName2(
                    ot_main,
                    ot_main_new_name.as_ptr(),
                    ot_main_new_name.as_bytes().len(),
                );

                // Create C-compatible main: int main(int argc, char** argv)
                let i32_ty = llvm_sys::core::LLVMInt32TypeInContext(self.context);
                let i8_ty = llvm_sys::core::LLVMInt8TypeInContext(self.context);
                let i8_ptr_ty = llvm_sys::core::LLVMPointerType(i8_ty, 0);
                let i8_ptr_ptr_ty = llvm_sys::core::LLVMPointerType(i8_ptr_ty, 0);
                let mut main_params = vec![i32_ty, i8_ptr_ptr_ty];
                let main_ty = llvm_sys::core::LLVMFunctionType(
                    i32_ty,
                    main_params.as_mut_ptr(),
                    main_params.len() as u32,
                    0,
                );

                let main_name = std::ffi::CString::new("main").unwrap();
                let c_main =
                    llvm_sys::core::LLVMAddFunction(self.module, main_name.as_ptr(), main_ty);

                if !c_main.is_null() {
                    // Set visibility and linkage to prevent LTO from eliminating it
                    llvm_sys::core::LLVMSetVisibility(
                        c_main,
                        llvm_sys::LLVMVisibility::LLVMDefaultVisibility,
                    );
                    llvm_sys::core::LLVMSetLinkage(
                        c_main,
                        llvm_sys::LLVMLinkage::LLVMExternalLinkage,
                    );

                    // Mark as used to prevent LTO dead code elimination
                    let i8_ty = llvm_sys::core::LLVMInt8TypeInContext(self.context);
                    let i8_ptr_ty = llvm_sys::core::LLVMPointerType(i8_ty, 0);
                    let used_array_ty = llvm_sys::core::LLVMArrayType2(i8_ptr_ty, 1);
                    let used_name = std::ffi::CString::new("llvm.used").unwrap();

                    let existing_used =
                        llvm_sys::core::LLVMGetNamedGlobal(self.module, used_name.as_ptr());
                    let used_global = if !existing_used.is_null() {
                        existing_used
                    } else {
                        llvm_sys::core::LLVMAddGlobal(
                            self.module,
                            used_array_ty,
                            used_name.as_ptr(),
                        )
                    };
                    if !used_global.is_null() {
                        let main_as_i8 = llvm_sys::core::LLVMConstBitCast(c_main, i8_ptr_ty);
                        let mut main_ptr = main_as_i8;
                        let used_array =
                            llvm_sys::core::LLVMConstArray2(i8_ptr_ty, &mut main_ptr, 1);
                        llvm_sys::core::LLVMSetInitializer(used_global, used_array);
                        llvm_sys::core::LLVMSetLinkage(
                            used_global,
                            llvm_sys::LLVMLinkage::LLVMAppendingLinkage,
                        );
                        llvm_sys::core::LLVMSetSection(
                            used_global,
                            b"llvm.metadata\0".as_ptr() as *const c_char,
                        );
                    }

                    // Create entry block and call ot_main
                    let entry = llvm_sys::core::LLVMAppendBasicBlock(
                        c_main,
                        b"entry\0".as_ptr() as *const c_char,
                    );
                    let builder = llvm_sys::core::LLVMCreateBuilderInContext(self.context);
                    llvm_sys::core::LLVMPositionBuilderAtEnd(builder, entry);

                    let ot_main_ty = llvm_sys::core::LLVMGlobalGetValueType(ot_main);
                    let _ot_result = llvm_sys::core::LLVMBuildCall2(
                        builder,
                        ot_main_ty,
                        ot_main,
                        std::ptr::null_mut(),
                        0,
                        b"call\0".as_ptr() as *const c_char,
                    );

                    // If there's a user-defined main() function, call it
                    if let Some(user_main_addr) = ir_module.user_main_addr {
                        let user_main_name =
                            std::ffi::CString::new(format!("func_{}", user_main_addr)).unwrap();
                        let user_main_func = llvm_sys::core::LLVMGetNamedFunction(
                            self.module,
                            user_main_name.as_ptr(),
                        );
                        if !user_main_func.is_null() {
                            let i64_ty = llvm_sys::core::LLVMInt64TypeInContext(self.context);
                            let user_main_ty = llvm_sys::core::LLVMFunctionType(
                                i64_ty,
                                std::ptr::null_mut(),
                                0,
                                0,
                            );
                            let _user_result = llvm_sys::core::LLVMBuildCall2(
                                builder,
                                user_main_ty,
                                user_main_func,
                                std::ptr::null_mut(),
                                0,
                                b"user_main_call\0".as_ptr() as *const c_char,
                            );
                        }
                    }

                    // Return 0 (success)
                    let zero = llvm_sys::core::LLVMConstInt(i32_ty, 0, 0);
                    llvm_sys::core::LLVMBuildRet(builder, zero);

                    llvm_sys::core::LLVMDisposeBuilder(builder);
                }
            }

            Ok(())
        }
    }

    unsafe fn declare_function(
        &mut self,
        name: &str,
        func: &IrFunction,
        _struct_types: &BTreeMap<u32, LLVMTypeRef>,
    ) -> Result<(), BackendError> {
        unsafe {
            // All function parameters and return values are i64 (NaN-boxed)
            let i64_ty = llvm_sys::core::LLVMInt64TypeInContext(self.context);
            let mut param_types = Vec::new();
            for _ in &func.params {
                param_types.push(i64_ty);
            }

            // Return type is always i64 (unless void)
            let return_ty = if matches!(
                func.return_ty,
                crate::ir::IrType::Void | crate::ir::IrType::Never
            ) {
                llvm_sys::core::LLVMVoidTypeInContext(self.context)
            } else {
                i64_ty
            };
            let func_ty = llvm_sys::core::LLVMFunctionType(
                return_ty,
                param_types.as_mut_ptr(),
                param_types.len() as u32,
                0,
            );

            // Create function
            let name_cstr = CString::new(name).unwrap();
            let func_val =
                llvm_sys::core::LLVMAddFunction(self.module, name_cstr.as_ptr(), func_ty);

            if func_val.is_null() {
                return Err(BackendError::Llvm(format!(
                    "Failed to declare function: {}",
                    name
                )));
            }

            // Set visibility: hidden for internal functions, default for main and runtime stubs
            // This enables LTO to eliminate unused code
            if name == "main" {
                // Main must be visible for linking
                llvm_sys::core::LLVMSetVisibility(
                    func_val,
                    llvm_sys::LLVMVisibility::LLVMDefaultVisibility,
                );
            } else {
                // All other user functions are hidden (can be eliminated by LTO if unused)
                llvm_sys::core::LLVMSetVisibility(
                    func_val,
                    llvm_sys::LLVMVisibility::LLVMHiddenVisibility,
                );
            }

            self.functions.insert(name.to_string(), func_val);
            Ok(())
        }
    }

    unsafe fn compile_function(
        &mut self,
        func: &IrFunction,
        ir_module: &IrModule,
        struct_types: &BTreeMap<u32, LLVMTypeRef>,
    ) -> Result<(), BackendError> {
        unsafe {
            let func_name = if func.name.is_empty() {
                "anonymous".to_string()
            } else {
                func.name.clone()
            };

            let func_val = *self.functions.get(&func_name).ok_or_else(|| {
                BackendError::Llvm(format!("Function {} not declared", func_name))
            })?;

            // Create builder
            let builder = llvm_sys::core::LLVMCreateBuilderInContext(self.context);
            if builder.is_null() {
                return Err(BackendError::Llvm(format!(
                    "Failed to create builder for function {}",
                    func_name
                )));
            }

            // Create all blocks first
            let mut ctx = TranslationContext {
                values: HashMap::new(),
                blocks: HashMap::new(),
                locals: Vec::new(),
                builder,
                func_val,
                context: self.context,
                module: self.module,
                struct_types: struct_types.clone(),
                stubs: &self.stubs,
                functions: &self.functions,
                function_addrs: &ir_module.function_addrs,
                return_ty: func.return_ty.clone(),
            };

            // Create blocks for all IR blocks
            for block in &func.blocks {
                let block_name = format!("bb{}\0", block.id.0);
                let llvm_block = llvm_sys::core::LLVMAppendBasicBlock(
                    func_val,
                    block_name.as_ptr() as *const c_char,
                );
                if llvm_block.is_null() {
                    llvm_sys::core::LLVMDisposeBuilder(builder);
                    return Err(BackendError::Llvm(format!(
                        "Failed to create block bb{}",
                        block.id.0
                    )));
                }
                ctx.blocks.insert(block.id, llvm_block);
            }

            // Allocate stack slots for locals in the entry block (first block)
            if let Some(entry_block_id) = func.blocks.first().map(|b| b.id) {
                llvm_sys::core::LLVMPositionBuilderAtEnd(ctx.builder, ctx.blocks[&entry_block_id]);

                // All locals are stored as i64 (NaN-boxed)
                let i64_ty = llvm_sys::core::LLVMInt64TypeInContext(self.context);
                for _ in &func.locals {
                    let alloca = llvm_sys::core::LLVMBuildAlloca(
                        ctx.builder,
                        i64_ty,
                        b"local\0".as_ptr() as *const c_char,
                    );
                    ctx.locals.push(alloca);
                }

                // Map function parameters to values in entry block
                let param_count = func.params.len();
                for i in 0..param_count {
                    let param = llvm_sys::core::LLVMGetParam(func_val, i as u32);
                    ctx.values.insert(ValueId(i as u32), param);
                }
            }

            // Translate each block
            for block in &func.blocks {
                llvm_sys::core::LLVMPositionBuilderAtEnd(ctx.builder, ctx.blocks[&block.id]);
                translate_block(&mut ctx, block, ir_module)?;
            }

            // Verify function
            llvm_sys::analysis::LLVMVerifyFunction(
                func_val,
                llvm_sys::analysis::LLVMVerifierFailureAction::LLVMReturnStatusAction,
            );

            llvm_sys::core::LLVMDisposeBuilder(builder);
            Ok(())
        }
    }

    unsafe fn build_struct_types(
        &self,
        ir_module: &IrModule,
    ) -> Result<BTreeMap<u32, LLVMTypeRef>, BackendError> {
        let mut struct_types = BTreeMap::new();

        // Build struct types recursively
        for (id, struct_def) in &ir_module.structs {
            let struct_ty = types::create_struct_type(self.context, struct_def, &struct_types)?;
            struct_types.insert(id.0, struct_ty);
        }

        Ok(struct_types)
    }
}

impl Drop for LlvmCodegen {
    fn drop(&mut self) {
        unsafe {
            llvm_sys::core::LLVMDisposeModule(self.module);
            llvm_sys::core::LLVMContextDispose(self.context);
        }
    }
}

/// Translation context for compiling a function
#[allow(dead_code)]
struct TranslationContext<'a> {
    /// Map from IR ValueId to LLVM Value (internal lookup, doesn't affect output order)
    values: HashMap<ValueId, LLVMValueRef>,
    /// Map from IR BlockId to LLVM BasicBlock (internal lookup, doesn't affect output order)
    blocks: HashMap<BlockId, LLVMBasicBlockRef>,
    /// Stack slots for local variables (slot index -> alloca)
    locals: Vec<LLVMValueRef>,
    /// LLVM builder
    builder: LLVMBuilderRef,
    /// Current function being built
    func_val: LLVMValueRef,
    /// LLVM context
    context: LLVMContextRef,
    /// LLVM module
    module: LLVMModuleRef,
    /// Struct type map (deterministic ordering for struct declarations)
    struct_types: BTreeMap<u32, LLVMTypeRef>,
    /// Runtime stubs (deterministic ordering for stub declarations)
    stubs: &'a BTreeMap<String, LLVMValueRef>,
    /// Compiled functions (deterministic ordering for function declarations)
    functions: &'a BTreeMap<String, LLVMValueRef>,
    /// Bytecode address to function name mapping (internal lookup)
    function_addrs: &'a HashMap<usize, usize>,
    /// Function return type (for handling Return(None) correctly)
    return_ty: IrType,
}

/// Translate a basic block
unsafe fn translate_block(
    ctx: &mut TranslationContext,
    block: &BasicBlock,
    _ir_module: &IrModule,
) -> Result<(), BackendError> {
    unsafe {
        // Translate operations
        for op in &block.ops {
            translate_op(ctx, op)?;
        }

        // Translate terminator
        translate_terminator(ctx, &block.terminator)?;
        Ok(())
    }
}

/// Translate a single IR operation
unsafe fn translate_op(ctx: &mut TranslationContext, op: &IrOp) -> Result<(), BackendError> {
    unsafe {
        match op {
            IrOp::Const(dst, lit) => {
                // Check if this is a function address constant
                if let Literal::Number(n) = lit {
                    let addr = *n as usize;
                    // If this address maps to a known function, emit the function pointer
                    if ctx.function_addrs.contains_key(&addr) {
                        let func_name = format!("func_{}", addr);
                        if let Some(&func_ptr) = ctx.functions.get(&func_name) {
                            // Convert function pointer to i64 (for NaN-boxing compatibility)
                            let i64_ty = llvm_sys::core::LLVMInt64TypeInContext(ctx.context);
                            let ptr_as_int = llvm_sys::core::LLVMBuildPtrToInt(
                                ctx.builder,
                                func_ptr,
                                i64_ty,
                                b"func_addr\0".as_ptr() as *const c_char,
                            );
                            ctx.values.insert(*dst, ptr_as_int);
                            return Ok(());
                        }
                    }
                }
                let val = translate_literal(ctx, lit)?;
                ctx.values.insert(*dst, val);
            }
            IrOp::AddNum(dst, a, b) => {
                let va = get_value(ctx, *a)?;
                let vb = get_value(ctx, *b)?;
                // Bitcast i64 to double for operation
                let double_ty = llvm_sys::core::LLVMDoubleTypeInContext(ctx.context);
                let i64_ty = llvm_sys::core::LLVMInt64TypeInContext(ctx.context);
                let fa = llvm_sys::core::LLVMBuildBitCast(
                    ctx.builder,
                    va,
                    double_ty,
                    b"bitcast\0".as_ptr() as *const c_char,
                );
                let fb = llvm_sys::core::LLVMBuildBitCast(
                    ctx.builder,
                    vb,
                    double_ty,
                    b"bitcast\0".as_ptr() as *const c_char,
                );
                let result_fp = llvm_sys::core::LLVMBuildFAdd(
                    ctx.builder,
                    fa,
                    fb,
                    b"add\0".as_ptr() as *const c_char,
                );
                // Bitcast back to i64 (NaN-boxed)
                let result = llvm_sys::core::LLVMBuildBitCast(
                    ctx.builder,
                    result_fp,
                    i64_ty,
                    b"bitcast\0".as_ptr() as *const c_char,
                );
                ctx.values.insert(*dst, result);
            }
            IrOp::SubNum(dst, a, b) => {
                let va = get_value(ctx, *a)?;
                let vb = get_value(ctx, *b)?;
                let double_ty = llvm_sys::core::LLVMDoubleTypeInContext(ctx.context);
                let i64_ty = llvm_sys::core::LLVMInt64TypeInContext(ctx.context);
                let fa = llvm_sys::core::LLVMBuildBitCast(
                    ctx.builder,
                    va,
                    double_ty,
                    b"bitcast\0".as_ptr() as *const c_char,
                );
                let fb = llvm_sys::core::LLVMBuildBitCast(
                    ctx.builder,
                    vb,
                    double_ty,
                    b"bitcast\0".as_ptr() as *const c_char,
                );
                let result_fp = llvm_sys::core::LLVMBuildFSub(
                    ctx.builder,
                    fa,
                    fb,
                    b"sub\0".as_ptr() as *const c_char,
                );
                let result = llvm_sys::core::LLVMBuildBitCast(
                    ctx.builder,
                    result_fp,
                    i64_ty,
                    b"bitcast\0".as_ptr() as *const c_char,
                );
                ctx.values.insert(*dst, result);
            }
            IrOp::MulNum(dst, a, b) => {
                let va = get_value(ctx, *a)?;
                let vb = get_value(ctx, *b)?;
                let double_ty = llvm_sys::core::LLVMDoubleTypeInContext(ctx.context);
                let i64_ty = llvm_sys::core::LLVMInt64TypeInContext(ctx.context);
                let fa = llvm_sys::core::LLVMBuildBitCast(
                    ctx.builder,
                    va,
                    double_ty,
                    b"bitcast\0".as_ptr() as *const c_char,
                );
                let fb = llvm_sys::core::LLVMBuildBitCast(
                    ctx.builder,
                    vb,
                    double_ty,
                    b"bitcast\0".as_ptr() as *const c_char,
                );
                let result_fp = llvm_sys::core::LLVMBuildFMul(
                    ctx.builder,
                    fa,
                    fb,
                    b"mul\0".as_ptr() as *const c_char,
                );
                let result = llvm_sys::core::LLVMBuildBitCast(
                    ctx.builder,
                    result_fp,
                    i64_ty,
                    b"bitcast\0".as_ptr() as *const c_char,
                );
                ctx.values.insert(*dst, result);
            }
            IrOp::DivNum(dst, a, b) => {
                let va = get_value(ctx, *a)?;
                let vb = get_value(ctx, *b)?;
                let double_ty = llvm_sys::core::LLVMDoubleTypeInContext(ctx.context);
                let i64_ty = llvm_sys::core::LLVMInt64TypeInContext(ctx.context);
                let fa = llvm_sys::core::LLVMBuildBitCast(
                    ctx.builder,
                    va,
                    double_ty,
                    b"bitcast\0".as_ptr() as *const c_char,
                );
                let fb = llvm_sys::core::LLVMBuildBitCast(
                    ctx.builder,
                    vb,
                    double_ty,
                    b"bitcast\0".as_ptr() as *const c_char,
                );
                let result_fp = llvm_sys::core::LLVMBuildFDiv(
                    ctx.builder,
                    fa,
                    fb,
                    b"div\0".as_ptr() as *const c_char,
                );
                let result = llvm_sys::core::LLVMBuildBitCast(
                    ctx.builder,
                    result_fp,
                    i64_ty,
                    b"bitcast\0".as_ptr() as *const c_char,
                );
                ctx.values.insert(*dst, result);
            }
            IrOp::ModNum(dst, a, b) => {
                let va = get_value(ctx, *a)?;
                let vb = get_value(ctx, *b)?;
                let double_ty = llvm_sys::core::LLVMDoubleTypeInContext(ctx.context);
                let i64_ty = llvm_sys::core::LLVMInt64TypeInContext(ctx.context);
                let fa = llvm_sys::core::LLVMBuildBitCast(
                    ctx.builder,
                    va,
                    double_ty,
                    b"bitcast\0".as_ptr() as *const c_char,
                );
                let fb = llvm_sys::core::LLVMBuildBitCast(
                    ctx.builder,
                    vb,
                    double_ty,
                    b"bitcast\0".as_ptr() as *const c_char,
                );
                let result_fp = llvm_sys::core::LLVMBuildFRem(
                    ctx.builder,
                    fa,
                    fb,
                    b"mod\0".as_ptr() as *const c_char,
                );
                let result = llvm_sys::core::LLVMBuildBitCast(
                    ctx.builder,
                    result_fp,
                    i64_ty,
                    b"bitcast\0".as_ptr() as *const c_char,
                );
                ctx.values.insert(*dst, result);
            }
            IrOp::NegNum(dst, a) => {
                let va = get_value(ctx, *a)?;
                let double_ty = llvm_sys::core::LLVMDoubleTypeInContext(ctx.context);
                let i64_ty = llvm_sys::core::LLVMInt64TypeInContext(ctx.context);
                let fa = llvm_sys::core::LLVMBuildBitCast(
                    ctx.builder,
                    va,
                    double_ty,
                    b"bitcast\0".as_ptr() as *const c_char,
                );
                let zero = llvm_sys::core::LLVMConstReal(double_ty, 0.0);
                let result_fp = llvm_sys::core::LLVMBuildFSub(
                    ctx.builder,
                    zero,
                    fa,
                    b"neg\0".as_ptr() as *const c_char,
                );
                let result = llvm_sys::core::LLVMBuildBitCast(
                    ctx.builder,
                    result_fp,
                    i64_ty,
                    b"bitcast\0".as_ptr() as *const c_char,
                );
                ctx.values.insert(*dst, result);
            }
            IrOp::LoadLocal(dst, slot) => {
                if let Some(&alloca) = ctx.locals.get(*slot as usize) {
                    // All locals are i64 (NaN-boxed)
                    let i64_ty = llvm_sys::core::LLVMInt64TypeInContext(ctx.context);
                    let val = llvm_sys::core::LLVMBuildLoad2(
                        ctx.builder,
                        i64_ty,
                        alloca,
                        b"load\0".as_ptr() as *const c_char,
                    );
                    ctx.values.insert(*dst, val);
                } else {
                    return Err(BackendError::Llvm(format!("Invalid local slot: {}", slot)));
                }
            }
            IrOp::StoreLocal(slot, src) => {
                if let Some(&alloca) = ctx.locals.get(*slot as usize) {
                    let val = get_value(ctx, *src)?;
                    llvm_sys::core::LLVMBuildStore(ctx.builder, val, alloca);
                } else {
                    return Err(BackendError::Llvm(format!("Invalid local slot: {}", slot)));
                }
            }
            IrOp::Lt(dst, a, b) => {
                let va = get_value(ctx, *a)?;
                let vb = get_value(ctx, *b)?;
                let double_ty = llvm_sys::core::LLVMDoubleTypeInContext(ctx.context);
                let fa = llvm_sys::core::LLVMBuildBitCast(
                    ctx.builder,
                    va,
                    double_ty,
                    b"bitcast\0".as_ptr() as *const c_char,
                );
                let fb = llvm_sys::core::LLVMBuildBitCast(
                    ctx.builder,
                    vb,
                    double_ty,
                    b"bitcast\0".as_ptr() as *const c_char,
                );
                let cmp = llvm_sys::core::LLVMBuildFCmp(
                    ctx.builder,
                    llvm_sys::LLVMRealPredicate::LLVMRealOLT,
                    fa,
                    fb,
                    b"cmp\0".as_ptr() as *const c_char,
                );
                let result = bool_to_ot_value(ctx, cmp)?;
                ctx.values.insert(*dst, result);
            }
            IrOp::LtEq(dst, a, b) => {
                let va = get_value(ctx, *a)?;
                let vb = get_value(ctx, *b)?;
                let double_ty = llvm_sys::core::LLVMDoubleTypeInContext(ctx.context);
                let fa = llvm_sys::core::LLVMBuildBitCast(
                    ctx.builder,
                    va,
                    double_ty,
                    b"bitcast\0".as_ptr() as *const c_char,
                );
                let fb = llvm_sys::core::LLVMBuildBitCast(
                    ctx.builder,
                    vb,
                    double_ty,
                    b"bitcast\0".as_ptr() as *const c_char,
                );
                let cmp = llvm_sys::core::LLVMBuildFCmp(
                    ctx.builder,
                    llvm_sys::LLVMRealPredicate::LLVMRealOLE,
                    fa,
                    fb,
                    b"cmp\0".as_ptr() as *const c_char,
                );
                let result = bool_to_ot_value(ctx, cmp)?;
                ctx.values.insert(*dst, result);
            }
            IrOp::Gt(dst, a, b) => {
                let va = get_value(ctx, *a)?;
                let vb = get_value(ctx, *b)?;
                let double_ty = llvm_sys::core::LLVMDoubleTypeInContext(ctx.context);
                let fa = llvm_sys::core::LLVMBuildBitCast(
                    ctx.builder,
                    va,
                    double_ty,
                    b"bitcast\0".as_ptr() as *const c_char,
                );
                let fb = llvm_sys::core::LLVMBuildBitCast(
                    ctx.builder,
                    vb,
                    double_ty,
                    b"bitcast\0".as_ptr() as *const c_char,
                );
                let cmp = llvm_sys::core::LLVMBuildFCmp(
                    ctx.builder,
                    llvm_sys::LLVMRealPredicate::LLVMRealOGT,
                    fa,
                    fb,
                    b"cmp\0".as_ptr() as *const c_char,
                );
                let result = bool_to_ot_value(ctx, cmp)?;
                ctx.values.insert(*dst, result);
            }
            IrOp::GtEq(dst, a, b) => {
                let va = get_value(ctx, *a)?;
                let vb = get_value(ctx, *b)?;
                let double_ty = llvm_sys::core::LLVMDoubleTypeInContext(ctx.context);
                let fa = llvm_sys::core::LLVMBuildBitCast(
                    ctx.builder,
                    va,
                    double_ty,
                    b"bitcast\0".as_ptr() as *const c_char,
                );
                let fb = llvm_sys::core::LLVMBuildBitCast(
                    ctx.builder,
                    vb,
                    double_ty,
                    b"bitcast\0".as_ptr() as *const c_char,
                );
                let cmp = llvm_sys::core::LLVMBuildFCmp(
                    ctx.builder,
                    llvm_sys::LLVMRealPredicate::LLVMRealOGE,
                    fa,
                    fb,
                    b"cmp\0".as_ptr() as *const c_char,
                );
                let result = bool_to_ot_value(ctx, cmp)?;
                ctx.values.insert(*dst, result);
            }
            IrOp::EqStrict(dst, a, b) => {
                // For strict equality, compare the NaN-boxed i64 values directly
                // This works because identical values have identical bit patterns
                let va = get_value(ctx, *a)?;
                let vb = get_value(ctx, *b)?;
                let double_ty = llvm_sys::core::LLVMDoubleTypeInContext(ctx.context);
                let fa = llvm_sys::core::LLVMBuildBitCast(
                    ctx.builder,
                    va,
                    double_ty,
                    b"bitcast\0".as_ptr() as *const c_char,
                );
                let fb = llvm_sys::core::LLVMBuildBitCast(
                    ctx.builder,
                    vb,
                    double_ty,
                    b"bitcast\0".as_ptr() as *const c_char,
                );
                let cmp = llvm_sys::core::LLVMBuildFCmp(
                    ctx.builder,
                    llvm_sys::LLVMRealPredicate::LLVMRealOEQ,
                    fa,
                    fb,
                    b"cmp\0".as_ptr() as *const c_char,
                );
                let result = bool_to_ot_value(ctx, cmp)?;
                ctx.values.insert(*dst, result);
            }
            IrOp::NeStrict(dst, a, b) => {
                // For strict inequality, compare the NaN-boxed i64 values directly
                let va = get_value(ctx, *a)?;
                let vb = get_value(ctx, *b)?;
                let double_ty = llvm_sys::core::LLVMDoubleTypeInContext(ctx.context);
                let fa = llvm_sys::core::LLVMBuildBitCast(
                    ctx.builder,
                    va,
                    double_ty,
                    b"bitcast\0".as_ptr() as *const c_char,
                );
                let fb = llvm_sys::core::LLVMBuildBitCast(
                    ctx.builder,
                    vb,
                    double_ty,
                    b"bitcast\0".as_ptr() as *const c_char,
                );
                let cmp = llvm_sys::core::LLVMBuildFCmp(
                    ctx.builder,
                    llvm_sys::LLVMRealPredicate::LLVMRealONE,
                    fa,
                    fb,
                    b"cmp\0".as_ptr() as *const c_char,
                );
                let result = bool_to_ot_value(ctx, cmp)?;
                ctx.values.insert(*dst, result);
            }
            IrOp::Not(dst, a) => {
                // Extract the boolean value and negate it
                let va = get_value(ctx, *a)?;
                let i64_ty = llvm_sys::core::LLVMInt64TypeInContext(ctx.context);
                let _i1_ty = llvm_sys::core::LLVMInt1TypeInContext(ctx.context);
                let one = llvm_sys::core::LLVMConstInt(i64_ty, 1, 0);
                // Extract low bit (the boolean value)
                let masked = llvm_sys::core::LLVMBuildAnd(
                    ctx.builder,
                    va,
                    one,
                    b"mask\0".as_ptr() as *const c_char,
                );
                // Check if it's zero (falsy)
                let is_falsy = llvm_sys::core::LLVMBuildICmp(
                    ctx.builder,
                    llvm_sys::LLVMIntPredicate::LLVMIntEQ,
                    masked,
                    llvm_sys::core::LLVMConstInt(i64_ty, 0, 0),
                    b"is_falsy\0".as_ptr() as *const c_char,
                );
                let result = bool_to_ot_value(ctx, is_falsy)?;
                ctx.values.insert(*dst, result);
            }
            IrOp::Copy(dst, src)
            | IrOp::Move(dst, src)
            | IrOp::Borrow(dst, src)
            | IrOp::BorrowMut(dst, src) => {
                let val = get_value(ctx, *src)?;
                ctx.values.insert(*dst, val);
            }
            IrOp::AddAny(dst, a, b) => {
                let va = get_value(ctx, *a)?;
                let vb = get_value(ctx, *b)?;
                let result = call_stub(ctx, "ot_add_any", &[va, vb])?;
                ctx.values.insert(*dst, result);
            }
            IrOp::SubAny(dst, a, b) => {
                let va = get_value(ctx, *a)?;
                let vb = get_value(ctx, *b)?;
                let result = call_stub(ctx, "ot_sub_any", &[va, vb])?;
                ctx.values.insert(*dst, result);
            }
            IrOp::MulAny(dst, a, b) => {
                let va = get_value(ctx, *a)?;
                let vb = get_value(ctx, *b)?;
                let result = call_stub(ctx, "ot_mul_any", &[va, vb])?;
                ctx.values.insert(*dst, result);
            }
            IrOp::DivAny(dst, a, b) => {
                let va = get_value(ctx, *a)?;
                let vb = get_value(ctx, *b)?;
                let result = call_stub(ctx, "ot_div_any", &[va, vb])?;
                ctx.values.insert(*dst, result);
            }
            IrOp::ModAny(dst, a, b) => {
                let va = get_value(ctx, *a)?;
                let vb = get_value(ctx, *b)?;
                let result = call_stub(ctx, "ot_mod_any", &[va, vb])?;
                ctx.values.insert(*dst, result);
            }
            IrOp::NegAny(dst, a) => {
                let va = get_value(ctx, *a)?;
                let result = call_stub(ctx, "ot_neg", &[va])?;
                ctx.values.insert(*dst, result);
            }
            IrOp::NewObject(dst) => {
                let result = call_stub(ctx, "ot_alloc_object", &[])?;
                ctx.values.insert(*dst, result);
            }
            IrOp::GetProp(dst, obj, _name) => {
                let obj_val = get_value(ctx, *obj)?;
                let result = call_stub(ctx, "ot_get_prop", &[obj_val])?;
                ctx.values.insert(*dst, result);
            }
            IrOp::SetProp(obj, _name, val) => {
                let obj_val = get_value(ctx, *obj)?;
                let val_val = get_value(ctx, *val)?;
                call_stub(ctx, "ot_set_prop", &[obj_val, val_val])?;
            }
            IrOp::GetElement(dst, obj, idx) => {
                let obj_val = get_value(ctx, *obj)?;
                let idx_val = get_value(ctx, *idx)?;
                let result = call_stub(ctx, "ot_get_element", &[obj_val, idx_val])?;
                ctx.values.insert(*dst, result);
            }
            IrOp::SetElement(obj, idx, val) => {
                let obj_val = get_value(ctx, *obj)?;
                let idx_val = get_value(ctx, *idx)?;
                let val_val = get_value(ctx, *val)?;
                call_stub(ctx, "ot_set_element", &[obj_val, idx_val, val_val])?;
            }
            IrOp::NewArray(dst) => {
                let capacity = llvm_sys::core::LLVMConstInt(
                    llvm_sys::core::LLVMInt64TypeInContext(ctx.context),
                    8,
                    0,
                );
                let result = call_stub(ctx, "ot_alloc_array", &[capacity])?;
                ctx.values.insert(*dst, result);
            }
            IrOp::Phi(dst, _entries) => {
                if !ctx.values.contains_key(dst) {
                    let undefined = translate_literal(ctx, &Literal::Undefined)?;
                    ctx.values.insert(*dst, undefined);
                }
            }
            IrOp::Call(dst, func_val, args) => {
                let func_ptr = get_value(ctx, *func_val)?;
                let arg_values: Vec<LLVMValueRef> = args
                    .iter()
                    .map(|id| get_value(ctx, *id))
                    .collect::<Result<_, _>>()?;
                let result = call_indirect(ctx, func_ptr, &arg_values)?;
                ctx.values.insert(*dst, result);
            }
            IrOp::CallMethod(dst, _obj, name, args) => {
                if name == "log" && !args.is_empty() {
                    let arg_val = get_value(ctx, args[0])?;
                    let result = call_stub(ctx, "ot_console_log", &[arg_val])?;
                    ctx.values.insert(*dst, result);
                } else {
                    let undefined = translate_literal(ctx, &Literal::Undefined)?;
                    ctx.values.insert(*dst, undefined);
                }
            }
            IrOp::MakeClosure(dst, addr, env) => {
                let func_addr = llvm_sys::core::LLVMConstInt(
                    llvm_sys::core::LLVMInt64TypeInContext(ctx.context),
                    *addr as u64,
                    0,
                );
                let env_val = get_value(ctx, *env)?;
                let result = call_stub(ctx, "ot_make_closure", &[func_addr, env_val])?;
                ctx.values.insert(*dst, result);
            }
            _ => {
                return Err(BackendError::UnsupportedOp(format!(
                    "Operation not yet implemented: {:?}",
                    op
                )));
            }
        }
        Ok(())
    }
}

/// Translate a terminator
unsafe fn translate_terminator(
    ctx: &mut TranslationContext,
    term: &Terminator,
) -> Result<(), BackendError> {
    unsafe {
        match term {
            Terminator::Jump(target) => {
                let target_block = ctx.blocks[target];
                llvm_sys::core::LLVMBuildBr(ctx.builder, target_block);
            }
            Terminator::Branch(cond, true_block, false_block) => {
                let cond_val = get_value(ctx, *cond)?;
                // Extract boolean from NaN-boxed value (check low bit)
                let _i1_ty = llvm_sys::core::LLVMInt1TypeInContext(ctx.context);
                let i64_ty = llvm_sys::core::LLVMInt64TypeInContext(ctx.context);
                let one = llvm_sys::core::LLVMConstInt(i64_ty, 1, 0);
                let masked = llvm_sys::core::LLVMBuildAnd(
                    ctx.builder,
                    cond_val,
                    one,
                    b"mask\0".as_ptr() as *const c_char,
                );
                let bool_val = llvm_sys::core::LLVMBuildICmp(
                    ctx.builder,
                    llvm_sys::LLVMIntPredicate::LLVMIntNE,
                    masked,
                    llvm_sys::core::LLVMConstInt(i64_ty, 0, 0),
                    b"bool\0".as_ptr() as *const c_char,
                );
                let true_block = ctx.blocks[true_block];
                let false_block = ctx.blocks[false_block];
                llvm_sys::core::LLVMBuildCondBr(ctx.builder, bool_val, true_block, false_block);
            }
            Terminator::Return(val) => {
                if let Some(v) = val {
                    let ret_val = get_value(ctx, *v)?;
                    llvm_sys::core::LLVMBuildRet(ctx.builder, ret_val);
                } else {
                    // Check if the function is declared as returning void
                    if matches!(ctx.return_ty, IrType::Void | IrType::Never) {
                        llvm_sys::core::LLVMBuildRetVoid(ctx.builder);
                    } else {
                        // Non-void function with no return value: return undefined (NaN-boxed)
                        let i64_ty = llvm_sys::core::LLVMInt64TypeInContext(ctx.context);
                        let qnan = 0x7FFC_0000_0000_0000u64;
                        let tag_undefined = 0x0003_0000_0000_0000u64;
                        let undefined_val =
                            llvm_sys::core::LLVMConstInt(i64_ty, qnan | tag_undefined, 0);
                        llvm_sys::core::LLVMBuildRet(ctx.builder, undefined_val);
                    }
                }
            }
            Terminator::Unreachable => {
                llvm_sys::core::LLVMBuildUnreachable(ctx.builder);
            }
        }
        Ok(())
    }
}

/// Translate a literal to an LLVM constant
unsafe fn translate_literal(
    ctx: &TranslationContext,
    lit: &Literal,
) -> Result<LLVMValueRef, BackendError> {
    unsafe {
        match lit {
            Literal::Number(n) => {
                // NaN-box the number: bitcast double to i64
                let double_ty = llvm_sys::core::LLVMDoubleTypeInContext(ctx.context);
                let double_const = llvm_sys::core::LLVMConstReal(double_ty, *n);
                let i64_ty = llvm_sys::core::LLVMInt64TypeInContext(ctx.context);
                // Use bitcast to convert double to i64 (NaN-boxing)
                Ok(llvm_sys::core::LLVMConstBitCast(double_const, i64_ty))
            }
            Literal::Boolean(b) => {
                // NaN-box the boolean
                let qnan = 0x7FFC_0000_0000_0000u64;
                let tag_boolean = 0x0001_0000_0000_0000u64;
                let ty = llvm_sys::core::LLVMInt64TypeInContext(ctx.context);
                Ok(llvm_sys::core::LLVMConstInt(
                    ty,
                    qnan | tag_boolean | (*b as u64),
                    0,
                ))
            }
            Literal::Null | Literal::Undefined => {
                let ty = llvm_sys::core::LLVMInt64TypeInContext(ctx.context);
                let qnan = 0x7FFC_0000_0000_0000u64;
                let tag = if matches!(lit, Literal::Null) {
                    0x0002_0000_0000_0000
                } else {
                    0x0003_0000_0000_0000
                };
                Ok(llvm_sys::core::LLVMConstInt(ty, qnan | tag, 0))
            }
            Literal::String(_s) => {
                let ty = llvm_sys::core::LLVMInt64TypeInContext(ctx.context);
                Ok(llvm_sys::core::LLVMConstInt(ty, 0, 0))
            }
        }
    }
}

/// Get an LLVM value for an IR value ID
unsafe fn get_value(ctx: &TranslationContext, id: ValueId) -> Result<LLVMValueRef, BackendError> {
    ctx.values
        .get(&id)
        .copied()
        .ok_or_else(|| BackendError::Llvm(format!("Undefined value: {:?}", id)))
}

/// Convert an i1 boolean to NaN-boxed boolean value
unsafe fn bool_to_ot_value(
    ctx: &TranslationContext,
    b: LLVMValueRef,
) -> Result<LLVMValueRef, BackendError> {
    unsafe {
        let qnan = 0x7FFC_0000_0000_0000u64;
        let tag_boolean = 0x0001_0000_0000_0000u64;
        let base = llvm_sys::core::LLVMConstInt(
            llvm_sys::core::LLVMInt64TypeInContext(ctx.context),
            qnan | tag_boolean,
            0,
        );

        let b_i64 = llvm_sys::core::LLVMBuildZExt(
            ctx.builder,
            b,
            llvm_sys::core::LLVMInt64TypeInContext(ctx.context),
            b"zext\0".as_ptr() as *const c_char,
        );
        let result = llvm_sys::core::LLVMBuildOr(
            ctx.builder,
            base,
            b_i64,
            b"bool\0".as_ptr() as *const c_char,
        );
        Ok(result)
    }
}

/// Call a runtime stub function
unsafe fn call_stub(
    ctx: &TranslationContext,
    name: &str,
    args: &[LLVMValueRef],
) -> Result<LLVMValueRef, BackendError> {
    unsafe {
        let stub = ctx
            .stubs
            .get(name)
            .copied()
            .ok_or_else(|| BackendError::Llvm(format!("Runtime stub not found: {}", name)))?;

        let name_cstr = CString::new(name).unwrap();
        let mut args_mut = args.to_vec();
        let call = llvm_sys::core::LLVMBuildCall2(
            ctx.builder,
            llvm_sys::core::LLVMGlobalGetValueType(stub),
            stub,
            args_mut.as_mut_ptr(),
            args_mut.len() as u32,
            name_cstr.as_ptr(),
        );

        Ok(call)
    }
}

/// Call a function indirectly (or directly if it's a known function)
///
/// This generates a direct LLVM call by:
/// 1. Building the function type based on number of arguments
/// 2. Casting the function pointer (i64) to the correct function pointer type
/// 3. Calling the function directly with the provided arguments
unsafe fn call_indirect(
    ctx: &TranslationContext,
    func_ptr: LLVMValueRef,
    args: &[LLVMValueRef],
) -> Result<LLVMValueRef, BackendError> {
    unsafe {
        let i64_ty = llvm_sys::core::LLVMInt64TypeInContext(ctx.context);

        // Build function type: all args are i64 (NaN-boxed), returns i64
        let mut param_types: Vec<LLVMTypeRef> = vec![i64_ty; args.len()];
        let func_ty = llvm_sys::core::LLVMFunctionType(
            i64_ty,
            if param_types.is_empty() {
                std::ptr::null_mut()
            } else {
                param_types.as_mut_ptr()
            },
            args.len() as u32,
            0,
        );
        let func_ptr_ty = llvm_sys::core::LLVMPointerType(func_ty, 0);

        // Cast i64 function address to function pointer
        let callee = llvm_sys::core::LLVMBuildIntToPtr(
            ctx.builder,
            func_ptr,
            func_ptr_ty,
            b"callee\0".as_ptr() as *const c_char,
        );

        // Build the call with actual arguments
        let mut args_mut: Vec<LLVMValueRef> = args.to_vec();
        let name_cstr = CString::new("call_result").unwrap();
        let call = llvm_sys::core::LLVMBuildCall2(
            ctx.builder,
            func_ty,
            callee,
            if args_mut.is_empty() {
                std::ptr::null_mut()
            } else {
                args_mut.as_mut_ptr()
            },
            args_mut.len() as u32,
            name_cstr.as_ptr(),
        );

        Ok(call)
    }
}
