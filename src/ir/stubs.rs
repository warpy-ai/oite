//! IR to runtime stub mapping.
//!
//! This module defines how IR operations map to runtime stubs for native code
//! generation. Each IR op is mapped to either:
//! - A direct machine instruction (for primitives like add.num)
//! - A call to a runtime stub (for dynamic operations like add.any)
//!
//! This mapping is used by the Cranelift/LLVM backends to generate native code.

use crate::ir::{IrOp, IrType};

/// How an IR operation should be compiled.
#[derive(Debug, Clone)]
pub enum CompileStrategy {
    /// Emit inline machine code (no function call).
    Inline(InlineOp),
    /// Call a runtime stub function.
    StubCall(StubCall),
    /// No code generation needed (e.g., pure type annotation).
    NoOp,
}

/// Inline operations that compile to direct machine instructions.
#[derive(Debug, Clone, Copy)]
pub enum InlineOp {
    /// Load constant into register.
    LoadConst,
    /// Floating-point add.
    FAdd,
    /// Floating-point subtract.
    FSub,
    /// Floating-point multiply.
    FMul,
    /// Floating-point divide.
    FDiv,
    /// Floating-point remainder.
    FRem,
    /// Floating-point negate.
    FNeg,
    /// Floating-point compare less than.
    FCmpLt,
    /// Floating-point compare less than or equal.
    FCmpLe,
    /// Floating-point compare greater than.
    FCmpGt,
    /// Floating-point compare greater than or equal.
    FCmpGe,
    /// Floating-point compare equal.
    FCmpEq,
    /// Floating-point compare not equal.
    FCmpNe,
    /// Boolean NOT.
    BoolNot,
    /// Copy value (register move).
    Copy,
    /// Load from local slot (stack load).
    LoadLocal,
    /// Store to local slot (stack store).
    StoreLocal,
    /// Unconditional jump.
    Jump,
    /// Conditional branch.
    Branch,
    /// Return.
    Return,
}

/// Runtime stub function to call.
#[derive(Debug, Clone)]
pub struct StubCall {
    /// Name of the stub function.
    pub name: &'static str,
    /// Number of arguments.
    pub arg_count: usize,
    /// Whether this call has side effects (can't be eliminated).
    pub has_side_effects: bool,
    /// Whether this call may throw/trap.
    pub may_trap: bool,
}

impl StubCall {
    pub const fn new(name: &'static str, arg_count: usize) -> Self {
        Self {
            name,
            arg_count,
            has_side_effects: false,
            may_trap: false,
        }
    }

    pub const fn with_side_effects(mut self) -> Self {
        self.has_side_effects = true;
        self
    }

    pub const fn may_trap(mut self) -> Self {
        self.may_trap = true;
        self
    }
}

// ============================================================================
// Stub Function Definitions
// ============================================================================

pub mod stubs {
    use super::StubCall;

    // Allocation stubs
    pub const ALLOC_OBJECT: StubCall = StubCall::new("tscl_alloc_object", 0).with_side_effects();
    pub const ALLOC_ARRAY: StubCall = StubCall::new("tscl_alloc_array", 1).with_side_effects();
    pub const ALLOC_STRING: StubCall = StubCall::new("tscl_alloc_string", 2).with_side_effects();

    // Property access stubs
    pub const GET_PROP: StubCall = StubCall::new("tscl_get_prop", 3);
    pub const SET_PROP: StubCall = StubCall::new("tscl_set_prop", 4).with_side_effects();
    pub const GET_ELEMENT: StubCall = StubCall::new("tscl_get_element", 2);
    pub const SET_ELEMENT: StubCall = StubCall::new("tscl_set_element", 3).with_side_effects();

    // Dynamic arithmetic stubs
    pub const ADD_ANY: StubCall = StubCall::new("tscl_add_any", 2);
    pub const SUB_ANY: StubCall = StubCall::new("tscl_sub_any", 2);
    pub const MUL_ANY: StubCall = StubCall::new("tscl_mul_any", 2);
    pub const DIV_ANY: StubCall = StubCall::new("tscl_div_any", 2).may_trap();
    pub const MOD_ANY: StubCall = StubCall::new("tscl_mod_any", 2).may_trap();
    pub const NEG_ANY: StubCall = StubCall::new("tscl_neg_any", 1);

    // Comparison stubs
    pub const EQ_STRICT: StubCall = StubCall::new("tscl_eq_strict", 2);
    pub const LT: StubCall = StubCall::new("tscl_lt", 2);
    pub const GT: StubCall = StubCall::new("tscl_gt", 2);
    pub const NOT: StubCall = StubCall::new("tscl_not", 1);
    pub const INSTANCEOF: StubCall = StubCall::new("tscl_instanceof", 2);

    // Type conversion stubs
    pub const TO_BOOLEAN: StubCall = StubCall::new("tscl_to_boolean", 1);
    pub const TO_NUMBER: StubCall = StubCall::new("tscl_to_number", 1);

