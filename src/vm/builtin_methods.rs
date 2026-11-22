//! Built-in methods: calling them on a value (`CallMethod`), and the
//! method values read off one (`const trim = s.trim`), which call the same
//! code with the `this` they are given. Which names are methods of which
//! kind of value is `builtins::Receiver::methods`.

use crate::runtime::builtins::{self, Error, Method, Outcome, Receiver};
use crate::vm::value::{HeapData, HeapObject, JsValue, Promise, Reaction};
use crate::vm::{VM, VmHost};

impl VM {
    /// The kind of value `v` is, if it has built-in methods.
    pub(crate) fn receiver_of(&self, v: &JsValue) -> Option<Receiver> {
        builtins::Host::receiver(&VmHost(&self.heap), v)
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
            (r @ (Receiver::Map | Receiver::Set), JsValue::Object(ptr)) => {
                return self.collection_method(*ptr, r, name, args);
            }
            (Receiver::MapIterator | Receiver::SetIterator, JsValue::Object(ptr)) => {
                return (name == "next").then(|| Ok(self.iterator_next(*ptr)));
            }
            (Receiver::Function, f) => return Some(self.function_method(f, name, args)),
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
            data: HeapData::Map(Default::default()),
        });
        let set = vm.heap.len();
        vm.heap.push(HeapObject {
            data: HeapData::Set(Default::default()),
        });
        let iterators = [map, set].map(|c| {
            let it = vm.heap.len();
            vm.heap.push(HeapObject {
                data: HeapData::Iterator(crate::runtime::collection::Iter::new(
                    JsValue::Object(c),
                    crate::runtime::collection::Yields::Keys,
                )),
            });
            JsValue::Object(it)
        });
        let function = vm.register_native(|_, _| JsValue::Undefined);
        let receivers = [
            JsValue::String("abc".to_string()),
            JsValue::Object(array),
            JsValue::Number(1.5),
            JsValue::Promise(Promise::new()),
            JsValue::Object(map),
            JsValue::Object(set),
            iterators[0].clone(),
            iterators[1].clone(),
            JsValue::NativeFunction(function),
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
