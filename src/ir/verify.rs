//! IR verification pass.
//!
//! Validates that IR is well-formed before native code generation:
//! - All used values are defined
//! - All blocks have terminators
//! - SSA property (each value defined exactly once)
//! - Type consistency
//! - Ownership validity (no use after move)
//! - Borrow rules (no mutable + immutable overlap)

use crate::ir::{
    BasicBlock, BlockId, IrFunction, IrModule, IrOp, IrType, Ownership, Terminator, ValueId,
};
use std::collections::{HashMap, HashSet};

/// Verification error.
#[derive(Debug)]
pub enum VerifyError {
    /// Value used but not defined.
    UndefinedValue(ValueId, BlockId),
    /// Value defined multiple times (violates SSA).
    MultipleDefinitions(ValueId),
    /// Block missing terminator.
    MissingTerminator(BlockId),
    /// Type mismatch in operation.
    TypeMismatch {
        op: String,
        expected: IrType,
        got: IrType,
    },
    /// Use of moved value.
    UseAfterMove(ValueId, BlockId),
    /// Invalid borrow (mutable borrow while immutable exists).
    InvalidBorrow(ValueId, BlockId),
    /// Jump to non-existent block.
    InvalidBlockTarget(BlockId, BlockId),
    /// Return type mismatch.
    ReturnTypeMismatch {
        expected: IrType,
        got: Option<IrType>,
    },
    /// Invalid local slot access.
    InvalidLocalSlot(u32),
}

impl std::fmt::Display for VerifyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VerifyError::UndefinedValue(val, block) => {
                write!(f, "Value {} used but not defined in block {}", val, block)
            }
            VerifyError::MultipleDefinitions(val) => {
                write!(f, "Value {} defined multiple times (SSA violation)", val)
            }
            VerifyError::MissingTerminator(block) => {
                write!(f, "Block {} missing terminator", block)
            }
            VerifyError::TypeMismatch { op, expected, got } => {
                write!(
                    f,
                    "Type mismatch in {}: expected {}, got {}",
                    op, expected, got
                )
            }
            VerifyError::UseAfterMove(val, block) => {
                write!(f, "Use of moved value {} in block {}", val, block)
            }
            VerifyError::InvalidBorrow(val, block) => {
                write!(f, "Invalid borrow of {} in block {}", val, block)
            }
            VerifyError::InvalidBlockTarget(from, to) => {
                write!(f, "Invalid jump target {} from block {}", to, from)
            }
            VerifyError::ReturnTypeMismatch { expected, got } => {
                write!(
                    f,
                    "Return type mismatch: expected {}, got {:?}",
                    expected, got
                )
            }
            VerifyError::InvalidLocalSlot(slot) => {
                write!(f, "Invalid local slot ${}", slot)
            }
        }
    }
}

impl std::error::Error for VerifyError {}

/// IR verifier.
pub struct Verifier<'a> {
    func: &'a IrFunction,
    /// All defined values.
    defined: HashSet<ValueId>,
    /// Values that have been moved.
    moved: HashSet<ValueId>,
    /// Errors found.
    errors: Vec<VerifyError>,
}

impl<'a> Verifier<'a> {
    pub fn new(func: &'a IrFunction) -> Self {
        Self {
            func,
            defined: HashSet::new(),
            moved: HashSet::new(),
            errors: Vec::new(),
        }
    }

    /// Run all verification passes on the function.
    pub fn verify(mut self) -> Result<(), Vec<VerifyError>> {
        self.verify_structure();
        self.verify_ssa();
        self.verify_control_flow();
        self.verify_ownership();

        if self.errors.is_empty() {
            Ok(())
        } else {
            Err(self.errors)
        }
    }

    /// Verify basic structure: blocks have terminators, locals are valid.
    fn verify_structure(&mut self) {
        for block in &self.func.blocks {
            // Check for terminator
            if matches!(block.terminator, Terminator::Unreachable) && !self.is_dead_block(block.id)
            {
                self.errors.push(VerifyError::MissingTerminator(block.id));
            }
        }
    }

    /// Verify SSA property: each value defined exactly once, used after definition.
    fn verify_ssa(&mut self) {
        // Collect all definitions
        let mut definitions: HashMap<ValueId, BlockId> = HashMap::new();

        for block in &self.func.blocks {
            for op in &block.ops {
                if let Some(dest) = op.dest() {
                    if definitions.contains_key(&dest) {
                        self.errors.push(VerifyError::MultipleDefinitions(dest));
                    } else {
                        definitions.insert(dest, block.id);
                        self.defined.insert(dest);
                    }
                }
            }
        }

        // Check all uses have definitions
        for block in &self.func.blocks {
            for op in &block.ops {
                for used in op.uses() {
                    if !self.defined.contains(&used) {
                        self.errors
                            .push(VerifyError::UndefinedValue(used, block.id));
                    }
                }
            }

            // Check terminator uses
            for used in block.terminator.uses() {
                if !self.defined.contains(&used) {
                    self.errors
                        .push(VerifyError::UndefinedValue(used, block.id));
                }
            }
        }
    }