    // Function call stubs
    pub const CALL: StubCall = StubCall::new("tscl_call", 3).with_side_effects().may_trap();

    // Console/IO stubs
    pub const CONSOLE_LOG: StubCall = StubCall::new("tscl_console_log", 1).with_side_effects();
}

// ============================================================================
// IR Operation Mapping
// ============================================================================

/// Get the compilation strategy for an IR operation.
pub fn compile_strategy(op: &IrOp) -> CompileStrategy {
    match op {
        // Constants - inline load
        IrOp::Const(_, _) => CompileStrategy::Inline(InlineOp::LoadConst),

        // Specialized numeric operations - inline FP instructions
        IrOp::AddNum(_, _, _) => CompileStrategy::Inline(InlineOp::FAdd),
        IrOp::SubNum(_, _, _) => CompileStrategy::Inline(InlineOp::FSub),
        IrOp::MulNum(_, _, _) => CompileStrategy::Inline(InlineOp::FMul),
        IrOp::DivNum(_, _, _) => CompileStrategy::Inline(InlineOp::FDiv),
        IrOp::ModNum(_, _, _) => CompileStrategy::Inline(InlineOp::FRem),
        IrOp::NegNum(_, _) => CompileStrategy::Inline(InlineOp::FNeg),

        // Dynamic arithmetic - call stubs
        IrOp::AddAny(_, _, _) => CompileStrategy::StubCall(stubs::ADD_ANY),
        IrOp::SubAny(_, _, _) => CompileStrategy::StubCall(stubs::SUB_ANY),
        IrOp::MulAny(_, _, _) => CompileStrategy::StubCall(stubs::MUL_ANY),
        IrOp::DivAny(_, _, _) => CompileStrategy::StubCall(stubs::DIV_ANY),
        IrOp::ModAny(_, _, _) => CompileStrategy::StubCall(stubs::MOD_ANY),
        IrOp::NegAny(_, _) => CompileStrategy::StubCall(stubs::NEG_ANY),

        // Comparisons - specialized for numbers, stub for mixed
        IrOp::Lt(_, _, _) => CompileStrategy::Inline(InlineOp::FCmpLt), // TODO: type-based selection
        IrOp::LtEq(_, _, _) => CompileStrategy::Inline(InlineOp::FCmpLe),
        IrOp::Gt(_, _, _) => CompileStrategy::Inline(InlineOp::FCmpGt),
        IrOp::GtEq(_, _, _) => CompileStrategy::Inline(InlineOp::FCmpGe),
        IrOp::EqStrict(_, _, _) => CompileStrategy::StubCall(stubs::EQ_STRICT),
        IrOp::NeStrict(_, _, _) => CompileStrategy::StubCall(stubs::EQ_STRICT), // Negate result

        // Logical operations
        IrOp::Not(_, _) => CompileStrategy::Inline(InlineOp::BoolNot),
        IrOp::And(_, _, _) => CompileStrategy::NoOp, // Handled by control flow
        IrOp::Or(_, _, _) => CompileStrategy::NoOp,  // Handled by control flow

        // Local variable access - inline stack operations
        IrOp::LoadLocal(_, _) => CompileStrategy::Inline(InlineOp::LoadLocal),
        IrOp::StoreLocal(_, _) => CompileStrategy::Inline(InlineOp::StoreLocal),

        // Global variable access - call stubs
        IrOp::LoadGlobal(_, _) => CompileStrategy::StubCall(stubs::GET_PROP),
        IrOp::StoreGlobal(_, _) => CompileStrategy::StubCall(stubs::SET_PROP),

        // Object operations - call stubs
        IrOp::NewObject(_) => CompileStrategy::StubCall(stubs::ALLOC_OBJECT),
        IrOp::GetProp(_, _, _) => CompileStrategy::StubCall(stubs::GET_PROP),
        IrOp::SetProp(_, _, _) => CompileStrategy::StubCall(stubs::SET_PROP),
        IrOp::GetElement(_, _, _) => CompileStrategy::StubCall(stubs::GET_ELEMENT),
        IrOp::SetElement(_, _, _) => CompileStrategy::StubCall(stubs::SET_ELEMENT),

        // Array operations
        IrOp::NewArray(_) => CompileStrategy::StubCall(stubs::ALLOC_ARRAY),
        IrOp::ArrayLen(_, _) => CompileStrategy::StubCall(stubs::GET_PROP), // .length property
        IrOp::ArrayPush(_, _) => CompileStrategy::StubCall(stubs::CALL),    // .push method

        // Function operations
        IrOp::Call(_, _, _) => CompileStrategy::StubCall(stubs::CALL),
        IrOp::CallMethod(_, _, _, _) => CompileStrategy::StubCall(stubs::CALL),
        IrOp::MakeClosure(_, _, _) => CompileStrategy::StubCall(stubs::ALLOC_OBJECT),

        // Type operations
        IrOp::TypeCheck(_, _, _) => CompileStrategy::NoOp, // Compile-time only
        IrOp::TypeGuard(_, _, _) => CompileStrategy::Inline(InlineOp::Copy),
        IrOp::ToBool(_, _) => CompileStrategy::StubCall(stubs::TO_BOOLEAN),
        IrOp::ToNum(_, _) => CompileStrategy::StubCall(stubs::TO_NUMBER),

        // SSA operations
        IrOp::Phi(_, _) => CompileStrategy::NoOp, // Handled by register allocation
        IrOp::Copy(_, _) => CompileStrategy::Inline(InlineOp::Copy),
        IrOp::LoadThis(_) => CompileStrategy::Inline(InlineOp::LoadLocal),

        // Borrow operations - handled by register allocation or inline
        IrOp::Borrow(_, _) => CompileStrategy::Inline(InlineOp::Copy), // Just copy ptr
        IrOp::BorrowMut(_, _) => CompileStrategy::Inline(InlineOp::Copy),
        IrOp::Deref(_, _) => CompileStrategy::Inline(InlineOp::LoadLocal), // Load through ptr
        IrOp::DerefStore(_, _) => CompileStrategy::Inline(InlineOp::StoreLocal), // Store through ptr
        IrOp::EndBorrow(_) => CompileStrategy::NoOp, // Compile-time only

        // Struct operations - will need stubs when implemented
        IrOp::StructNew(_, _) => CompileStrategy::StubCall(stubs::ALLOC_OBJECT),
        IrOp::StructGetField(_, _, _) => CompileStrategy::Inline(InlineOp::LoadLocal),
        IrOp::StructSetField(_, _, _) => CompileStrategy::Inline(InlineOp::StoreLocal),
        IrOp::StructGetFieldNamed(_, _, _) => CompileStrategy::StubCall(stubs::GET_PROP),
        IrOp::StructSetFieldNamed(_, _, _) => CompileStrategy::StubCall(stubs::SET_PROP),

        // Monomorphized calls
        IrOp::CallMono(_, _, _) => CompileStrategy::StubCall(stubs::CALL),

        // Move/Clone operations
        IrOp::Move(_, _) => CompileStrategy::Inline(InlineOp::Copy), // Move is just ownership transfer
        IrOp::Clone(_, _) => CompileStrategy::StubCall(stubs::ALLOC_OBJECT), // Clone needs allocation
    }
}

