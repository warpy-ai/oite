// Memory representation. We will use a enum to implement ownership,
// and we track wheter a value is "Owned" or a "reference" in the low-level representation
use std::collections::HashMap;

pub type NativeFn = fn(&mut crate::vm::VM, Vec<JsValue>) -> JsValue;

#[derive(Debug, Clone, PartialEq)]
pub enum JsValue {
    Number(f64),
    String(String),
    Boolean(bool),
    // In a real low-level VM, this would be a pointer to a Heap
    Object(usize),
    Function(usize),
    NativeFunction(usize),
    Null,
    Undefined,
}

#[derive(Debug, Clone)]
pub struct HeapObject {
    pub data: HeapData,
}

#[derive(Debug, Clone)]
pub struct HeapArray {
    pub elements: Vec<JsValue>,
}

// Update HeapObject to be an enum of different types of heap data
#[derive(Debug, Clone)]
pub enum HeapData {
    Object(HashMap<String, JsValue>),
    Array(Vec<JsValue>),
}
