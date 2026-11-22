//! Element access: `obj[key]`, `obj[key] = value` and an array's `length`.
//!
//! The rules are JavaScript's. A key is an array index only if it is a
//! non-negative integer below 2^32 - 1, or the canonical string form of
//! one (`"0"`, not `"00"` or `"1.0"`). Every other key is a property name:
//! the key's string form. So `a[-1]` and `a[1.5]` are not elements, and on
//! a plain object `o[7]` and `o["7"]` are the same property.

use crate::runtime::builtins::{char_at, char_count, is_ascii};
use crate::vm::VM;
use crate::vm::value::{HeapData, JsValue};

/// The largest array length, 2^32 - 1. Array indices are below it.
const MAX_ARRAY_LENGTH: f64 = 4_294_967_295.0;

/// `key` as an array index, if it is one.
pub(super) fn array_index(key: &JsValue) -> Option<usize> {
    match key {
        // NaN fails the comparison; -0 is index 0.
        JsValue::Number(n) if *n >= 0.0 && *n < MAX_ARRAY_LENGTH && n.fract() == 0.0 => {
            Some(*n as usize)
        }
        JsValue::String(s) => {
            let i: u32 = s.parse().ok()?;
            // Only the canonical form: "1", not "01" or "+1".
            (i != u32::MAX && i.to_string() == *s).then_some(i as usize)
        }
        _ => None,
    }
}

impl VM {
    /// `target[key]`. `undefined` for a missing element or property, and
    /// for a target that has none (the caller rejects null and undefined).
    pub(super) fn get_element(&self, target: &JsValue, key: &JsValue) -> JsValue {
        if let Some(i) = array_index(key) {
            match target {
                JsValue::Object(ptr) => {
                    if let Some(HeapData::Array(arr)) = self.heap.get(*ptr).map(|o| &o.data) {
                        return arr.get(i).cloned().unwrap_or(JsValue::Undefined);
                    }
                }
                JsValue::String(s) => {
                    return char_at(s, is_ascii(s), i)
                        .map_or(JsValue::Undefined, |c| JsValue::String(c.to_string()));
                }
                _ => {}
            }
        }
        self.get_named(target, &self.to_js_string(key))
    }

    /// `target[name]` for a name that is not an element index: a plain
    /// object's property, found along its prototype chain (`GetProp` calls
    /// getters first), `length` and `size`, and built-in methods (as
    /// values: `"a".trim`).
    pub(super) fn get_named(&self, target: &JsValue, name: &str) -> JsValue {
        let data = match target {
            JsValue::Object(ptr) => match self.heap.get(*ptr) {
                Some(o) => Some(&o.data),
                None => return JsValue::Undefined,
            },
            _ => None,
        };
        let size = |n: usize| JsValue::Number(n as f64);
        match (target, data) {
            (JsValue::Object(ptr), Some(HeapData::Object(_))) => {
                self.get_prop_with_proto_chain(*ptr, name)
            }
            (_, Some(HeapData::Array(arr))) if name == "length" => size(arr.len()),
            (_, Some(HeapData::ByteStream(bytes))) if name == "length" => size(bytes.len()),
            (_, Some(HeapData::Map(table) | HeapData::Set(table))) if name == "size" => {
                size(table.len())
            }
            (_, Some(HeapData::Iterator(it))) if it.for_of && name == "length" => {
                JsValue::Number(self.cursor_length(it))
            }
            (JsValue::String(s), _) if name == "length" => size(char_count(s, is_ascii(s))),
            _ => match self.builtin_method(target, name) {
                Some(method) => method,
                // The methods written in Oite (`std/core/array.ot`).
                None if matches!(data, Some(HeapData::Array(_))) => {
                    self.array_proto_method(name).unwrap_or(JsValue::Undefined)
                }
                None => JsValue::Undefined,
            },
        }
    }

    /// `target[key] = value`. An array grows to hold a new element; a key
    /// that is not an index is stored only on a plain object (arrays keep
    /// no named properties besides `length`). Errors with the exception to
    /// throw.
    pub(super) fn set_element(
        &mut self,
        target: &JsValue,
        key: &JsValue,
        value: JsValue,
    ) -> Result<(), JsValue> {
        let JsValue::Object(ptr) = target else {
            return Ok(());
        };
        let index = array_index(key);
        if let (Some(i), Some(HeapData::Array(arr))) =
            (index, self.heap.get_mut(*ptr).map(|o| &mut o.data))
        {
            if i >= arr.len() {
                arr.resize(i + 1, JsValue::Undefined);
            }
            arr[i] = value;
            return Ok(());
        }
        let name = self.to_js_string(key);
        match self.heap.get_mut(*ptr).map(|o| &mut o.data) {
            Some(HeapData::Array(_)) if name == "length" => self.set_array_length(*ptr, &value),
            Some(HeapData::Object(props)) => {
                props.insert(name, value);
                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// Whether the heap object at `ptr` is an array.
    pub(super) fn is_array(&self, ptr: usize) -> bool {
        matches!(
            self.heap.get(ptr).map(|o| &o.data),
            Some(HeapData::Array(_))
        )
    }

    /// `array.length = value`: drop the elements past the new length, or
    /// pad with holes up to it. A length that is not an integer in
    /// 0..=2^32-1 is a RangeError.
    pub(super) fn set_array_length(&mut self, ptr: usize, value: &JsValue) -> Result<(), JsValue> {
        let n = crate::vm::errors::to_number(value);
        if !((0.0..=MAX_ARRAY_LENGTH).contains(&n) && n.fract() == 0.0) {
            return Err(self.make_error("RangeError", "Invalid array length".to_string()));
        }
        if let Some(HeapData::Array(arr)) = self.heap.get_mut(ptr).map(|o| &mut o.data) {
            arr.resize(n as usize, JsValue::Undefined);
        }
        Ok(())
    }
}
