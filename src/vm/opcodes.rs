use crate::vm::value::JsValue;

#[derive(Debug, Clone)]
pub enum OpCode {
    Push(JsValue),
    Add,
    Sub,
    #[allow(dead_code)]
    Print,
    Pop,
    Store(String),
    Load(String),
    Drop(String),
    Call,
    Return,
    Jump(usize),
    NewObject,
    SetProp(String),
    GetProp(String),
    Dup,
    Eq,
    Lt,
    Gt,
    NewArray(usize), // Creates array of size N
    StoreElement,    // Pops index, value, and array_ptr -> arr[idx] = val
    LoadElement,     // Pops index and array_ptr -> pushes arr[idx]
    JumpIfFalse(usize),
    Halt,
}
