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
    /// A function value with its code address and optional captured environment.
    /// The `env` field points to a HeapObject containing variables "lifted" from
    /// the enclosing scope. This enables closures to survive after their
    /// defining scope's stack frame is destroyed.
    Function {
        address: usize,
        env: Option<usize>, // Points to HeapObject with captured variables
    },
    NativeFunction(usize),
    Null,
    Undefined,
    /// Accessor descriptor for getters/setters
    /// First element is getter function, second is setter function
    Accessor(Option<Box<JsValue>>, Option<Box<JsValue>>),
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
    /// ByteStream for building binary bytecode buffers
    ByteStream(Vec<u8>),
}
