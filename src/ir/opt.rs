//! IR optimization passes.
//!
//! This module provides common optimizations for SSA IR:
//! - Dead Code Elimination (DCE)
//! - Constant Folding
//! - Common Subexpression Elimination (CSE)
//! - Copy Propagation

use crate::ir::{
    BasicBlock, BlockId, IrFunction, IrModule, IrOp, IrType, Literal, Terminator, ValueId,
};
use std::collections::{HashMap, HashSet};

// ============================================================================
// Dead Code Elimination
// ============================================================================

/// Remove operations whose results are never used.
pub fn dead_code_elimination(func: &mut IrFunction) {
    // Compute which values are used
    let used_values = compute_used_values(func);

    // Remove unused operations from each block
    for block in &mut func.blocks {
        block.ops.retain(|op| {
            // Keep operations with side effects
            if has_side_effects(op) {
                return true;
            }

            // Keep operations whose result is used
            if let Some(dst) = op.dest() {
                return used_values.contains(&dst);
            }

            true
        });
    }
}

/// Compute the set of all values that are used.
fn compute_used_values(func: &IrFunction) -> HashSet<ValueId> {
    let mut used = HashSet::new();

    // Add uses from all operations
    for block in &func.blocks {
        for op in &block.ops {
            for val in op.uses() {
                used.insert(val);
            }
        }

        // Add uses from terminator
        for val in block.terminator.uses() {
            used.insert(val);
        }
    }

    used
}

/// Check if an operation has side effects.
fn has_side_effects(op: &IrOp) -> bool {
    matches!(
        op,
        IrOp::StoreLocal(_, _)
            | IrOp::StoreGlobal(_, _)
            | IrOp::SetProp(_, _, _)
            | IrOp::SetElement(_, _, _)
            | IrOp::ArrayPush(_, _)
            | IrOp::Call(_, _, _)
            | IrOp::CallMethod(_, _, _, _)
    )
}

// ============================================================================
// Constant Folding
// ============================================================================

/// Fold constant expressions at compile time.
pub fn constant_folding(func: &mut IrFunction) {
    // Track known constant values
    let mut constants: HashMap<ValueId, Literal> = HashMap::new();
    // Track constants stored in local slots
    let mut local_constants: HashMap<u32, Literal> = HashMap::new();

    // First pass: collect constants and track local stores
    for block in &func.blocks {
        for op in &block.ops {
            match op {
                IrOp::Const(dst, lit) => {
                    constants.insert(*dst, lit.clone());
                }
                IrOp::StoreLocal(slot, src) => {
                    // If source is a constant, track it in the local slot
                    if let Some(lit) = constants.get(src) {
                        local_constants.insert(*slot, lit.clone());
                    } else {
                        // If we store a non-constant, remove the slot from local_constants
                        local_constants.remove(slot);
                    }
                }
                // Bitwise operations
                IrOp::BitAnd(_, _, _)
                | IrOp::BitOr(_, _, _)
                | IrOp::Xor(_, _, _)
                | IrOp::Shl(_, _, _)
                | IrOp::Shr(_, _, _)
                | IrOp::ShrU(_, _, _)
                | IrOp::Pow(_, _, _) => {}

                // Other operations - no constants to track
                _ => {}
            }
        }
    }

    // Second pass: fold operations and propagate local constants
    for block in &mut func.blocks {
        let ops = std::mem::take(&mut block.ops);
        block.ops = ops
            .into_iter()
            .map(|op| fold_op(op, &mut constants, &local_constants))
            .collect();
    }
}

