//! IR serialization and deserialization for deterministic output.
//!
//! This module provides functions to write IR in a canonical text format
//! that is deterministic and suitable for:
//! - Build caching and incrementality
//! - Build reproducibility verification
//! - Cross-version IR validation
//!
//! # Design Principles
//!
//! - **Deterministic ordering**: Functions, blocks, and values are ordered consistently.
//! - **Human-readable**: Text format that can be inspected and debugged.
//! - **Versioned**: Includes IR format version for forward/backward compatibility.

use crate::ir::{IrFunction, IrModule, IrOp, Literal, Terminator};
use crate::runtime::ABI_VERSION;
use std::fs;
use std::io::{self, Write};
use std::path::Path;

/// Current IR format version (bumped on any breaking change to the format).
pub const IR_FORMAT_VERSION: u32 = 1;

/// Write an IR module to a file in canonical text format.
pub fn write_ir_to_file(module: &IrModule, path: &Path) -> io::Result<()> {
    let content = serialize_module(module);
    fs::write(path, content)
}

/// Serialize an IR module to a canonical string representation.
pub fn serialize_module(module: &IrModule) -> String {
    let mut output = String::new();

    // Header
    output.push_str("; ============================================================\n");
    output.push_str("; tscl IR Module\n");
    output.push_str(&format!("; Format version: {}\n", IR_FORMAT_VERSION));
    output.push_str(&format!("; ABI version: {}\n", ABI_VERSION));
    output.push_str("; ============================================================\n");
    output.push('\n');

    // Struct definitions (sorted by ID for determinism)
    let mut struct_ids: Vec<_> = module.structs.keys().collect();
    struct_ids.sort_by_key(|id| id.0);

    if !struct_ids.is_empty() {
        output.push_str("; Struct definitions\n");
        for sid in &struct_ids {
            if let Some(struct_def) = module.structs.get(sid) {
                output.push_str(&format!("struct {} {{\n", struct_def.name));
                for (name, ty, offset) in &struct_def.fields {
                    output.push_str(&format!("    {}: {} // offset: {}\n", name, ty, offset));
                }
                output.push_str(&format!(
                    "    // size: {}, alignment: {}\n",
                    struct_def.size, struct_def.alignment
                ));
                output.push_str("}\n\n");
            }
        }
    }

    // Functions (sorted by name for determinism)
    let mut functions: Vec<_> = module.functions.iter().enumerate().collect();
    functions.sort_by_key(|(_, f)| f.name.clone());

    for (sorted_idx, func) in &functions {
        serialize_function(&mut output, func, *sorted_idx);
        output.push('\n');
    }

    output
}

/// Serialize a single function.
fn serialize_function(output: &mut String, func: &IrFunction, _sorted_idx: usize) {
    // Function signature
    output.push_str("fn ");
    output.push_str(&func.name);
    output.push('(');

    let param_strs: Vec<String> = func
        .params
        .iter()
        .map(|(name, ty)| format!("{}: {}", name, ty))
        .collect();
    output.push_str(&param_strs.join(", "));
    output.push_str(&format!(") -> {}", func.return_ty));
    output.push_str(" {\n");

    // Local variables (in order)
    if !func.locals.is_empty() {
        output.push_str("    ; Local variables\n");
        for (i, (name, ty)) in func.locals.iter().enumerate() {
            output.push_str(&format!("    local ${}: {} = {}\n", i, ty, name));
        }
        output.push('\n');
    }

    // Basic blocks (in creation order for determinism)
    for block in &func.blocks {
        serialize_block(output, block);
    }

    output.push_str("}\n");
}

/// Serialize a basic block.
fn serialize_block(output: &mut String, block: &crate::ir::BasicBlock) {
    output.push_str(&format!("{}:\n", block.id));

    // Operations
    for op in &block.ops {
        output.push_str("    ");
        serialize_op(output, op);
        output.push('\n');
    }

    // Terminator
    output.push_str("    ");
    serialize_terminator(output, &block.terminator);
    output.push('\n');
}

