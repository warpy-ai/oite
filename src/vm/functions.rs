//! Functions are objects: the properties a program gives a function, its
//! `name`, `length` and `prototype`, `call`, `apply` and `bind`, and the
//! bound functions `bind` makes.
//!
//! A function value is its code and environment (`JsValue::Function`), not
//! a heap object, so a function's own properties are kept in an object of
//! their own (`VM::fn_props`), made when the first one is set. They are
//! found by what `===` compares, so every copy of a function sees them.

use crate::runtime::builtins::{self, Method, Receiver};
use crate::vm::VM;
use crate::vm::opcodes::function_info;
use crate::vm::value::{BoundFunction, HeapData, HeapObject, JsValue, PropMap};
use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};

/// Each function's own properties (`VM::fn_props`). It is looked up on
/// every property read and method call on a function, `f.call(...)`
/// included, so it hashes with a multiply per word instead of SipHash: its
/// keys are the program's own addresses and indices, which no input
/// chooses.
pub type FunctionProps = HashMap<FunctionId, usize, BuildHasherDefault<IdHasher>>;

#[derive(Default)]
pub struct IdHasher(u64);

impl Hasher for IdHasher {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.write_u64(b as u64);
        }
    }

    fn write_u64(&mut self, n: u64) {
        self.0 = (self.0.rotate_left(5) ^ n).wrapping_mul(0x517c_c1b7_2722_0a95);
    }

    fn write_usize(&mut self, n: usize) {
        self.write_u64(n as u64);
    }
}

/// Which function a function value is: what `===` compares.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FunctionId {
    Code { address: usize, env: Option<usize> },
    Native(usize),
    Builtin(Method),
    Bound(usize),
}

impl FunctionId {
    /// The function `v` is, if it is one.
    pub fn of(v: &JsValue) -> Option<FunctionId> {
        Some(match v {
            JsValue::Function { address, env } => FunctionId::Code {
                address: *address,
                env: *env,
            },
            JsValue::NativeFunction(i) => FunctionId::Native(*i),
            JsValue::BuiltinMethod(m) => FunctionId::Builtin(*m),
            JsValue::BoundFunction(p) => FunctionId::Bound(*p),
            _ => return None,
        })
    }
}

impl VM {
    /// A new heap object holding `data`.
    pub(super) fn alloc(&mut self, data: HeapData) -> usize {
        let ptr = self.heap.len();
        self.heap.push(HeapObject { data });
        ptr
    }

    /// The own properties of the function `id`, if it has any.
    fn fn_props(&self, id: FunctionId) -> Option<&PropMap> {
        match &self.heap.get(*self.fn_props.get(&id)?)?.data {
            HeapData::Object(props) => Some(props),
            _ => None,
        }
    }

    fn fn_props_mut(&mut self, id: FunctionId) -> &mut PropMap {
        let ptr = match self.fn_props.get(&id) {
            Some(ptr) => *ptr,
            None => {
                let ptr = self.alloc(HeapData::Object(PropMap::new()));
                self.fn_props.insert(id, ptr);
                ptr
            }
        };
        match &mut self.heap[ptr].data {
            HeapData::Object(props) => props,
            _ => unreachable!("a function's properties are an object"),
        }
    }

    /// The bound function at `ptr`.
    fn bound(&self, ptr: usize) -> &BoundFunction {
        match &self.heap[ptr].data {
            HeapData::Bound(b) => b,
            _ => unreachable!("a bound function is `HeapData::Bound`"),
        }
    }

    /// `f.name`.
    pub(crate) fn function_name(&self, f: &JsValue) -> String {
        match f {
            JsValue::Function { address, .. } => function_info(&self.program, *address)
                .map(|info| info.name.clone())
                .unwrap_or_default(),
            JsValue::BuiltinMethod(m) => m.name().to_string(),
            JsValue::BoundFunction(ptr) => {
                format!("bound {}", self.function_name(&self.bound(*ptr).target))
            }
            _ => String::new(),
        }
    }

    /// `f.length`.
    fn function_length(&self, f: &JsValue) -> f64 {
        match f {
            JsValue::Function { address, .. } => {
                function_info(&self.program, *address).map_or(0, |info| info.length) as f64
            }
            JsValue::BoundFunction(ptr) => {
                let b = self.bound(*ptr);
                let target = self.function_length(&b.target);
                builtins::bound_length(target, b.args.len())
            }
            _ => 0.0,
        }
    }

    /// Whether `f` has a `prototype`: a `function` has one, and so does any
    /// function a program gave one.
    fn has_prototype(&self, f: &JsValue) -> bool {
        let declared = matches!(f, JsValue::Function { address, .. }
            if function_info(&self.program, *address).is_some_and(|info| info.prototype));
        declared
            || FunctionId::of(f)
                .and_then(|id| self.fn_props(id))
                .is_some_and(|props| props.contains_key("prototype"))
    }

