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
    /// Store into object with computed key: pops [obj, value, key] -> sets obj[key] = value
    SetPropComputed,
    /// Get from object with computed key: pops [obj, key] -> pushes obj[key]
    GetPropComputed,
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
    Or,     // ||
    Not,    // ! (logical not)
    Neg,    // - (unary negation)
    TypeOf, // typeof operator - returns type string
    /// `typeof name` for a plain variable: like `Load(name)` then `TypeOf`,
    /// except that an unbound name gives "undefined" instead of throwing.
    TypeOfName(String),
    /// `String(v)`, as in a template literal: an object's own `toString`
    /// (or `valueOf`) converts it. [v] -> [string]
    ToStr,
    Delete(String),  // delete operator - removes property from object
    NewArray(usize), // Creates array of size N
    StoreElement,    // Pops index, value, and array_ptr -> arr[idx] = val
    LoadElement,     // Pops index and array_ptr -> pushes arr[idx]
    /// ArrayPush: pops [array, value] -> pushes value to array, pushes array back
    ArrayPush,
    /// ArraySpread: pops [target_array, source_array] -> appends all source elements to target, pushes target
    ArraySpread,
    /// ObjectSpread: pops [target_obj, source_obj] -> copies all properties from source to target, pushes target
    ObjectSpread,
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
    /// Push the environment object of the running closure (the one
    /// `MakeClosure` captured), or `undefined`. Compiled functions read
    /// captured variables through it.
    LoadClosureEnv,
    /// Construct a new object: pops constructor, args, and `this` object from stack.
    /// Binds `this` to the new object, calls the constructor, returns the object.
    Construct(usize), // arg_count
    /// Store top of stack into indexed local variable slot
    StoreLocal(u32),
    /// Load indexed local variable slot onto stack
    LoadLocal(u32),

    // === Bitwise operators ===
    /// Bitwise AND (&)
    BitAnd,
    /// Bitwise OR (|)
    BitOr,
    /// Bitwise XOR (^)
    Xor,
    /// Left shift (<<)
    ShiftLeft,
    /// Right shift (>>) - arithmetic
    ShiftRight,
    /// Unsigned right shift (>>>) - logical
    ShiftRightUnsigned,
    /// Exponentiation (**)
    Pow,

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
    /// `super(args)`: run the parent class's constructor on the current
    /// `this`. [args..., parent] -> [this]
    CallSuper(usize),
    /// First instruction of a derived class's implicit constructor: its
    /// caller passes every argument, then their count. A no-op itself.
    ForwardArgs,
    /// The implicit constructor's `super(...args)`.
    /// [args..., count, parent] -> [this]
    SuperForward,
    /// `f.call(this, args)`: call with an explicit `this` (`super.m(args)`).
    /// [args..., f, this] -> [result]
    CallWithThis(usize),
    /// `obj.#x`: like `GetProp`, but an object without the private member
    /// throws a TypeError. [obj] -> [value]
    GetPrivate(String),
    /// `obj.#x = value`: the object must have the private field.
    /// [obj, value] -> []
    SetPrivate(String),
    /// `super.name`: `name` looked up from `base` (the parent's prototype,
    /// or the parent in a static method); a getter runs with `this`.
    /// [this, base] -> [value]
    GetSuperProp(String),

    // === instanceof ===
    /// InstanceOf: pops constructor and object, checks if constructor.prototype is in object's prototype chain
    InstanceOf,

    // === new.target ===
    /// NewTarget: pushes the constructor that was called with new (stored in frame)
    /// This implements the ES6 new.target meta-property
    NewTarget,

    // === ES Modules ===
    /// ImportAsync: Asynchronously load a module
    /// Stack: [module_url] -> [promise]
    /// The promise resolves to the module namespace object
    ImportAsync(String),
    /// Await: Await a promise value (must be in async context)
    /// Stack: [promise] -> [result]
    /// Suspends execution until promise resolves
    Await,
    /// GetExport: Get named export from module namespace
    /// Stack: [namespace] -> [export_value]
    GetExport {
        name: String,
        is_default: bool,
    },
    /// ModuleResolutionError: Error with source location and dependency chain
    ModuleResolutionError {
        message: String,
        specifier: String,
        importer: String,
        dependency_chain: Vec<String>,
    },
}
