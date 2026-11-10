//! Hints for allocation sites.

use super::{IrFunction, IrOp, ValueId};
use std::collections::HashMap;

/// For each `NewObject` in `func`, how many properties the code right
/// after it stores on the new object (in the same block): the slots worth
/// allocating in the object itself. Sites that store none are left out.
///
/// It is only a hint about size; an object takes any number of properties
/// whatever it was allocated with.
pub fn object_slots(func: &IrFunction) -> HashMap<ValueId, usize> {
    let mut slots = HashMap::new();
    for block in &func.blocks {
        for (i, op) in block.ops.iter().enumerate() {
            let IrOp::NewObject(obj) = op else { continue };
            let mut names: Vec<&str> = Vec::new();
            for later in &block.ops[i + 1..] {
                if let IrOp::SetProp(target, name, _) = later
                    && target == obj
                    && name != "__proto__"
                    && !name.starts_with("getter:")
                    && !name.starts_with("setter:")
                    && !names.contains(&name.as_str())
                {
                    names.push(name);
                }
            }
            if !names.is_empty() {
                slots.insert(*obj, names.len());
            }
        }
    }
    slots
}