    /// `f.prototype`, made the first time it is read (`{ constructor: f }`);
    /// `undefined` for a function without one.
    pub(super) fn function_prototype(&mut self, f: &JsValue) -> JsValue {
        let Some(id) = FunctionId::of(f) else {
            return JsValue::Undefined;
        };
        if let Some(proto) = self.fn_props(id).and_then(|p| p.get("prototype")) {
            return proto.clone();
        }
        if !self.has_prototype(f) {
            return JsValue::Undefined;
        }
        let mut props = PropMap::new();
        props.insert("constructor".to_string(), f.clone());
        let proto = self.alloc(HeapData::Object(props));
        self.fn_props_mut(id)
            .insert("prototype".to_string(), JsValue::Object(proto));
        JsValue::Object(proto)
    }

    /// `f[name]`: its own property, then `name`, `length`, `prototype`, and
    /// the methods of `Function.prototype`.
    pub(super) fn get_function_property(&mut self, f: &JsValue, name: &str) -> JsValue {
        if let Some(v) = FunctionId::of(f)
            .and_then(|id| self.fn_props(id))
            .and_then(|props| props.get(name))
        {
            return v.clone();
        }
        match name {
            "name" => JsValue::String(self.function_name(f)),
            "length" => JsValue::Number(self.function_length(f)),
            "prototype" => self.function_prototype(f),
            _ => Method::find(Receiver::Function, name)
                .map_or(JsValue::Undefined, JsValue::BuiltinMethod),
        }
    }

    /// `f[name] = value`. `name` and `length` cannot be written (outside
    /// strict mode the write is ignored).
    pub(crate) fn set_function_property(&mut self, f: &JsValue, name: &str, value: JsValue) {
        if matches!(name, "name" | "length") {
            return;
        }
        if let Some(id) = FunctionId::of(f) {
            self.fn_props_mut(id).insert(name.to_string(), value);
        }
    }

    /// `delete f[name]`.
    pub(super) fn delete_function_property(&mut self, f: &JsValue, name: &str) {
        if let Some(id) = FunctionId::of(f)
            && self.fn_props.contains_key(&id)
        {
            self.fn_props_mut(id).shift_remove(name);
        }
    }

    /// `name in f`.
    pub(super) fn function_has(&self, f: &JsValue, name: &str) -> bool {
        let own = FunctionId::of(f)
            .and_then(|id| self.fn_props(id))
            .is_some_and(|props| props.contains_key(name));
        own || matches!(name, "name" | "length")
            || (name == "prototype" && self.has_prototype(f))
            || Method::find(Receiver::Function, name).is_some()
    }

    /// The properties `Object.keys`, `for...in`, spread and `console.log`
    /// list: the ones a program set (not `prototype`).
    pub(crate) fn function_entries(&self, f: &JsValue) -> Vec<(String, JsValue)> {
        FunctionId::of(f)
            .and_then(|id| self.fn_props(id))
            .map(|props| {
                props
                    .iter()
                    .filter(|(k, _)| !builtins::is_hidden_key(k) && !builtins::is_function_key(k))
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// `target.bind(this, ...args)`.
    fn bind(&mut self, target: JsValue, mut args: Vec<JsValue>) -> JsValue {
        let this = if args.is_empty() {
            JsValue::Undefined
        } else {
            args.remove(0)
        };
        let ptr = self.alloc(HeapData::Bound(Box::new(BoundFunction {
            target,
            this,
            args,
        })));
        JsValue::BoundFunction(ptr)
    }

    /// What calling `f` with `this` and `args` calls: through any number of
    /// binds, the target, with the bound `this` and the bound arguments
    /// first.
    pub(super) fn unbind(
        &self,
        mut f: JsValue,
        mut this: JsValue,
        mut args: Vec<JsValue>,
    ) -> (JsValue, JsValue, Vec<JsValue>) {
        while let JsValue::BoundFunction(ptr) = f {
            let b = self.bound(ptr);
            args.splice(0..0, b.args.iter().cloned());
            this = b.this.clone();
            f = b.target.clone();
        }
        (f, this, args)
    }

    /// `f.call(this, ...args)` / `f.apply(this, array)`: the `this` and the
    /// arguments the call gives `f`.
    pub(super) fn call_apply_args(
        &self,
        method: &str,
        mut args: Vec<JsValue>,
    ) -> (JsValue, Vec<JsValue>) {
        let this = if args.is_empty() {
            JsValue::Undefined
        } else {
            args.remove(0)
        };
        if method == "apply" {
            args = match args.first() {
                Some(JsValue::Object(ptr)) => match self.heap.get(*ptr) {
                    Some(HeapObject {
                        data: HeapData::Array(items),
                    }) => items.clone(),
                    _ => Vec::new(),
                },
                _ => Vec::new(),
            };
        }
        (this, args)
    }

    /// `f.call(...)`, `f.apply(...)` or `f.bind(...)`, to completion.
    pub(super) fn function_method(
        &mut self,
        f: &JsValue,
        name: &str,
        args: Vec<JsValue>,
    ) -> Result<JsValue, JsValue> {
        if name == "bind" {
            return Ok(self.bind(f.clone(), args));
        }
        let (this, args) = self.call_apply_args(name, args);
        self.call_sync(f, this, args)
    }
}
