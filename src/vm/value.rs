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
            handlers: Vec::new(),
        }));
        Self { state }
    }

    pub fn with_value(value: JsValue) -> Self {
        let state = Arc::new(Mutex::new(PromiseInternal {
            state: PromiseState::Fulfilled,
            value: Some(value),
            handlers: Vec::new(),
        }));
        Self { state }
    }

    pub fn get_state(&self) -> PromiseState {
        let internal = self.state.lock().unwrap();
        internal.state.clone()
    }

    pub fn get_value(&self) -> Option<JsValue> {
        let internal = self.state.lock().unwrap();
        internal.value.clone()
    }

    pub fn set_value(&self, value: JsValue, is_fulfilled: bool) {
        let mut internal = self.state.lock().unwrap();

        if matches!(internal.state, PromiseState::Pending) {
            internal.state = if is_fulfilled {
                PromiseState::Fulfilled
            } else {
                PromiseState::Rejected
            };
            let value_to_store = value.clone();
            internal.value = Some(value);

            let handlers = internal.handlers.clone();
            internal.handlers.clear();

            drop(internal);

            for handler in handlers {
                if is_fulfilled {
                    if let Some(on_fulfilled) = handler.on_fulfilled
                        && let JsValue::Function { address, env } = *on_fulfilled
                    {
                        let _ = address;
                        let _ = env;
                        let _ = value_to_store;
                    }
                } else if let Some(on_rejected) = handler.on_rejected
                    && let JsValue::Function { address, env } = *on_rejected
                {
                    let _ = address;
                    let _ = env;
                    let _ = value_to_store;
                }
            }
        }
    }

    pub fn then(&self, on_fulfilled: Option<JsValue>) -> Self {
        let mut internal = self.state.lock().unwrap();

        match internal.state {
            PromiseState::Pending => {
                internal.handlers.push(PromiseHandler {
                    on_fulfilled: on_fulfilled.map(Box::new),
                    on_rejected: None,
                    continuation: None,
                });
                self.clone()
            }
            PromiseState::Fulfilled => {
                if let Some(handler) = on_fulfilled {
                    Promise::with_value(handler)
                } else {
                    Promise::with_value(internal.value.clone().unwrap_or(JsValue::Undefined))
                }
            }
            PromiseState::Rejected => {
                Promise::with_value(internal.value.clone().unwrap_or(JsValue::Undefined))
            }
        }
    }

    pub fn catch(&self, on_rejected: Option<JsValue>) -> Self {
        let mut internal = self.state.lock().unwrap();

        match internal.state {
            PromiseState::Pending => {
                internal.handlers.push(PromiseHandler {
                    on_fulfilled: None,
                    on_rejected: on_rejected.map(Box::new),
                    continuation: None,
                });
                self.clone()
            }
            PromiseState::Fulfilled => {
                Promise::with_value(internal.value.clone().unwrap_or(JsValue::Undefined))
            }
            PromiseState::Rejected => {
                if let Some(handler) = on_rejected {
                    Promise::with_value(handler)
                } else {
                    Promise::with_value(internal.value.clone().unwrap_or(JsValue::Undefined))
                }
            }
        }
    }

    /// Register a continuation for async/await
    pub fn then_await(&self, on_fulfilled: Option<JsValue>, continuation: Continuation) -> Self {
        let mut internal = self.state.lock().unwrap();

        match internal.state {
            PromiseState::Pending => {
                internal.handlers.push(PromiseHandler {
                    on_fulfilled: on_fulfilled.map(Box::new),
                    on_rejected: None,
                    continuation: Some(continuation),
                });
                self.clone()
            }
            PromiseState::Fulfilled => {
                // Already fulfilled, create resolved promise with value
                Promise::with_value(internal.value.clone().unwrap_or(JsValue::Undefined))
            }
            PromiseState::Rejected => {
                Promise::with_value(internal.value.clone().unwrap_or(JsValue::Undefined))
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct PromiseInternal {
    pub state: PromiseState,
    pub value: Option<JsValue>,
    pub handlers: Vec<PromiseHandler>,
}

#[derive(Debug, Clone)]
pub struct PromiseHandler {
    pub on_fulfilled: Option<Box<JsValue>>,
    pub on_rejected: Option<Box<JsValue>>,
    /// Continuation for async/await - stores resume IP when awaiting
    /// Uses Arc<Frame> to avoid import cycles
    pub continuation: Option<Continuation>,
}

#[derive(Debug, Clone)]
pub struct Continuation {
    pub resume_ip: usize,
    /// Stores the frame state for resuming async function
    /// We store locals separately to avoid import cycle
    pub locals: HashMap<String, JsValue>,
    pub this_context: JsValue,
}

/// Continuation callback type for async operations
pub type ContinuationCallback = Box<dyn FnOnce(JsValue) + Send>;

/// Context for async/await continuations
#[derive(Debug, Clone)]
pub struct AsyncContext {
    pub resume_ip: usize,
    /// Stores the frame state for resuming async function
    pub locals: HashMap<String, JsValue>,
    pub this_context: JsValue,
    pub promise: Promise,
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
