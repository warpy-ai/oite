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

/// Whether `name` is stored as an ordinary data property.
fn is_data_name(name: &str) -> bool {
    name != "__proto__" && !name.starts_with("getter:") && !name.starts_with("setter:")
}

/// For each `NewObject` in `func`, the properties the code right after it
/// stores on the new object before anything else can see the object: the
/// leading `SetProp`s with distinct data names. Such an object can be
/// allocated already in the shape it has after those stores, and each
/// store then writes its slot directly.
///
/// The names are in store order: the n-th of the object's first `SetProp`s
/// writes slot n. Sites with no such property, or more than `max`, are left
/// out.
pub fn literal_props(func: &IrFunction, max: usize) -> HashMap<ValueId, Vec<String>> {
    let mut literals = HashMap::new();
    for block in &func.blocks {
        for (i, op) in block.ops.iter().enumerate() {
            let IrOp::NewObject(obj) = op else { continue };
            let mut names: Vec<String> = Vec::new();
            for later in &block.ops[i + 1..] {
                match later {
                    IrOp::SetProp(target, name, value)
                        if target == obj
                            && value != obj
                            && is_data_name(name)
                            && !names.contains(name) =>
                    {
                        names.push(name.clone());
                    }
                    // Anything else that touches the object ends the run.
                    other if other.uses().contains(obj) => break,
                    _ => {}
                }
            }
            if !names.is_empty() && names.len() <= max {
                literals.insert(*obj, names);
            }
        }
    }
    literals
}
