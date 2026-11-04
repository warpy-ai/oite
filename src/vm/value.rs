// Memory representation. We will use a enum to implement ownership,
// and we track wheter a value is "Owned" or a "reference" in the low-level representation
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

pub type NativeFn = fn(&mut crate::vm::VM, Vec<JsValue>) -> JsValue;

#[derive(Debug, Clone)]
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
    /// Promise for ES modules and async operations
    Promise(Promise),
}

impl PartialEq for JsValue {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (JsValue::Number(a), JsValue::Number(b)) => a == b,
            (JsValue::String(a), JsValue::String(b)) => a == b,
            (JsValue::Boolean(a), JsValue::Boolean(b)) => a == b,
            (JsValue::Object(a), JsValue::Object(b)) => a == b,
            (
                JsValue::Function {
                    address: a,
                    env: ae,
                },
                JsValue::Function {
                    address: b,
                    env: be,
                },
            ) => a == b && ae == be,
            (JsValue::NativeFunction(a), JsValue::NativeFunction(b)) => a == b,
            (JsValue::Null, JsValue::Null) => true,
            (JsValue::Undefined, JsValue::Undefined) => true,
            (JsValue::Accessor(a_get, a_set), JsValue::Accessor(b_get, b_set)) => {
                a_get == b_get && a_set == b_set
            }
            (JsValue::Promise(a), JsValue::Promise(b)) => a == b,
            _ => false,
        }
    }
}

impl Eq for JsValue {}

#[derive(Debug, Clone, PartialEq)]
pub enum PromiseState {
    Pending,
    Fulfilled,
    Rejected,
}

#[derive(Debug, Clone)]
pub struct Promise {
    pub state: Arc<Mutex<PromiseInternal>>,
}

impl PartialEq for Promise {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.state, &other.state)
    }
}

impl Default for Promise {
    fn default() -> Self {
        Self::new()
    }
}

impl Promise {
    pub fn new() -> Self {
        let state = Arc::new(Mutex::new(PromiseInternal {
            state: PromiseState::Pending,
            value: None,
            reactions: Vec::new(),
            handled: false,
        }));
        Self { state }
    }

    /// A promise already fulfilled with `value`.
    pub fn with_value(value: JsValue) -> Self {
        let p = Self::new();
        {
            let mut internal = p.state.lock().unwrap();
            internal.state = PromiseState::Fulfilled;
            internal.value = Some(value);
        }
        p
    }

    pub fn get_state(&self) -> PromiseState {
        let internal = self.state.lock().unwrap();
        internal.state.clone()
    }

    pub fn get_value(&self) -> Option<JsValue> {
        let internal = self.state.lock().unwrap();
        internal.value.clone()
    }
}

/// A promise's state. Settling is done by the VM (`vm::promise`), which
/// queues the reactions as microtasks.
#[derive(Debug, Clone)]
pub struct PromiseInternal {
    pub state: PromiseState,
    pub value: Option<JsValue>,
    /// What runs when the promise settles.
    pub reactions: Vec<Reaction>,
    /// Whether a reaction was ever attached (else a rejection is unhandled).
    pub handled: bool,
}

/// Something waiting for a promise to settle.
#[derive(Debug, Clone)]
pub enum Reaction {
    /// `.then(onFulfilled, onRejected)`; `result` is the promise `then`
    /// returned.
    Then {
        on_fulfilled: JsValue,
        on_rejected: JsValue,
        result: Promise,
    },
    /// `.finally(onFinally)`
    Finally {
        on_finally: JsValue,
        result: Promise,
    },
    /// Settle this promise the same way (a promise resolved with another).
    Adopt(Promise),
    /// Attach `Adopt(target)` to `source` (queued, as JavaScript's
    /// promise-resolve-thenable job is).
    AdoptLater(Promise, Promise),
    /// Resume the suspended async function with this id.
    Resume(usize),
    /// Element `index` of the `Promise.all` with this id.
    All { id: usize, index: usize },
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
    /// Map - ordered key-value pairs with any key type
    Map(Vec<(JsValue, JsValue)>),
    /// Set - ordered unique values
    Set(Vec<JsValue>),
}