/// Serialize an operation.
fn serialize_op(output: &mut String, op: &IrOp) {
    match op {
        IrOp::Const(d, lit) => {
            output.push_str(&format!("{} = const {}", d, lit));
        }
        IrOp::AddNum(d, a, b) => output.push_str(&format!("{} = add.num {}, {}", d, a, b)),
        IrOp::SubNum(d, a, b) => output.push_str(&format!("{} = sub.num {}, {}", d, a, b)),
        IrOp::MulNum(d, a, b) => output.push_str(&format!("{} = mul.num {}, {}", d, a, b)),
        IrOp::DivNum(d, a, b) => output.push_str(&format!("{} = div.num {}, {}", d, a, b)),
        IrOp::ModNum(d, a, b) => output.push_str(&format!("{} = mod.num {}, {}", d, a, b)),
        IrOp::NegNum(d, a) => output.push_str(&format!("{} = neg.num {}", d, a)),
        IrOp::AddAny(d, a, b) => output.push_str(&format!("{} = add.any {}, {}", d, a, b)),
        IrOp::SubAny(d, a, b) => output.push_str(&format!("{} = sub.any {}, {}", d, a, b)),
        IrOp::MulAny(d, a, b) => output.push_str(&format!("{} = mul.any {}, {}", d, a, b)),
        IrOp::DivAny(d, a, b) => output.push_str(&format!("{} = div.any {}, {}", d, a, b)),
        IrOp::ModAny(d, a, b) => output.push_str(&format!("{} = mod.any {}, {}", d, a, b)),
        IrOp::NegAny(d, a) => output.push_str(&format!("{} = neg.any {}", d, a)),
        IrOp::EqStrict(d, a, b) => output.push_str(&format!("{} = eq.strict {}, {}", d, a, b)),
        IrOp::NeStrict(d, a, b) => output.push_str(&format!("{} = ne.strict {}, {}", d, a, b)),
        IrOp::Lt(d, a, b) => output.push_str(&format!("{} = lt {}, {}", d, a, b)),
        IrOp::LtEq(d, a, b) => output.push_str(&format!("{} = le {}, {}", d, a, b)),
        IrOp::Gt(d, a, b) => output.push_str(&format!("{} = gt {}, {}", d, a, b)),
        IrOp::GtEq(d, a, b) => output.push_str(&format!("{} = ge {}, {}", d, a, b)),
        IrOp::Not(d, a) => output.push_str(&format!("{} = not {}", d, a)),
        IrOp::And(d, a, b) => output.push_str(&format!("{} = and {}, {}", d, a, b)),
        IrOp::Or(d, a, b) => output.push_str(&format!("{} = or {}, {}", d, a, b)),
        IrOp::BitAnd(d, a, b) => output.push_str(&format!("{} = and {}, {}", d, a, b)),
        IrOp::BitOr(d, a, b) => output.push_str(&format!("{} = or {}, {}", d, a, b)),
        IrOp::Xor(d, a, b) => output.push_str(&format!("{} = xor {}, {}", d, a, b)),
        IrOp::Shl(d, a, b) => output.push_str(&format!("{} = shl {}, {}", d, a, b)),
        IrOp::Shr(d, a, b) => output.push_str(&format!("{} = shr {}, {}", d, a, b)),
        IrOp::ShrU(d, a, b) => output.push_str(&format!("{} = shr.u {}, {}", d, a, b)),
        IrOp::Pow(d, a, b) => output.push_str(&format!("{} = pow {}, {}", d, a, b)),
        IrOp::LoadLocal(d, slot) => output.push_str(&format!("{} = load.local ${}", d, slot)),
        IrOp::StoreLocal(slot, v) => output.push_str(&format!("store.local ${}, {}", slot, v)),
        IrOp::LoadGlobal(d, name) => output.push_str(&format!("{} = load.global @{}", d, name)),
        IrOp::StoreGlobal(name, v) => output.push_str(&format!("store.global @{}, {}", name, v)),
        IrOp::NewObject(d) => output.push_str(&format!("{} = new.object", d)),
        IrOp::GetProp(d, obj, name) => {
            output.push_str(&format!("{} = get.prop {}, .{}", d, obj, name))
        }
        IrOp::SetProp(obj, name, val) => {
            output.push_str(&format!("set.prop {}, .{}, {}", obj, name, val))
        }
        IrOp::GetElement(d, obj, key) => {
            output.push_str(&format!("{} = get.elem {}, [{}]", d, obj, key))
        }
        IrOp::SetElement(obj, key, val) => {
            output.push_str(&format!("set.elem {}, [{}], {}", obj, key, val))
        }
        IrOp::NewArray(d) => output.push_str(&format!("{} = new.array", d)),
        IrOp::ArrayLen(d, arr) => output.push_str(&format!("{} = array.len {}", d, arr)),
        IrOp::ArrayPush(arr, val) => output.push_str(&format!("array.push {}, {}", arr, val)),
        IrOp::Call(d, func, args) => {
            let args_str: Vec<String> = args.iter().map(|a| a.to_string()).collect();
            output.push_str(&format!("{} = call {}({})", d, func, args_str.join(", ")));
        }
        IrOp::CallMethod(d, obj, method, args) => {
            let args_str: Vec<String> = args.iter().map(|a| a.to_string()).collect();
            output.push_str(&format!(
                "{} = call.method {}.{}({})",
                d,
                obj,
                method,
                args_str.join(", ")
            ));
        }
        IrOp::MakeClosure(d, func_id, env) => {
            output.push_str(&format!("{} = make.closure func#{}, {}", d, func_id, env));
        }
        IrOp::TypeCheck(d, v, ty) => output.push_str(&format!("{} = typecheck {}, {}", d, v, ty)),
        IrOp::TypeGuard(d, v, ty) => output.push_str(&format!("{} = typeguard {}, {}", d, v, ty)),
        IrOp::ToBool(d, v) => output.push_str(&format!("{} = to.bool {}", d, v)),
        IrOp::ToNum(d, v) => output.push_str(&format!("{} = to.num {}", d, v)),
        IrOp::Phi(d, entries) => {
            let entries_str: Vec<String> = entries
                .iter()
                .map(|(b, v)| format!("[{}: {}]", b, v))
                .collect();
            output.push_str(&format!("{} = phi {}", d, entries_str.join(", ")));
        }
        IrOp::Copy(d, s) => output.push_str(&format!("{} = copy {}", d, s)),
        IrOp::LoadThis(d) => output.push_str(&format!("{} = load.this", d)),
        IrOp::Borrow(d, s) => output.push_str(&format!("{} = borrow {}", d, s)),
        IrOp::BorrowMut(d, s) => output.push_str(&format!("{} = borrow.mut {}", d, s)),
        IrOp::Deref(d, s) => output.push_str(&format!("{} = deref {}", d, s)),
        IrOp::DerefStore(dst, val) => output.push_str(&format!("deref.store {}, {}", dst, val)),
        IrOp::EndBorrow(v) => output.push_str(&format!("end.borrow {}", v)),
        IrOp::StructNew(d, id) => output.push_str(&format!("{} = struct.new {}", d, id)),
        IrOp::StructGetField(d, src, field) => {
            output.push_str(&format!("{} = struct.get {}, {}", d, src, field));
        }
        IrOp::StructSetField(dst, field, val) => {
            output.push_str(&format!("struct.set {}, {}, {}", dst, field, val));
        }
        IrOp::StructGetFieldNamed(d, src, name) => {
            output.push_str(&format!("{} = struct.get {}, .{}", d, src, name));
        }
        IrOp::StructSetFieldNamed(dst, name, val) => {
            output.push_str(&format!("struct.set {}, .{}, {}", dst, name, val));
        }
        IrOp::CallMono(d, mono_id, args) => {
            let args_str: Vec<String> = args.iter().map(|a| a.to_string()).collect();
            output.push_str(&format!(
                "{} = call.mono {}({})",
                d,
                mono_id,
                args_str.join(", ")
            ));
        }
        IrOp::Move(d, s) => output.push_str(&format!("{} = move {}", d, s)),
        IrOp::Clone(d, s) => output.push_str(&format!("{} = clone {}", d, s)),
    }
}

