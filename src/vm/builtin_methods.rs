//! Built-in methods: calling them on a value (`CallMethod`), and the
//! method values read off one (`const trim = s.trim`), which call the same
//! code with the `this` they are given. Which names are methods of which
//! kind of value is `builtins::Receiver::methods`.

use crate::runtime::builtins::{self, Error, Method, Outcome, Receiver};
use crate::vm::value::{HeapData, HeapObject, JsValue, Promise, Reaction};
use crate::vm::{VM, VmHost};

/// Map and Set key equality (SameValueZero): `===`, except that NaN
/// equals NaN.
fn same_key(a: &JsValue, b: &JsValue) -> bool {
    match (a, b) {
        (JsValue::Number(x), JsValue::Number(y)) => x == y || (x.is_nan() && y.is_nan()),
        _ => a == b,
    }
}

impl VM {
    /// The kind of value `v` is, if it has built-in methods.
    pub(super) fn receiver_of(&self, v: &JsValue) -> Option<Receiver> {
        match v {
            JsValue::String(_) => Some(Receiver::String),
            JsValue::Number(_) => Some(Receiver::Number),
            JsValue::Promise(_) => Some(Receiver::Promise),
            JsValue::Object(ptr) => match self.heap.get(*ptr).map(|o| &o.data) {
                Some(HeapData::Array(_)) => Some(Receiver::Array),
                Some(HeapData::Map(_)) => Some(Receiver::Map),
                Some(HeapData::Set(_)) => Some(Receiver::Set),
                _ => None,
            },
            _ => None,
        }
    }

    /// `v.name` when it is a built-in method of `v`.
    pub(super) fn builtin_method(&self, v: &JsValue, name: &str) -> Option<JsValue> {
        let m = Method::find(self.receiver_of(v)?, name)?;
        Some(JsValue::BuiltinMethod(m))
    }

    /// Call the method value `m` with `this`, to completion. A `this` of
    /// another kind is a TypeError, except that a string method converts
    /// it to a string, as JavaScript does.
    pub(crate) fn call_builtin(
        &mut self,
        m: Method,
        this: JsValue,
        args: Vec<JsValue>,
    ) -> Result<JsValue, JsValue> {
        let nullish = matches!(this, JsValue::Null | JsValue::Undefined);
        if self.receiver_of(&this) != Some(m.receiver()) {
            if m.receiver() == Receiver::String && !nullish {
                let s = match self.to_primitive(&this, true)? {
                    Some(p) => self.to_js_string(&p),
                    None => self.to_js_string(&this),
                };
                return self.call_builtin(m, JsValue::String(s), args);
            }
            let shown = builtins::receiver_shown(&VmHost(&self.heap), &this);
            let e = m.receiver_error(nullish, &shown);
            return Err(self.make_error(e.kind, e.message));
        }
        match self.run_builtin(&this, m.name(), args) {
            Some(result) => result,
            None => unreachable!("{} is listed but not implemented", m.name()),
        }
    }

    /// `this.name(...args)` when `name` is a built-in method of `this`;
    /// `None` when it is not one.
    pub(super) fn run_builtin(
        &mut self,
        this: &JsValue,
        name: &str,
        args: Vec<JsValue>,
    ) -> Option<Result<JsValue, JsValue>> {
        let result = match (self.receiver_of(this)?, this) {
            (Receiver::String, JsValue::String(s)) => {
                builtins::string_method(&VmHost(&self.heap), s, name, &args)
            }
            (Receiver::Number, JsValue::Number(n)) => {
                builtins::number_method(&VmHost(&self.heap), *n, name, &args)
            }
            (Receiver::Array, JsValue::Object(ptr)) => self.array_method(*ptr, name, &args),
            (Receiver::Promise, JsValue::Promise(p)) => {
                let p = self.promise_method(p, name, args)?;
                return Some(Ok(JsValue::Promise(p)));
            }
            (Receiver::Map, JsValue::Object(ptr)) => self.map_method(*ptr, name, &args),
            (Receiver::Set, JsValue::Object(ptr)) => self.set_method(*ptr, name, &args),
            _ => None,
        }?;
        Some(self.outcome(result, this.clone()))
    }

    /// A built-in method's result as a value (`receiver` for `This`), or
    /// the exception it throws.
    pub(super) fn outcome(
        &mut self,
        result: Result<Outcome<JsValue>, Error>,
        receiver: JsValue,
    ) -> Result<JsValue, JsValue> {
        match result {
            Ok(Outcome::Value(v)) => Ok(v),
            Ok(Outcome::This) => Ok(receiver),
            Ok(Outcome::NewArray(elems)) => {
                let ptr = self.heap.len();
                self.heap.push(HeapObject {
                    data: HeapData::Array(elems),
                });
                Ok(JsValue::Object(ptr))
            }
            Err(e) => Err(self.make_error(e.kind, e.message)),
        }
    }

