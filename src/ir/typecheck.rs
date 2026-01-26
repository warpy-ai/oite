//! Flow-sensitive type inference for SSA IR.
//!
//! This module performs forward dataflow analysis to infer concrete types
//! for values, enabling specialization of dynamic operations.
//!
//! Example:
//!   Before: v3 = add.any v1, v2  (where v1: num, v2: num)
//!   After:  v3 = add.num v1, v2

use crate::ir::{BlockId, IrFunction, IrModule, IrOp, IrType, ValueId};
use std::collections::{HashMap, HashSet, VecDeque};

/// Type inference context for a function.
pub struct TypeChecker<'a> {
    func: &'a mut IrFunction,
    /// Work queue of blocks to process.
    worklist: VecDeque<BlockId>,
    /// Set of blocks already in the worklist.
    in_worklist: HashSet<BlockId>,
    /// Type information for each value.
    types: HashMap<ValueId, IrType>,
    /// Whether any changes were made in the current iteration.
    changed: bool,
}

impl<'a> TypeChecker<'a> {
    /// Create a new type checker for a function.
    pub fn new(func: &'a mut IrFunction) -> Self {
        let mut types = HashMap::new();

        // Initialize types from function's value_types
        for (&val, ty) in &func.value_types {
            types.insert(val, ty.clone());
        }

        Self {
            func,
            worklist: VecDeque::new(),
            in_worklist: HashSet::new(),
            types,
            changed: false,
        }
    }

    /// Run type inference on the function.
    pub fn infer(&mut self) {
        // Initialize worklist with entry block
        let entry = self.func.entry_block();
        self.worklist.push_back(entry);
        self.in_worklist.insert(entry);

        // Process blocks until fixpoint
        while let Some(block_id) = self.worklist.pop_front() {
            self.in_worklist.remove(&block_id);
            self.process_block(block_id);
        }

        // Update function's value_types with inferred types
        for (val, ty) in &self.types {
            self.func.value_types.insert(*val, ty.clone());
        }
    }

    /// Process a single block, inferring types for all operations.
    fn process_block(&mut self, block_id: BlockId) {
        let block = &self.func.blocks[block_id.0 as usize];

        // Clone ops to avoid borrow issues
        let ops: Vec<IrOp> = block.ops.clone();
        let terminator = block.terminator.clone();

        // Process each operation
        for op in &ops {
            self.infer_op(op);
        }

        // Add successors to worklist if types changed
        if self.changed {
            self.changed = false;
            for succ in terminator.successors() {
                if !self.in_worklist.contains(&succ) {
                    self.worklist.push_back(succ);
                    self.in_worklist.insert(succ);
                }
            }
        }
    }

    /// Get the type of a value.
    fn get_type(&self, val: ValueId) -> IrType {
        self.types.get(&val).cloned().unwrap_or(IrType::Any)
    }

    /// Set the type of a value, marking changed if different.
    fn set_type(&mut self, val: ValueId, ty: IrType) {
        let old = self.get_type(val);
        if old != ty {
            // Only narrow from Any to concrete, don't widen
            if old == IrType::Any || ty == old {
                self.types.insert(val, ty);
                self.changed = true;
            }
        }
    }

