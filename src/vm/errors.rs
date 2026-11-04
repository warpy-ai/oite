//! Runtime errors.
//!
//! The error classes (`Error`, `TypeError`, ...) are written in Oite, in
//! `std/core/error.ot`, and run before every program. The VM creates
//! instances of them for its own runtime errors, so user code catches,
//! inspects and subclasses them like any other error. An exception that
//! no handler catches is reported on stderr as `Uncaught TypeError: ...`
//! and the process exits with status 1, the same report native code gives.
//! With `OITE_DEBUG_THROW=1` the report also lists the bytecode around the
//! throw.

use std::collections::HashMap;

use super::value::{HeapData, HeapObject, JsValue};
use super::{ExecResult, VM};

/// Source of the built-in error classes.
pub const CORE_ERRORS: &str = include_str!("../../std/core/error.ot");

impl VM {
    fn global(&self, name: &str) -> Option<&JsValue> {
        self.call_stack.first()?.locals.get(name)
    }

    /// The `prototype` object of the global class `class`.
    fn class_prototype(&self, class: &str) -> Option<usize> {
        let JsValue::Object(ptr) = self.global(class)? else {
            return None;
        };
        match &self.heap.get(*ptr)?.data {
            HeapData::Object(props) => match props.get("prototype") {
                Some(JsValue::Object(proto)) => Some(*proto),
                _ => None,
            },
            _ => None,
        }
    }

    /// A new instance of the built-in error class `kind` ("TypeError", ...).
    /// Without the core classes loaded, the error is a `"Kind: message"`
    /// string.
    pub fn make_error(&mut self, kind: &str, message: String) -> JsValue {
        let Some(proto) = self.class_prototype(kind) else {
            return JsValue::String(format!("{kind}: {message}"));
        };
        let mut props = HashMap::new();
        props.insert("message".to_string(), JsValue::String(message));
        props.insert("__proto__".to_string(), JsValue::Object(proto));
        let ptr = self.heap.len();
        self.heap.push(HeapObject {
            data: HeapData::Object(props),
        });
        JsValue::Object(ptr)
    }

    /// Throw a new `kind` error. Like `throw_exception`, the result must be
    /// returned from `exec_one` as-is.
    pub(super) fn throw_error(&mut self, kind: &str, message: impl Into<String>) -> ExecResult {
        let error = self.make_error(kind, message.into());
        self.throw_exception(error)
    }

    /// Whether the object at `ptr` inherits from `Error.prototype`.
    pub fn is_error(&self, ptr: usize) -> bool {
        let Some(error_proto) = self.class_prototype("Error") else {
            return false;
        };
        let mut current = ptr;
        // Bounded, in case of a prototype cycle.
        for _ in 0..1000 {
            let Some(HeapObject {
                data: HeapData::Object(props),
            }) = self.heap.get(current)
            else {
                return false;
            };
            match props.get("__proto__") {
                Some(JsValue::Object(proto)) if *proto == error_proto => return true,
                Some(JsValue::Object(proto)) => current = *proto,
                _ => return false,
            }
        }
        false
    }

    /// Whether `key` is on the object at `ptr` or its prototype chain.
    pub fn has_prop_in_chain(&self, ptr: usize, key: &str) -> bool {
        let mut current = ptr;
        for _ in 0..1000 {
            let Some(HeapObject {
                data: HeapData::Object(props),
            }) = self.heap.get(current)
            else {
                return false;
            };
            if props.contains_key(key) {
                return true;
            }
            match props.get("__proto__") {
                Some(JsValue::Object(p)) => current = *p,
                _ => return false,
            }
        }
        false
    }

    /// Whether the object at `ptr` is a class (`class C {}` evaluates to an
    /// object marked `__class__`).
    pub fn is_class(&self, ptr: usize) -> bool {
        matches!(
            self.heap.get(ptr),
            Some(HeapObject { data: HeapData::Object(props) })
                if matches!(props.get("__class__"), Some(JsValue::Boolean(true)))
        )
    }

    /// `[class C]`, or `[class C extends P]`, as Node prints a class.
    pub fn class_summary(&self, ptr: usize) -> String {
        let name = match self.get_prop_with_proto_chain(ptr, "name") {
            JsValue::String(s) => s,
            _ => "(anonymous)".to_string(),
        };
        let parent = match self.heap.get(ptr) {
            Some(HeapObject {
                data: HeapData::Object(props),
            }) => match props.get("__proto__") {
                Some(JsValue::Object(p)) if self.is_class(*p) => {
                    match self.get_prop_with_proto_chain(*p, "name") {
                        JsValue::String(s) => Some(s),
                        _ => None,
                    }
                }
                _ => None,
            },
            _ => None,
        };
        match parent {
            Some(p) => format!("[class {name} extends {p}]"),
            None => format!("[class {name}]"),
        }
    }