    /// An array method shared with native code (`builtins::array_method`).
    /// It works on a copy of the elements, which is written back.
    pub(super) fn array_method(
        &mut self,
        ptr: usize,
        name: &str,
        args: &[JsValue],
    ) -> Option<Result<Outcome<JsValue>, Error>> {
        let Some(HeapData::Array(arr)) = self.heap.get(ptr).map(|o| &o.data) else {
            return None;
        };
        let mut elems = arr.clone();
        let result = builtins::array_method(&VmHost(&self.heap), &mut elems, name, args)?;
        self.heap[ptr].data = HeapData::Array(elems);
        Some(result)
    }

    /// `p.then`, `p.catch`, `p.finally`: the promise each returns.
    fn promise_method(&mut self, p: &Promise, name: &str, args: Vec<JsValue>) -> Option<Promise> {
        let mut args = args.into_iter();
        let mut arg = || args.next().unwrap_or(JsValue::Undefined);
        Some(match name {
            "then" => {
                let (f, r) = (arg(), arg());
                self.promise_then(p, f, r)
            }
            "catch" => {
                let r = arg();
                self.promise_then(p, JsValue::Undefined, r)
            }
            "finally" => {
                let on_finally = arg();
                let result = Promise::new();
                self.add_reaction(
                    p,
                    Reaction::Finally {
                        on_finally,
                        result: result.clone(),
                    },
                );
                result
            }
            _ => return None,
        })
    }

    /// The methods of the `Map` at `ptr`.
    fn map_method(
        &mut self,
        ptr: usize,
        name: &str,
        args: &[JsValue],
    ) -> Option<Result<Outcome<JsValue>, Error>> {
        let Some(HeapData::Map(map)) = self.heap.get_mut(ptr).map(|o| &mut o.data) else {
            return None;
        };
        let arg = |i: usize| args.get(i).cloned().unwrap_or(JsValue::Undefined);
        let key = arg(0);
        let found = map.iter().position(|(k, _)| same_key(k, &key));
        let value = match name {
            "get" => found.map_or(JsValue::Undefined, |i| map[i].1.clone()),
            "set" => {
                match found {
                    Some(i) => map[i].1 = arg(1),
                    None => map.push((key, arg(1))),
                }
                return Some(Ok(Outcome::This));
            }
            "has" => JsValue::Boolean(found.is_some()),
            "delete" => {
                if let Some(i) = found {
                    map.remove(i);
                }
                JsValue::Boolean(found.is_some())
            }
            "clear" => {
                map.clear();
                JsValue::Undefined
            }
            _ => return None,
        };
        Some(Ok(Outcome::Value(value)))
    }

    /// The methods of the `Set` at `ptr`.
    fn set_method(
        &mut self,
        ptr: usize,
        name: &str,
        args: &[JsValue],
    ) -> Option<Result<Outcome<JsValue>, Error>> {
        let Some(HeapData::Set(set)) = self.heap.get_mut(ptr).map(|o| &mut o.data) else {
            return None;
        };
        let value = args.first().cloned().unwrap_or(JsValue::Undefined);
        let found = set.iter().position(|v| same_key(v, &value));
        let result = match name {
            "add" => {
                if found.is_none() {
                    set.push(value);
                }
                return Some(Ok(Outcome::This));
            }
            "has" => JsValue::Boolean(found.is_some()),
            "delete" => {
                if let Some(i) = found {
                    set.remove(i);
                }
                JsValue::Boolean(found.is_some())
            }
            "clear" => {
                set.clear();
                JsValue::Undefined
            }
            _ => return None,
        };
        Some(Ok(Outcome::Value(result)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every name `Receiver::methods` lists is one the backends implement:
    /// a listed name that is not would read as a function and then fail
    /// when called.
    #[test]
    fn listed_methods_are_implemented() {
        let mut vm = VM::new_bare();
        let array = vm.heap.len();
        vm.heap.push(HeapObject {
            data: HeapData::Array(Vec::new()),
        });
        let map = vm.heap.len();
        vm.heap.push(HeapObject {
            data: HeapData::Map(Vec::new()),
        });
        let set = vm.heap.len();
        vm.heap.push(HeapObject {
            data: HeapData::Set(Vec::new()),
        });
        let receivers = [
            JsValue::String("abc".to_string()),
            JsValue::Object(array),
            JsValue::Number(1.5),
            JsValue::Promise(Promise::new()),
            JsValue::Object(map),
            JsValue::Object(set),
        ];
        for this in receivers {
            let receiver = vm.receiver_of(&this).unwrap();
            for name in receiver.methods() {
                assert!(
                    vm.run_builtin(&this, name, Vec::new()).is_some(),
                    "{}.prototype.{name}",
                    receiver.name()
                );
                let m = Method::find(receiver, name).unwrap();
                assert_eq!(Method::from_bits(m.to_bits()), Some(m));
            }
        }
    }
}
