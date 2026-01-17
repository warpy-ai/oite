use crate::vm::value::JsValue;

#[derive(Debug, Clone)]
pub enum OpCode {
    LoadThis,
    Push(JsValue),
    Add,
    Sub,
    #[allow(dead_code)]
    Print,
    Pop,
    /// Create a new variable binding in the current frame (let declaration)
    Let(String),
    /// Assign to an existing variable (searches frames from inner to outer)
    Store(String),
    Load(String),
    Drop(String),
    Call(usize),
    Return,
    Jump(usize),
    NewObject,
    SetProp(String),
    GetProp(String),
    Dup,
    Eq,   // === (strict equality)
    EqEq, // == (loose equality)
    Ne,   // !== (strict inequality)
    NeEq, // != (loose inequality)
    Lt,
    LtEq, // <=
    Gt,
    GtEq, // >=
    Mod,  // %
    And,
    Or,              // ||
    Not,             // ! (logical not)
    Neg,             // - (unary negation)
    NewArray(usize), // Creates array of size N
    StoreElement,    // Pops index, value, and array_ptr -> arr[idx] = val
    LoadElement,     // Pops index and array_ptr -> pushes arr[idx]
    JumpIfFalse(usize),
    Halt,
    CallMethod(String, usize),
    Mul,
    Div,
    Require,
    /// Create a closure: pops environment object pointer from stack,
    /// combines it with the function address to create a Function value.
    /// This is the key to "lifting" captured variables from stack to heap.
    MakeClosure(usize), // address of the function body
    /// Construct a new object: pops constructor, args, and `this` object from stack.
    /// Binds `this` to the new object, calls the constructor, returns the object.
    Construct(usize), // arg_count
    /// Store top of stack into indexed local variable slot
    StoreLocal(u32),
    /// Load indexed local variable slot onto stack
    LoadLocal(u32),
}