    /// Infer types for an operation.
    fn infer_op(&mut self, op: &IrOp) {
        match op {
            IrOp::Const(dst, lit) => {
                let ty = lit.ir_type();
                self.set_type(*dst, ty);
            }

            // Numeric operations always produce numbers
            IrOp::AddNum(dst, _, _)
            | IrOp::SubNum(dst, _, _)
            | IrOp::MulNum(dst, _, _)
            | IrOp::DivNum(dst, _, _)
            | IrOp::ModNum(dst, _, _)
            | IrOp::NegNum(dst, _) => {
                self.set_type(*dst, IrType::Number);
            }

            // Dynamic operations: infer from operands
            IrOp::AddAny(dst, a, b) => {
                let ta = self.get_type(*a);
                let tb = self.get_type(*b);

                // If both are numbers, result is number
                // If either is string, result is string (concatenation)
                // Otherwise, any
                let result_ty = match (ta, tb) {
                    (IrType::Number, IrType::Number) => IrType::Number,
                    (IrType::String, _) | (_, IrType::String) => IrType::String,
                    _ => IrType::Any,
                };
                self.set_type(*dst, result_ty);
            }

            IrOp::SubAny(dst, a, b)
            | IrOp::MulAny(dst, a, b)
            | IrOp::DivAny(dst, a, b)
            | IrOp::ModAny(dst, a, b) => {
                let ta = self.get_type(*a);
                let tb = self.get_type(*b);

                // These ops always produce numbers (coercion)
                let result_ty = if ta == IrType::Number && tb == IrType::Number {
                    IrType::Number
                } else {
                    IrType::Any // May need runtime coercion
                };
                self.set_type(*dst, result_ty);
            }

            IrOp::NegAny(dst, a) => {
                let ta = self.get_type(*a);
                let result_ty = if ta == IrType::Number {
                    IrType::Number
                } else {
                    IrType::Any
                };
                self.set_type(*dst, result_ty);
            }

            // Comparison operations always produce boolean
            IrOp::EqStrict(dst, _, _)
            | IrOp::NeStrict(dst, _, _)
            | IrOp::Lt(dst, _, _)
            | IrOp::LtEq(dst, _, _)
            | IrOp::Gt(dst, _, _)
            | IrOp::GtEq(dst, _, _) => {
                self.set_type(*dst, IrType::Boolean);
            }

            // Logical NOT always produces boolean
            IrOp::Not(dst, _) => {
                self.set_type(*dst, IrType::Boolean);
            }

            // And/Or return one of their operands
            IrOp::And(dst, a, b) | IrOp::Or(dst, a, b) => {
                let ta = self.get_type(*a);
                let tb = self.get_type(*b);
                let result_ty = if ta == tb { ta } else { IrType::Any };
                self.set_type(*dst, result_ty);
            }

            // Local loads get Any (unless we track local types)
            IrOp::LoadLocal(dst, _) => {
                self.set_type(*dst, IrType::Any);
            }

            // Global loads get Any
            IrOp::LoadGlobal(dst, _) => {
                self.set_type(*dst, IrType::Any);
            }

            // Object creation
            IrOp::NewObject(dst) => {
                self.set_type(*dst, IrType::Object);
            }

            // Property access returns Any
            IrOp::GetProp(dst, _, _) => {
                self.set_type(*dst, IrType::Any);
            }

            IrOp::GetElement(dst, _, _) => {
                self.set_type(*dst, IrType::Any);
            }

            // Array creation
            IrOp::NewArray(dst) => {
                self.set_type(*dst, IrType::Array);
            }

            // Array length is a number
            IrOp::ArrayLen(dst, _) => {
                self.set_type(*dst, IrType::Number);
            }

            // Function call returns Any (without more analysis)
            IrOp::Call(dst, _, _) => {
                self.set_type(*dst, IrType::Any);
            }

            IrOp::CallMethod(dst, _, _, _) => {
                self.set_type(*dst, IrType::Any);
            }

            // Closure creation
            IrOp::MakeClosure(dst, _, _) => {
                self.set_type(*dst, IrType::Function);
            }

            // Type operations
            IrOp::TypeCheck(dst, _, _) => {
                self.set_type(*dst, IrType::Boolean);
            }

            IrOp::TypeGuard(dst, val, ty) => {
                // TypeGuard narrows the type
                let _ = self.get_type(*val); // Mark as used
                self.set_type(*dst, ty.clone());
            }

            IrOp::ToBool(dst, _) => {
                self.set_type(*dst, IrType::Boolean);
            }

            IrOp::ToNum(dst, _) => {
                self.set_type(*dst, IrType::Number);
            }

            // Phi: meet of all incoming types
            IrOp::Phi(dst, entries) => {
                let mut result_ty = IrType::Never;
                for (_, val) in entries {
                    let ty = self.get_type(*val);
                    result_ty = type_meet(result_ty, ty);
                }
                self.set_type(*dst, result_ty);
            }

            // Copy preserves type
            IrOp::Copy(dst, src) => {
                let ty = self.get_type(*src);
                self.set_type(*dst, ty);
            }

            // LoadThis returns Object
            IrOp::LoadThis(dst) => {
                self.set_type(*dst, IrType::Object);
            }

            // Side-effecting ops with no result
            IrOp::StoreLocal(_, _)
            | IrOp::StoreGlobal(_, _)
            | IrOp::SetProp(_, _, _)
            | IrOp::SetElement(_, _, _)
            | IrOp::ArrayPush(_, _)
            | IrOp::DerefStore(_, _)
            | IrOp::EndBorrow(_)
            | IrOp::StructSetField(_, _, _)
            | IrOp::StructSetFieldNamed(_, _, _) => {}

            // Borrow operations
            IrOp::Borrow(dst, src) => {
                let src_ty = self.get_type(*src);
                self.set_type(*dst, IrType::Ref(Box::new(src_ty)));
            }

            IrOp::BorrowMut(dst, src) => {
                let src_ty = self.get_type(*src);
                self.set_type(*dst, IrType::MutRef(Box::new(src_ty)));
            }

            IrOp::Deref(dst, src) => {
                let src_ty = self.get_type(*src);
                // Unwrap the reference
                let result_ty = match src_ty.deref_type() {
                    Some(inner) => inner.clone(),
                    None => IrType::Any,
                };
                self.set_type(*dst, result_ty);
            }

            // Struct operations
            IrOp::StructNew(dst, struct_id) => {
                self.set_type(*dst, IrType::Struct(*struct_id));
            }

            IrOp::StructGetField(dst, _, _) | IrOp::StructGetFieldNamed(dst, _, _) => {
                // Would need struct type info to get field type
                self.set_type(*dst, IrType::Any);
            }

            // Monomorphized calls
            IrOp::CallMono(dst, _, _) => {
                self.set_type(*dst, IrType::Any);
            }

            // Move/Clone preserve type
            IrOp::Move(dst, src) | IrOp::Clone(dst, src) => {
                let ty = self.get_type(*src);
                self.set_type(*dst, ty);
            }

            // Bitwise operations always produce numbers
            IrOp::BitAnd(dst, _, _)
            | IrOp::BitOr(dst, _, _)
            | IrOp::Xor(dst, _, _)
            | IrOp::Shl(dst, _, _)
            | IrOp::Shr(dst, _, _)
            | IrOp::ShrU(dst, _, _)
            | IrOp::Pow(dst, _, _) => {
                self.set_type(*dst, IrType::Number);
            }

            // TypeOf always produces a string
            IrOp::TypeOf(dst, _) => {
                self.set_type(*dst, IrType::String);
            }

            // DeleteProp always produces a boolean
            IrOp::DeleteProp(dst, _, _) => {
                self.set_type(*dst, IrType::Boolean);
            }
        }
    }
}