/// Attempt to fold a single operation.
fn fold_op(
    op: IrOp,
    constants: &mut HashMap<ValueId, Literal>,
    local_constants: &HashMap<u32, Literal>,
) -> IrOp {
    match op {
        // Numeric binary operations
        IrOp::AddNum(dst, a, b) => {
            if let (Some(Literal::Number(va)), Some(Literal::Number(vb))) =
                (constants.get(&a), constants.get(&b))
            {
                let result = va + vb;
                constants.insert(dst, Literal::Number(result));
                return IrOp::Const(dst, Literal::Number(result));
            }
            IrOp::AddNum(dst, a, b)
        }

        IrOp::SubNum(dst, a, b) => {
            if let (Some(Literal::Number(va)), Some(Literal::Number(vb))) =
                (constants.get(&a), constants.get(&b))
            {
                let result = va - vb;
                constants.insert(dst, Literal::Number(result));
                return IrOp::Const(dst, Literal::Number(result));
            }
            IrOp::SubNum(dst, a, b)
        }

        IrOp::MulNum(dst, a, b) => {
            if let (Some(Literal::Number(va)), Some(Literal::Number(vb))) =
                (constants.get(&a), constants.get(&b))
            {
                let result = va * vb;
                constants.insert(dst, Literal::Number(result));
                return IrOp::Const(dst, Literal::Number(result));
            }
            IrOp::MulNum(dst, a, b)
        }

        IrOp::DivNum(dst, a, b) => {
            if let (Some(Literal::Number(va)), Some(Literal::Number(vb))) =
                (constants.get(&a), constants.get(&b))
            {
                if *vb != 0.0 {
                    let result = va / vb;
                    constants.insert(dst, Literal::Number(result));
                    return IrOp::Const(dst, Literal::Number(result));
                }
            }
            IrOp::DivNum(dst, a, b)
        }

        IrOp::ModNum(dst, a, b) => {
            if let (Some(Literal::Number(va)), Some(Literal::Number(vb))) =
                (constants.get(&a), constants.get(&b))
            {
                if *vb != 0.0 {
                    let result = va % vb;
                    constants.insert(dst, Literal::Number(result));
                    return IrOp::Const(dst, Literal::Number(result));
                }
            }
            IrOp::ModNum(dst, a, b)
        }

        IrOp::NegNum(dst, a) => {
            if let Some(Literal::Number(va)) = constants.get(&a) {
                let result = -va;
                constants.insert(dst, Literal::Number(result));
                return IrOp::Const(dst, Literal::Number(result));
            }
            IrOp::NegNum(dst, a)
        }

        // Comparison operations
        IrOp::Lt(dst, a, b) => {
            if let (Some(Literal::Number(va)), Some(Literal::Number(vb))) =
                (constants.get(&a), constants.get(&b))
            {
                let result = va < vb;
                constants.insert(dst, Literal::Boolean(result));
                return IrOp::Const(dst, Literal::Boolean(result));
            }
            IrOp::Lt(dst, a, b)
        }

        IrOp::LtEq(dst, a, b) => {
            if let (Some(Literal::Number(va)), Some(Literal::Number(vb))) =
                (constants.get(&a), constants.get(&b))
            {
                let result = va <= vb;
                constants.insert(dst, Literal::Boolean(result));
                return IrOp::Const(dst, Literal::Boolean(result));
            }
            IrOp::LtEq(dst, a, b)
        }

        IrOp::Gt(dst, a, b) => {
            if let (Some(Literal::Number(va)), Some(Literal::Number(vb))) =
                (constants.get(&a), constants.get(&b))
            {
                let result = va > vb;
                constants.insert(dst, Literal::Boolean(result));
                return IrOp::Const(dst, Literal::Boolean(result));
            }
            IrOp::Gt(dst, a, b)
        }

        IrOp::GtEq(dst, a, b) => {
            if let (Some(Literal::Number(va)), Some(Literal::Number(vb))) =
                (constants.get(&a), constants.get(&b))
            {
                let result = va >= vb;
                constants.insert(dst, Literal::Boolean(result));
                return IrOp::Const(dst, Literal::Boolean(result));
            }
            IrOp::GtEq(dst, a, b)
        }

        IrOp::EqStrict(dst, a, b) => {
            if let (Some(la), Some(lb)) = (constants.get(&a), constants.get(&b)) {
                let result = la == lb;
                constants.insert(dst, Literal::Boolean(result));
                return IrOp::Const(dst, Literal::Boolean(result));
            }
            IrOp::EqStrict(dst, a, b)
        }

        IrOp::NeStrict(dst, a, b) => {
            if let (Some(la), Some(lb)) = (constants.get(&a), constants.get(&b)) {
                let result = la != lb;
                constants.insert(dst, Literal::Boolean(result));
                return IrOp::Const(dst, Literal::Boolean(result));
            }
            IrOp::NeStrict(dst, a, b)
        }

        // Logical NOT
        IrOp::Not(dst, a) => {
            if let Some(Literal::Boolean(va)) = constants.get(&a) {
                let result = !va;
                constants.insert(dst, Literal::Boolean(result));
                return IrOp::Const(dst, Literal::Boolean(result));
            }
            IrOp::Not(dst, a)
        }

        // Copy propagation: if source is constant, replace with constant
        IrOp::Copy(dst, src) => {
            if let Some(lit) = constants.get(&src).cloned() {
                constants.insert(dst, lit.clone());
                return IrOp::Const(dst, lit);
            }
            IrOp::Copy(dst, src)
        }

        // LoadLocal: propagate constant from local slot if available
        IrOp::LoadLocal(dst, slot) => {
            if let Some(lit) = local_constants.get(&slot).cloned() {
                constants.insert(dst, lit.clone());
                return IrOp::Const(dst, lit);
            }
            IrOp::LoadLocal(dst, slot)
        }

        // StoreLocal: update local_constants tracking
        IrOp::StoreLocal(slot, src) => {
            // Note: We can't modify local_constants here since it's immutable,
            // but the first pass already handles this. This is just for consistency.
            IrOp::StoreLocal(slot, src)
        }

        // All other operations pass through
        other => other,
    }
}