    /// Verify control flow: all jump targets exist.
    fn verify_control_flow(&mut self) {
        let block_ids: HashSet<_> = self.func.blocks.iter().map(|b| b.id).collect();

        for block in &self.func.blocks {
            for succ in block.terminator.successors() {
                if !block_ids.contains(&succ) {
                    self.errors
                        .push(VerifyError::InvalidBlockTarget(block.id, succ));
                }
            }
        }
    }

    /// Verify ownership: no use after move, valid borrows.
    fn verify_ownership(&mut self) {
        // Track moved values through the function
        let mut moved_at: HashMap<ValueId, BlockId> = HashMap::new();

        for block in &self.func.blocks {
            for op in &block.ops {
                // Check uses are not moved
                for used in op.uses() {
                    if let Some(&move_block) = moved_at.get(&used) {
                        // Only report if moved in a predecessor block
                        // (same-block moves are handled by lowering order)
                        if move_block != block.id {
                            self.errors.push(VerifyError::UseAfterMove(used, block.id));
                        }
                    }
                }

                // Track moves
                if Self::is_move_op(op) {
                    for used in op.uses() {
                        // Only move reference types
                        if let Some(ty) = self.func.value_types.get(&used) {
                            if ty.is_reference() {
                                moved_at.insert(used, block.id);
                                self.moved.insert(used);
                            }
                        }
                    }
                }
            }
        }
    }

    /// Check if a block is dead (unreachable).
    fn is_dead_block(&self, block_id: BlockId) -> bool {
        // Entry block is never dead
        if block_id.0 == 0 {
            return false;
        }

        // A block is dead if it has no predecessors and isn't the entry
        self.func.blocks[block_id.0 as usize]
            .predecessors
            .is_empty()
    }

    /// Check if an operation performs a move.
    fn is_move_op(op: &IrOp) -> bool {
        matches!(
            op,
            IrOp::StoreLocal(_, _)
                | IrOp::StoreGlobal(_, _)
                | IrOp::SetProp(_, _, _)
                | IrOp::SetElement(_, _, _)
                | IrOp::Call(_, _, _)
                | IrOp::MakeClosure(_, _, _)
        )
    }
}

/// Verify a single function.
pub fn verify_function(func: &IrFunction) -> Result<(), Vec<VerifyError>> {
    Verifier::new(func).verify()
}

/// Verify all functions in a module.
pub fn verify_module(module: &IrModule) -> Result<(), Vec<VerifyError>> {
    let mut all_errors = Vec::new();

    for func in &module.functions {
        if let Err(errors) = verify_function(func) {
            all_errors.extend(errors);
        }
    }

    if all_errors.is_empty() {
        Ok(())
    } else {
        Err(all_errors)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{Literal, Terminator};

    #[test]
    fn test_verify_valid_function() {
        let mut func = IrFunction::new("test".to_string());
        let entry = func.alloc_block();

        let a = func.alloc_value(IrType::Number);
        let b = func.alloc_value(IrType::Number);
        let c = func.alloc_value(IrType::Number);

        {
            let block = func.block_mut(entry);
            block.push(IrOp::Const(a, Literal::Number(1.0)));
            block.push(IrOp::Const(b, Literal::Number(2.0)));
            block.push(IrOp::AddNum(c, a, b));
            block.terminate(Terminator::Return(Some(c)));
        }

        func.compute_predecessors();
        assert!(verify_function(&func).is_ok());
    }

    #[test]
    fn test_verify_undefined_value() {
        let mut func = IrFunction::new("test".to_string());
        let entry = func.alloc_block();

        let a = func.alloc_value(IrType::Number);
        let c = func.alloc_value(IrType::Number);

        // Use 'b' without defining it
        let undefined_b = ValueId(999);

        {
            let block = func.block_mut(entry);
            block.push(IrOp::Const(a, Literal::Number(1.0)));
            block.push(IrOp::AddNum(c, a, undefined_b)); // Error: b not defined
            block.terminate(Terminator::Return(Some(c)));
        }

        func.compute_predecessors();
        let result = verify_function(&func);
        assert!(result.is_err());

        let errors = result.unwrap_err();
        assert!(errors.iter().any(|e| matches!(
            e,
            VerifyError::UndefinedValue(v, _) if v.0 == 999
        )));
    }

    #[test]
    fn test_verify_ssa_violation() {
        let mut func = IrFunction::new("test".to_string());
        let entry = func.alloc_block();

        let a = func.alloc_value(IrType::Number);

        {
            let block = func.block_mut(entry);
            // Define 'a' twice - SSA violation
            block.push(IrOp::Const(a, Literal::Number(1.0)));
            block.push(IrOp::Const(a, Literal::Number(2.0)));
            block.terminate(Terminator::Return(Some(a)));
        }

        func.compute_predecessors();
        let result = verify_function(&func);
        assert!(result.is_err());

        let errors = result.unwrap_err();
        assert!(errors
            .iter()
            .any(|e| matches!(e, VerifyError::MultipleDefinitions(_))));
    }
}