/// Compute the meet (least upper bound) of two types.
fn type_meet(a: IrType, b: IrType) -> IrType {
    if a == b {
        return a;
    }
    if a == IrType::Never {
        return b;
    }
    if b == IrType::Never {
        return a;
    }
    // Different concrete types → Any
    IrType::Any
}

/// Specialize dynamic operations based on inferred types.
pub fn specialize_ops(func: &mut IrFunction) {
    for block in &mut func.blocks {
        let ops = std::mem::take(&mut block.ops);
        block.ops = ops
            .into_iter()
            .map(|op| specialize_op(op, &func.value_types))
            .collect();
    }
}

/// Specialize a single operation.
fn specialize_op(op: IrOp, types: &HashMap<ValueId, IrType>) -> IrOp {
    let get_type = |v: ValueId| types.get(&v).cloned().unwrap_or(IrType::Any);

    match op {
        IrOp::AddAny(dst, a, b) => {
            if get_type(a) == IrType::Number && get_type(b) == IrType::Number {
                IrOp::AddNum(dst, a, b)
            } else {
                IrOp::AddAny(dst, a, b)
            }
        }

        IrOp::SubAny(dst, a, b) => {
            if get_type(a) == IrType::Number && get_type(b) == IrType::Number {
                IrOp::SubNum(dst, a, b)
            } else {
                IrOp::SubAny(dst, a, b)
            }
        }

        IrOp::MulAny(dst, a, b) => {
            if get_type(a) == IrType::Number && get_type(b) == IrType::Number {
                IrOp::MulNum(dst, a, b)
            } else {
                IrOp::MulAny(dst, a, b)
            }
        }

        IrOp::DivAny(dst, a, b) => {
            if get_type(a) == IrType::Number && get_type(b) == IrType::Number {
                IrOp::DivNum(dst, a, b)
            } else {
                IrOp::DivAny(dst, a, b)
            }
        }

        IrOp::ModAny(dst, a, b) => {
            if get_type(a) == IrType::Number && get_type(b) == IrType::Number {
                IrOp::ModNum(dst, a, b)
            } else {
                IrOp::ModAny(dst, a, b)
            }
        }

        IrOp::NegAny(dst, a) => {
            if get_type(a) == IrType::Number {
                IrOp::NegNum(dst, a)
            } else {
                IrOp::NegAny(dst, a)
            }
        }

        // All other operations pass through unchanged
        other => other,
    }
}