/// Check if an operation can be inlined (no function call).
pub fn can_inline(op: &IrOp) -> bool {
    matches!(compile_strategy(op), CompileStrategy::Inline(_))
}

/// Check if an operation requires a runtime stub call.
pub fn needs_stub(op: &IrOp) -> bool {
    matches!(compile_strategy(op), CompileStrategy::StubCall(_))
}

/// Get the stub call for an operation, if any.
pub fn get_stub(op: &IrOp) -> Option<StubCall> {
    match compile_strategy(op) {
        CompileStrategy::StubCall(stub) => Some(stub),
        _ => None,
    }
}

// ============================================================================
// Type-Based Optimization
// ============================================================================

/// Determine if a comparison can be inlined based on operand types.
pub fn can_inline_comparison(ty_a: IrType, ty_b: IrType) -> bool {
    // Can inline if both are numbers
    ty_a == IrType::Number && ty_b == IrType::Number
}

/// Determine if arithmetic can be inlined based on operand types.
pub fn can_inline_arithmetic(ty_a: IrType, ty_b: IrType) -> bool {
    // Can inline if both are numbers
    ty_a == IrType::Number && ty_b == IrType::Number
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{Literal, ValueId};

    #[test]
    fn test_numeric_ops_are_inline() {
        let a = ValueId(0);
        let b = ValueId(1);
        let c = ValueId(2);

        assert!(can_inline(&IrOp::AddNum(c, a, b)));
        assert!(can_inline(&IrOp::SubNum(c, a, b)));
        assert!(can_inline(&IrOp::MulNum(c, a, b)));
        assert!(can_inline(&IrOp::DivNum(c, a, b)));
    }

    #[test]
    fn test_dynamic_ops_need_stubs() {
        let a = ValueId(0);
        let b = ValueId(1);
        let c = ValueId(2);

        assert!(needs_stub(&IrOp::AddAny(c, a, b)));
        assert!(needs_stub(&IrOp::SubAny(c, a, b)));
        assert!(needs_stub(&IrOp::MulAny(c, a, b)));
    }

    #[test]
    fn test_stub_properties() {
        assert!(stubs::CALL.has_side_effects);
        assert!(stubs::CALL.may_trap);
        assert!(!stubs::ADD_ANY.has_side_effects);
        assert!(!stubs::ADD_ANY.may_trap);
    }

    #[test]
    fn test_const_is_inline() {
        let d = ValueId(0);
        assert!(can_inline(&IrOp::Const(d, Literal::Number(42.0))));
    }
}
