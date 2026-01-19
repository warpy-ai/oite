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
    NewObjectWithProto, // Creates object with given prototype
    SetProp(String),
    GetProp(String),
    Dup,
    Swap,
    Swap3,
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

    // === Exception handling ===
    /// Throw an exception: pops value from stack and begins unwinding
    Throw,
    /// Setup a try block: catch_addr is where to jump on exception,
    /// finally_addr is where to jump after try/catch completes (0 = no finally)
    /// Also records the current stack depth and call stack depth for unwinding
    SetupTry {
        catch_addr: usize,
        finally_addr: usize,
    },
    /// Remove the current try block from the exception handler stack
    PopTry,
    /// Used internally: jump to finally block after catch completes
    /// The boolean indicates whether we're rethrowing after finally
    EnterFinally(bool),

    // === Class inheritance ===
    /// Set the __proto__ of an object: pops [obj, proto] -> sets obj.__proto__ = proto, pushes obj
    SetProto,
    /// Load the super constructor: reads __super__ from current frame, pushes it
    LoadSuper,
    /// Call super constructor: pops [args...] and calls __super__ with current this context
    CallSuper(usize),
    /// Get property from super's prototype: pops super object, pushes property value
    GetSuperProp(String),

    // === Private fields ===
    /// Get a private field: pops `this` from stack, looks up field in class's private storage,
    /// pushes the field value. The index refers to the class's private field descriptor.
    GetPrivateProp(usize),
    /// Set a private field: pops value and `this` from stack, sets field in class's private storage.
    SetPrivateProp(usize),
}
