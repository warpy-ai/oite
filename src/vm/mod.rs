pub mod opcodes;
pub mod value;
use crate::vm::opcodes::OpCode;
use crate::vm::value::HeapData;
use crate::vm::value::HeapObject;
use crate::vm::value::JsValue;
use crate::vm::value::NativeFn;
use std::collections::HashMap;
pub struct Frame {
    pub return_address: usize,
    pub locals: HashMap<String, JsValue>,
}

pub struct VM {
    stack: Vec<JsValue>,
    pub call_stack: Vec<Frame>,
    pub heap: Vec<HeapObject>,
    pub native_functions: Vec<NativeFn>,
    ip: usize, // Instruction Pointer
}

impl VM {
    pub fn new() -> Self {
        Self {
            stack: Vec::new(),
            call_stack: vec![Frame {
                return_address: 0,
                locals: HashMap::new(),
            }],
            heap: Vec::new(),
            native_functions: Vec::new(),
            ip: 0,
        }
    }

    pub fn run(&mut self, bytecode: Vec<OpCode>) {
        while self.ip < bytecode.len() {
            let op = &bytecode[self.ip];
            match op {
                OpCode::NewObject => {
                    let ptr = self.heap.len();
                    self.heap.push(HeapObject {
                        data: HeapData::Object(HashMap::new()),
                    });
                    self.stack.push(JsValue::Object(ptr));
                }

                OpCode::SetProp(name) => {
                    let value = self.stack.pop().unwrap();
                    if let Some(JsValue::Object(ptr)) = self.stack.pop()
                        && let Some(HeapObject {
                            data: HeapData::Object(props),
                        }) = self.heap.get_mut(ptr)
                    {
                        props.insert(name.to_string(), value);
                    }
                }

                OpCode::GetProp(name) => {
                    if let Some(JsValue::Object(ptr)) = self.stack.pop()
                        && let Some(heap_item) = self.heap.get(ptr)
                    {
                        match &heap_item.data {
                            HeapData::Object(props) => {
                                let val = props.get(name).cloned().unwrap_or(JsValue::Undefined);
                                self.stack.push(val);
                            }
                            HeapData::Array(_) => {
                                // If accessing .length on an array, handle it here
                                if name == "length" {
                                    // self.stack.push(JsValue::Number(...))
                                } else {
                                    self.stack.push(JsValue::Undefined);
                                }
                            }
                        }
                    }
                }

                OpCode::Push(v) => self.stack.push(v.clone()),

                OpCode::Store(name) => {
                    let val = self.stack.pop().unwrap();
                    self.call_stack
                        .last_mut()
                        .unwrap()
                        .locals
                        .insert(name.clone(), val);
                }

                OpCode::Load(name) => {
                    // Search for variable in the current frame
                    let val = self
                        .call_stack
                        .last()
                        .unwrap()
                        .locals
                        .get(name)
                        .cloned()
                        .unwrap_or(JsValue::Undefined);
                    self.stack.push(val);
                }

                OpCode::Call => {
                    let callee = self.stack.pop().unwrap();
                    match callee {
                        JsValue::Function(address) => {
                            let frame = Frame {
                                return_address: self.ip + 1,
                                locals: HashMap::new(),
                            };
                            self.call_stack.push(frame);
                            self.ip = address;
                            continue; // don't auto-increment ip after jumping into the function
                        }
                        JsValue::NativeFunction(idx) => {
                            // 1. For simplicity, assume console.log takes 1 arg for now
                            let arg = self.stack.pop().unwrap();
                            let func = self.native_functions[idx];
                            let result = func(vec![arg]);
                            self.stack.push(result);
                        }
                        _ => panic!("Target is not callable"),
                    }
                }

                OpCode::Return => {
                    let frame = self.call_stack.pop().unwrap();
                    self.ip = frame.return_address;
                    continue;
                }

                // And ensure Drop handles the String correctly:
                OpCode::Drop(name) => {
                    if let Some(JsValue::Object(ptr)) =
                        self.call_stack.last_mut().unwrap().locals.remove(name)
                        && let Some(HeapObject {
                            data: HeapData::Object(props),
                        }) = self.heap.get_mut(ptr)
                    {
                        // In a low-level language, this is where you 'free' memory.
                        // For our Vec-based heap, we clear the properties to release nested values.
                        props.clear();
                        println!("DEBUG: Memory Freed at Heap Index {}", ptr);
                    }
                }

                OpCode::Add => {
                    if let (Some(JsValue::Number(b)), Some(JsValue::Number(a))) =
                        (self.stack.pop(), self.stack.pop())
                    {
                        self.stack.push(JsValue::Number(a + b));
                    } else {
                        // Handle non-numeric operands by pushing Undefined
                        self.stack.push(JsValue::Undefined);
                    }
                }

                OpCode::Print => {
                    // Printing with an empty stack can happen if codegen emits `Print`
                    // for an expression that doesn't produce a value. Prefer a safe,
                    // JS-like behavior over panicking.
                    let v = self.stack.pop().unwrap_or(JsValue::Undefined);
                    println!("➜ {:?}", v);
                }
                OpCode::Pop => {
                    // Discard the top-of-stack value (expression statement semantics).
                    // Safe if empty to avoid panics from imperfect codegen.
                    let _ = self.stack.pop();
                }
                OpCode::Jump(address) => {
                    self.ip = *address;
                    continue;
                }
                OpCode::JumpIfFalse(target) => {
                    let condition = self.stack.pop().expect("Stack underflow in JumpIfFalse");

                    // Define "Falsy" in your language (0, false, null, undefined)
                    let is_falsy = match condition {
                        JsValue::Boolean(b) => !b,
                        JsValue::Number(n) => n == 0.0,
                        JsValue::Null | JsValue::Undefined => true,
                        _ => false,
                    };

                    if is_falsy {
                        self.ip = *target;
                        continue; // Skip the automatic self.ip += 1 at the end of the loop
                    }
                }
                OpCode::Dup => {
                    let val = self.stack.last().expect("Stack underflow").clone();
                    self.stack.push(val);
                }
                OpCode::Eq => {
                    let b = self.stack.pop().unwrap();
                    let a = self.stack.pop().unwrap();
                    self.stack.push(JsValue::Boolean(a == b));
                }
                OpCode::Lt => {
                    if let (Some(JsValue::Number(b)), Some(JsValue::Number(a))) =
                        (self.stack.pop(), self.stack.pop())
                    {
                        self.stack.push(JsValue::Boolean(a < b));
                    }
                }
                OpCode::Gt => {
                    if let (Some(JsValue::Number(b)), Some(JsValue::Number(a))) =
                        (self.stack.pop(), self.stack.pop())
                    {
                        self.stack.push(JsValue::Boolean(a > b));
                    }
                }
                OpCode::StoreElement => {
                    let index_val = self.stack.pop().unwrap();
                    let value = self.stack.pop().unwrap();
                    let array_ptr = self.stack.pop().unwrap();

                    if let (JsValue::Object(ptr), JsValue::Number(idx)) = (array_ptr, index_val)
                        && let Some(HeapObject {
                            data: HeapData::Array(arr),
                        }) = self.heap.get_mut(ptr)
                    {
                        let i = idx as usize;
                        if i < arr.len() {
                            arr[i] = value;
                        }
                    }
                }
                OpCode::NewArray(size) => {
                    let ptr = self.heap.len();
                    // Pre-allocate with Undefined to mimic JS behavior
                    let elements = vec![JsValue::Undefined; *size];
                    self.heap.push(HeapObject {
                        data: HeapData::Array(elements),
                    });
                    self.stack.push(JsValue::Object(ptr));
                }

                OpCode::LoadElement => {
                    let index_val = self.stack.pop().expect("Missing index");
                    let array_ptr = self.stack.pop().expect("Missing array pointer");

                    if let (JsValue::Object(ptr), JsValue::Number(idx)) = (array_ptr, index_val) {
                        // 1. Get the HeapObject at the pointer
                        if let Some(heap_obj) = self.heap.get(ptr) {
                            // 2. Look inside the 'data' field for the Array variant
                            if let HeapData::Array(arr) = &heap_obj.data {
                                let i = idx as usize;
                                let val = arr.get(i).cloned().unwrap_or(JsValue::Undefined);
                                self.stack.push(val);
                            } else {
                                // Handle cases where someone tries to index a non-array object
                                self.stack.push(JsValue::Undefined);
                            }
                        }
                    }
                }
                OpCode::Sub => {
                    if let (Some(JsValue::Number(b)), Some(JsValue::Number(a))) =
                        (self.stack.pop(), self.stack.pop())
                    {
                        self.stack.push(JsValue::Number(a - b));
                    }
                }
                OpCode::Halt => break,
            }
            self.ip += 1;
        }
    }
}

// Example Native Function
pub fn native_log(args: Vec<JsValue>) -> JsValue {
    for arg in args {
        println!("LOG: {:?}", arg);
    }
    JsValue::Undefined
}