/// Serialize a terminator.
fn serialize_terminator(output: &mut String, term: &Terminator) {
    match term {
        Terminator::Jump(target) => output.push_str(&format!("jump {}", target)),
        Terminator::Branch(cond, t, fa) => {
            output.push_str(&format!("branch {}, {}, {}", cond, t, fa));
        }
        Terminator::Return(Some(v)) => output.push_str(&format!("return {}", v)),
        Terminator::Return(None) => output.push_str("return"),
        Terminator::Unreachable => output.push_str("unreachable"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_serialize_module() {
        let mut module = IrModule::new();
        let mut func = IrFunction::new("add".to_string());
        func.params
            .push(("a".to_string(), crate::ir::IrType::Number));
        func.params
            .push(("b".to_string(), crate::ir::IrType::Number));
        func.return_ty = crate::ir::IrType::Number;

        let entry = func.alloc_block();
        let a = func.alloc_value(crate::ir::IrType::Number);
        let b = func.alloc_value(crate::ir::IrType::Number);
        let result = func.alloc_value(crate::ir::IrType::Number);

        func.add_local("a".to_string(), crate::ir::IrType::Number);
        func.add_local("b".to_string(), crate::ir::IrType::Number);

        {
            let block = func.block_mut(entry);
            block.push(IrOp::LoadLocal(a, 0));
            block.push(IrOp::LoadLocal(b, 1));
            block.push(IrOp::AddNum(result, a, b));
            block.terminate(Terminator::Return(Some(result)));
        }

        module.add_function(func);

        let serialized = serialize_module(&module);
        assert!(serialized.contains("fn add(a: num, b: num) -> num"));
        assert!(serialized.contains("add.num"));
        assert!(serialized.contains("return"));
    }

    #[test]
    fn test_ir_format_version() {
        assert_eq!(IR_FORMAT_VERSION, 1);
    }
}