// ============================================================================
// Common Subexpression Elimination
// ============================================================================

/// A key for identifying equivalent expressions.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum ExprKey {
    Binary(&'static str, ValueId, ValueId),
    Unary(&'static str, ValueId),
    LoadLocal(u32),
    LoadGlobal(String),
    GetProp(ValueId, String),
}

/// Eliminate redundant computations.
pub fn common_subexpression_elimination(func: &mut IrFunction) {
    // Map from expression to the value that computes it
    let mut available: HashMap<ExprKey, ValueId> = HashMap::new();

    for block in &mut func.blocks {
        // Clear available expressions at block boundaries (conservative)
        available.clear();

        let ops = std::mem::take(&mut block.ops);
        let mut new_ops = Vec::with_capacity(ops.len());

        for op in ops {
            if let Some((key, dst)) = extract_expr_key(&op) {
                if let Some(&existing) = available.get(&key) {
                    // Replace with copy
                    new_ops.push(IrOp::Copy(dst, existing));
                    continue;
                }
                available.insert(key, dst);
            }

            // Invalidate expressions that may be affected by side effects
            if has_side_effects(&op) {
                // Conservative: clear all for stores
                if matches!(
                    op,
                    IrOp::StoreLocal(_, _)
                        | IrOp::StoreGlobal(_, _)
                        | IrOp::SetProp(_, _, _)
                        | IrOp::SetElement(_, _, _)
                ) {
                    available.clear();
                }
            }

            new_ops.push(op);
        }

        block.ops = new_ops;
    }
}

/// Extract an expression key from an operation.
fn extract_expr_key(op: &IrOp) -> Option<(ExprKey, ValueId)> {
    match op {
        IrOp::AddNum(d, a, b) => Some((ExprKey::Binary("add.num", *a, *b), *d)),
        IrOp::SubNum(d, a, b) => Some((ExprKey::Binary("sub.num", *a, *b), *d)),
        IrOp::MulNum(d, a, b) => Some((ExprKey::Binary("mul.num", *a, *b), *d)),
        IrOp::DivNum(d, a, b) => Some((ExprKey::Binary("div.num", *a, *b), *d)),
        IrOp::ModNum(d, a, b) => Some((ExprKey::Binary("mod.num", *a, *b), *d)),
        IrOp::NegNum(d, a) => Some((ExprKey::Unary("neg.num", *a), *d)),
        IrOp::Lt(d, a, b) => Some((ExprKey::Binary("lt", *a, *b), *d)),
        IrOp::LtEq(d, a, b) => Some((ExprKey::Binary("le", *a, *b), *d)),
        IrOp::Gt(d, a, b) => Some((ExprKey::Binary("gt", *a, *b), *d)),
        IrOp::GtEq(d, a, b) => Some((ExprKey::Binary("ge", *a, *b), *d)),
        IrOp::EqStrict(d, a, b) => Some((ExprKey::Binary("eq", *a, *b), *d)),
        IrOp::NeStrict(d, a, b) => Some((ExprKey::Binary("ne", *a, *b), *d)),
        IrOp::Not(d, a) => Some((ExprKey::Unary("not", *a), *d)),
        IrOp::LoadLocal(d, slot) => Some((ExprKey::LoadLocal(*slot), *d)),
        IrOp::LoadGlobal(d, name) => Some((ExprKey::LoadGlobal(name.clone()), *d)),
        IrOp::GetProp(d, obj, name) => Some((ExprKey::GetProp(*obj, name.clone()), *d)),
        // Bitwise operations - not supported for CSE yet
        IrOp::BitAnd(_, _, _)
        | IrOp::BitOr(_, _, _)
        | IrOp::Xor(_, _, _)
        | IrOp::Shl(_, _, _)
        | IrOp::Shr(_, _, _)
        | IrOp::ShrU(_, _, _)
        | IrOp::Pow(_, _, _) => None,

        // Other operations - not supported for CSE
        _ => None,
    }
}

// ============================================================================
// Copy Propagation
// ============================================================================

/// Replace uses of copied values with their source.
pub fn copy_propagation(func: &mut IrFunction) {
    // Build copy chains
    let mut copies: HashMap<ValueId, ValueId> = HashMap::new();

    for block in &func.blocks {
        for op in &block.ops {
            if let IrOp::Copy(dst, src) = op {
                // Follow chains
                let mut root = *src;
                while let Some(&next) = copies.get(&root) {
                    root = next;
                }
                copies.insert(*dst, root);
            }
        }
    }

    if copies.is_empty() {
        return;
    }

    // Replace uses
    for block in &mut func.blocks {
        for op in &mut block.ops {
            replace_uses_in_op(op, &copies);
        }

        replace_uses_in_terminator(&mut block.terminator, &copies);
    }
}

/// Replace uses in an operation.
fn replace_uses_in_op(op: &mut IrOp, copies: &HashMap<ValueId, ValueId>) {
    let resolve = |v: &mut ValueId| {
        if let Some(&src) = copies.get(v) {
            *v = src;
        }
    };

    match op {
        IrOp::AddNum(_, a, b)
        | IrOp::SubNum(_, a, b)
        | IrOp::MulNum(_, a, b)
        | IrOp::DivNum(_, a, b)
        | IrOp::ModNum(_, a, b)
        | IrOp::AddAny(_, a, b)
        | IrOp::SubAny(_, a, b)
        | IrOp::MulAny(_, a, b)
        | IrOp::DivAny(_, a, b)
        | IrOp::ModAny(_, a, b)
        | IrOp::EqStrict(_, a, b)
        | IrOp::NeStrict(_, a, b)
        | IrOp::Lt(_, a, b)
        | IrOp::LtEq(_, a, b)
        | IrOp::Gt(_, a, b)
        | IrOp::GtEq(_, a, b)
        | IrOp::And(_, a, b)
        | IrOp::Or(_, a, b)
        | IrOp::BitAnd(_, a, b)
        | IrOp::BitOr(_, a, b)
        | IrOp::Xor(_, a, b)
        | IrOp::Shl(_, a, b)
        | IrOp::Shr(_, a, b)
        | IrOp::ShrU(_, a, b)
        | IrOp::Pow(_, a, b) => {
            resolve(a);
            resolve(b);
        }

        IrOp::NegNum(_, a)
        | IrOp::NegAny(_, a)
        | IrOp::Not(_, a)
        | IrOp::ToBool(_, a)
        | IrOp::ToNum(_, a)
        | IrOp::Copy(_, a)
        | IrOp::ArrayLen(_, a)
        | IrOp::TypeCheck(_, a, _)
        | IrOp::TypeGuard(_, a, _)
        | IrOp::Borrow(_, a)
        | IrOp::BorrowMut(_, a)
        | IrOp::Deref(_, a)
        | IrOp::EndBorrow(a)
        | IrOp::Move(_, a)
        | IrOp::Clone(_, a)
        | IrOp::StructGetField(_, a, _)
        | IrOp::StructGetFieldNamed(_, a, _)
        | IrOp::TypeOf(_, a)
        | IrOp::DeleteProp(_, a, _) => {
            resolve(a);
        }

        IrOp::StoreLocal(_, v) | IrOp::StoreGlobal(_, v) => {
            resolve(v);
        }

        IrOp::GetProp(_, obj, _) => {
            resolve(obj);
        }

        IrOp::SetProp(obj, _, val) => {
            resolve(obj);
            resolve(val);
        }

        IrOp::GetElement(_, obj, key) => {
            resolve(obj);
            resolve(key);
        }

        IrOp::SetElement(obj, key, val) => {
            resolve(obj);
            resolve(key);
            resolve(val);
        }

        IrOp::ArrayPush(arr, val) => {
            resolve(arr);
            resolve(val);
        }

        IrOp::Call(_, func_val, args) => {
            resolve(func_val);
            for arg in args {
                resolve(arg);
            }
        }

        IrOp::CallMethod(_, obj, _, args) => {
            resolve(obj);
            for arg in args {
                resolve(arg);
            }
        }

        IrOp::MakeClosure(_, _, env) => {
            resolve(env);
        }

        IrOp::Phi(_, entries) => {
            for (_, val) in entries {
                resolve(val);
            }
        }

        IrOp::DerefStore(a, b)
        | IrOp::StructSetField(a, _, b)
        | IrOp::StructSetFieldNamed(a, _, b) => {
            resolve(a);
            resolve(b);
        }

        IrOp::CallMono(_, _, args) => {
            for arg in args {
                resolve(arg);
            }
        }

        // No uses to replace
        IrOp::Const(_, _)
        | IrOp::LoadLocal(_, _)
        | IrOp::LoadGlobal(_, _)
        | IrOp::NewObject(_)
        | IrOp::NewArray(_)
        | IrOp::LoadThis(_)
        | IrOp::StructNew(_, _) => {}
    }
}

/// Replace uses in a terminator.
fn replace_uses_in_terminator(term: &mut Terminator, copies: &HashMap<ValueId, ValueId>) {
    let resolve = |v: &mut ValueId| {
        if let Some(&src) = copies.get(v) {
            *v = src;
        }
    };

    match term {
        Terminator::Branch(cond, _, _) => {
            resolve(cond);
        }
        Terminator::Return(Some(val)) => {
            resolve(val);
        }
        Terminator::Jump(_) | Terminator::Return(None) | Terminator::Unreachable => {}
    }
}

// ============================================================================
// Unreachable Block Elimination
// ============================================================================

/// Remove blocks that are not reachable from the entry.
pub fn remove_unreachable_blocks(func: &mut IrFunction) {
    let mut reachable = HashSet::new();
    let mut worklist = vec![func.entry_block()];

    // Find all reachable blocks
    while let Some(block_id) = worklist.pop() {
        if reachable.contains(&block_id) {
            continue;
        }
        reachable.insert(block_id);

        let block = func.block(block_id);
        for succ in block.terminator.successors() {
            if !reachable.contains(&succ) {
                worklist.push(succ);
            }
        }
    }

    // Keep only reachable blocks
    // Note: This invalidates block IDs, so we need to be careful
    // For now, just clear unreachable blocks rather than removing them
    for block in &mut func.blocks {
        if !reachable.contains(&block.id) {
            block.ops.clear();
            block.terminator = Terminator::Unreachable;
        }
    }
}

// ============================================================================
// Branch Simplification
// ============================================================================

/// Simplify branches with constant conditions.
pub fn simplify_branches(func: &mut IrFunction) {
    // Collect constant values
    let mut constants: HashMap<ValueId, Literal> = HashMap::new();
    for block in &func.blocks {
        for op in &block.ops {
            if let IrOp::Const(dst, lit) = op {
                constants.insert(*dst, lit.clone());
            }
        }
    }

    // Simplify branch terminators
    for block in &mut func.blocks {
        if let Terminator::Branch(cond, true_block, false_block) = &block.terminator {
            if let Some(Literal::Boolean(b)) = constants.get(cond) {
                let target = if *b { *true_block } else { *false_block };
                block.terminator = Terminator::Jump(target);
            }
        }
    }
}

// ============================================================================
// Optimization Pipeline
// ============================================================================

/// Run all optimizations on a function.
pub fn optimize_function(func: &mut IrFunction) {
    // Run passes until no changes
    for _ in 0..10 {
        let before = format!("{}", func);

        constant_folding(func);
        copy_propagation(func);
        dead_code_elimination(func);
        common_subexpression_elimination(func);
        simplify_branches(func);
        remove_unreachable_blocks(func);

        let after = format!("{}", func);
        if before == after {
            break;
        }
    }

    func.compute_predecessors();
}

/// Run all optimizations on a module.
pub fn optimize_module(module: &mut IrModule) {
    for func in &mut module.functions {
        optimize_function(func);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_constant_folding() {
        let mut func = IrFunction::new("test".to_string());
        let entry = func.alloc_block();

        let a = func.alloc_value(IrType::Number);
        let b = func.alloc_value(IrType::Number);
        let c = func.alloc_value(IrType::Number);

        {
            let block = func.block_mut(entry);
            block.push(IrOp::Const(a, Literal::Number(2.0)));
            block.push(IrOp::Const(b, Literal::Number(3.0)));
            block.push(IrOp::AddNum(c, a, b));
            block.terminate(Terminator::Return(Some(c)));
        }

        constant_folding(&mut func);

        // c should now be a constant 5.0
        let ops = &func.blocks[entry.0 as usize].ops;
        let has_const_5 = ops
            .iter()
            .any(|op| matches!(op, IrOp::Const(_, Literal::Number(n)) if (*n - 5.0).abs() < 0.001));
        assert!(has_const_5, "Should fold 2+3 to 5");
    }

    #[test]
    fn test_dead_code_elimination() {
        let mut func = IrFunction::new("test".to_string());
        let entry = func.alloc_block();

        let a = func.alloc_value(IrType::Number);
        let b = func.alloc_value(IrType::Number);
        let _unused = func.alloc_value(IrType::Number); // unused result

        {
            let block = func.block_mut(entry);
            block.push(IrOp::Const(a, Literal::Number(1.0)));
            block.push(IrOp::Const(b, Literal::Number(2.0)));
            block.push(IrOp::AddNum(_unused, a, b)); // Result not used
            block.terminate(Terminator::Return(Some(a)));
        }

        dead_code_elimination(&mut func);

        // The unused add should be removed
        let ops = &func.blocks[entry.0 as usize].ops;
        let has_add = ops.iter().any(|op| matches!(op, IrOp::AddNum(_, _, _)));
        assert!(!has_add, "Unused add should be eliminated");
    }

    #[test]
    fn test_cse() {
        let mut func = IrFunction::new("test".to_string());
        let entry = func.alloc_block();

        let a = func.alloc_value(IrType::Number);
        let b = func.alloc_value(IrType::Number);
        let c = func.alloc_value(IrType::Number);
        let d = func.alloc_value(IrType::Number); // Same as c

        {
            let block = func.block_mut(entry);
            block.push(IrOp::Const(a, Literal::Number(1.0)));
            block.push(IrOp::Const(b, Literal::Number(2.0)));
            block.push(IrOp::AddNum(c, a, b));
            block.push(IrOp::AddNum(d, a, b)); // Duplicate
            block.terminate(Terminator::Return(Some(d)));
        }

        common_subexpression_elimination(&mut func);

        // d should be replaced with a copy of c
        let ops = &func.blocks[entry.0 as usize].ops;
        let has_copy = ops.iter().any(|op| matches!(op, IrOp::Copy(_, _)));
        assert!(has_copy, "Duplicate add should become copy");
    }

    #[test]
    fn test_branch_simplification() {
        let mut func = IrFunction::new("test".to_string());
        let entry = func.alloc_block();
        let then_block = func.alloc_block();
        let else_block = func.alloc_block();

        let cond = func.alloc_value(IrType::Boolean);
        let one = func.alloc_value(IrType::Number);
        let two = func.alloc_value(IrType::Number);

        {
            let block = func.block_mut(entry);
            block.push(IrOp::Const(cond, Literal::Boolean(true)));
            block.terminate(Terminator::Branch(cond, then_block, else_block));
        }

        {
            let block = func.block_mut(then_block);
            block.push(IrOp::Const(one, Literal::Number(1.0)));
            block.terminate(Terminator::Return(Some(one)));
        }

        {
            let block = func.block_mut(else_block);
            block.push(IrOp::Const(two, Literal::Number(2.0)));
            block.terminate(Terminator::Return(Some(two)));
        }

        simplify_branches(&mut func);

        // Entry should now have an unconditional jump to then_block
        let term = &func.blocks[entry.0 as usize].terminator;
        assert!(
            matches!(term, Terminator::Jump(target) if *target == then_block),
            "Branch should be simplified to jump"
        );
    }
}