    /// The `typeof` of a value: classes are functions.
    pub fn typeof_value(&self, v: &JsValue) -> &'static str {
        match v {
            JsValue::Object(ptr) if self.is_class(*ptr) => "function",
            other => super::js_typeof(other),
        }
    }

    /// `"TypeError: message"`, as `Error.prototype.toString` gives it.
    pub fn error_summary(&self, ptr: usize) -> String {
        let name = match self.get_prop_with_proto_chain(ptr, "name") {
            JsValue::Undefined => "Error".to_string(),
            v => self.to_js_string(&v),
        };
        let message = match self.get_prop_with_proto_chain(ptr, "message") {
            JsValue::Undefined => String::new(),
            v => self.to_js_string(&v),
        };
        match (name.is_empty(), message.is_empty()) {
            (_, true) => name,
            (true, false) => message,
            (false, false) => format!("{name}: {message}"),
        }
    }

    /// JavaScript `ToString`, without calling user code: arrays join their
    /// elements, errors give their summary, other objects
    /// `[object Object]`.
    pub fn to_js_string(&self, v: &JsValue) -> String {
        self.to_js_string_depth(v, 0)
    }

    fn to_js_string_depth(&self, v: &JsValue, depth: usize) -> String {
        match v {
            JsValue::String(s) => s.clone(),
            JsValue::Number(n) => crate::stdlib::number_to_string(n),
            JsValue::Boolean(b) => b.to_string(),
            JsValue::Null => "null".to_string(),
            JsValue::Undefined => "undefined".to_string(),
            JsValue::Function { .. } | JsValue::NativeFunction(_) => "function".to_string(),
            JsValue::Object(ptr) => match self.heap.get(*ptr).map(|o| &o.data) {
                Some(HeapData::Array(items)) if depth < 100 => items
                    .iter()
                    .map(|e| match e {
                        JsValue::Null | JsValue::Undefined => String::new(),
                        e => self.to_js_string_depth(e, depth + 1),
                    })
                    .collect::<Vec<_>>()
                    .join(","),
                Some(HeapData::Object(_)) if self.is_error(*ptr) => self.error_summary(*ptr),
                _ => "[object Object]".to_string(),
            },
            _ => "[object Object]".to_string(),
        }
    }

    /// Report an exception that no handler caught. The `oitec` binary
    /// prints it and exits with status 1; embedded VMs (tests) panic.
    pub(super) fn report_uncaught(&self, exception: &JsValue) -> ! {
        let text = match exception {
            JsValue::String(s) => s.clone(),
            other => crate::stdlib::inspect(self, other),
        };
        // Until bytecode carries source positions (#125), the instructions
        // before the throw are the best pointer to where it happened.
        if std::env::var("OITE_DEBUG_THROW").is_ok() {
            let start = self.ip.saturating_sub(25);
            for i in start..(self.ip + 2).min(self.program.len()) {
                eprintln!(
                    "{} {:5} {:?}",
                    if i == self.ip { ">>" } else { "  " },
                    i,
                    self.program[i]
                );
            }
        }
        if self.exit_on_uncaught {
            use std::io::Write;
            let _ = std::io::stdout().flush();
            eprintln!("Uncaught {text}");
            std::process::exit(1);
        }
        panic!("Uncaught exception: {text}");
    }
}

/// JavaScript `ToNumber`, for primitives (objects are `NaN`).
pub fn to_number(v: &JsValue) -> f64 {
    match v {
        JsValue::Number(n) => *n,
        JsValue::Boolean(b) => {
            if *b {
                1.0
            } else {
                0.0
            }
        }
        JsValue::Null => 0.0,
        JsValue::String(s) => {
            let t = s.trim();
            if t.is_empty() {
                0.0
            } else {
                t.parse::<f64>().unwrap_or(f64::NAN)
            }
        }
        _ => f64::NAN,
    }
}

/// `#x` from a private member's key (`#x@<class>`), for messages.
pub fn private_name(key: &str) -> &str {
    key.split('@').next().unwrap_or(key)
}