/// Run type inference and specialization on a function.
pub fn typecheck_function(func: &mut IrFunction) {
    let mut checker = TypeChecker::new(func);
    checker.infer();
    specialize_ops(func);
}

/// Run type inference and specialization on a module.
pub fn typecheck_module(module: &mut IrModule) {
    for func in &mut module.functions {
        typecheck_function(func);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{Literal, Terminator};

    #[test]
    fn test_type_inference_numeric() {
        let mut func = IrFunction::new("test".to_string());
        let entry = func.alloc_block();

        let a = func.alloc_value(IrType::Number);
        let b = func.alloc_value(IrType::Number);
        let c = func.alloc_value(IrType::Any); // Will be inferred as Number

        {
            let block = func.block_mut(entry);
            block.push(IrOp::Const(a, Literal::Number(1.0)));
            block.push(IrOp::Const(b, Literal::Number(2.0)));
            block.push(IrOp::AddAny(c, a, b)); // Should become AddNum
            block.terminate(Terminator::Return(Some(c)));
        }

        typecheck_function(&mut func);

        // After type checking, c should be Number
        assert_eq!(func.value_types.get(&c), Some(&IrType::Number));

        // And AddAny should be specialized to AddNum
        let ops = &func.blocks[entry.0 as usize].ops;
        let has_add_num = ops.iter().any(|op| matches!(op, IrOp::AddNum(_, _, _)));
        assert!(has_add_num, "AddAny should be specialized to AddNum");
    }

    #[test]
    fn test_type_inference_string_concat() {
        let mut func = IrFunction::new("test".to_string());
        let entry = func.alloc_block();

        let a = func.alloc_value(IrType::String);
        let b = func.alloc_value(IrType::String);
        let c = func.alloc_value(IrType::Any);

        {
            let block = func.block_mut(entry);
            block.push(IrOp::Const(a, Literal::String("hello".to_string())));
            block.push(IrOp::Const(b, Literal::String(" world".to_string())));
            block.push(IrOp::AddAny(c, a, b));
            block.terminate(Terminator::Return(Some(c)));
        }

        typecheck_function(&mut func);

        // c should be String
        assert_eq!(func.value_types.get(&c), Some(&IrType::String));

        // AddAny stays AddAny for strings (no specialized string concat op yet)
        let ops = &func.blocks[entry.0 as usize].ops;
        let has_add_any = ops.iter().any(|op| matches!(op, IrOp::AddAny(_, _, _)));
        assert!(has_add_any, "String concat should remain AddAny");
    }

    #[test]
    fn test_type_meet() {
        assert_eq!(type_meet(IrType::Number, IrType::Number), IrType::Number);
        assert_eq!(type_meet(IrType::Number, IrType::String), IrType::Any);
        assert_eq!(type_meet(IrType::Never, IrType::Number), IrType::Number);
        assert_eq!(type_meet(IrType::Any, IrType::Number), IrType::Any);
    }
}
