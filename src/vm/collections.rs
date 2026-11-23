//! `Map`, `Set`, their iterators, and iterating them: `new Map(items)`,
//! spread and `for...of`. The tables and their methods are shared with
//! native code (`runtime::collection`).

use crate::runtime::builtins::{Outcome, Receiver};
use crate::runtime::collection::{self, Cursor, Iter, Table, Yields};
use crate::vm::value::{HeapData, JsValue};
use crate::vm::{VM, VmHost};

impl VM {
    fn table_mut(&mut self, ptr: usize) -> &mut Table<JsValue> {
        match &mut self.heap[ptr].data {
            HeapData::Map(table) | HeapData::Set(table) => table,
            _ => unreachable!("not a Map or a Set"),
        }
    }

    /// `this.name(...args)` for the `Map` or `Set` (`collection`) at `ptr`.
    pub(super) fn collection_method(
        &mut self,
        ptr: usize,
        collection: Receiver,
        name: &str,
        args: Vec<JsValue>,
    ) -> Option<Result<JsValue, JsValue>> {
        if let Some(yields) = Yields::of_method(name) {
            let it = Iter::new(JsValue::Object(ptr), yields);
            return Some(Ok(JsValue::Object(self.alloc(HeapData::Iterator(it)))));
        }
        if name == "forEach" {
            return Some(self.for_each(ptr, args));
        }
        // The table, out of the heap while the shared code changes it.
        let mut table = std::mem::take(self.table_mut(ptr));
        let result = collection::method(&VmHost(&self.heap), &mut table, collection, name, &args);
        *self.table_mut(ptr) = table;
        Some(self.outcome(result?, JsValue::Object(ptr)))
    }

    /// `forEach(callback, thisArg)`: `callback(value, key, collection)`
    /// for each entry, including ones it adds.
    fn for_each(&mut self, ptr: usize, args: Vec<JsValue>) -> Result<JsValue, JsValue> {
        let mut args = args.into_iter();
        let callback = args.next().unwrap_or(JsValue::Undefined);
        let this = args.next().unwrap_or(JsValue::Undefined);
        if !self.is_callable(&callback) {
            let shown = crate::stdlib::inspect(self, &callback);
            return Err(self.make_error("TypeError", format!("{shown} is not a function")));
        }
        let mut cursor = Cursor::START;
        while let Some((key, value)) = self.table_mut(ptr).advance(&mut cursor) {
            let call_args = vec![value, key, JsValue::Object(ptr)];
            if let Err(e) = self.call_sync(&callback, this.clone(), call_args) {
                self.table_mut(ptr).release(cursor);
                return Err(e);
            }
        }
        Ok(JsValue::Undefined)
    }

    /// The iterator at `ptr` gives its next item; `None` when it is done.
    fn step(&mut self, ptr: usize) -> Option<JsValue> {
        let HeapData::Iterator(it) = &self.heap[ptr].data else {
            return None;
        };
        let (source, mut cursor, yields) = (it.source.clone(), it.cursor, it.yields);
        let JsValue::Object(table) = source else {
            return None;
        };
        let entry = self.table_mut(table).advance(&mut cursor);
        if let HeapData::Iterator(it) = &mut self.heap[ptr].data {
            it.cursor = cursor;
        }
        let (key, value) = entry?;
        let item = match yields.item(key, value) {
            Outcome::NewArray(pair) => self.alloc_array(pair),
            Outcome::Value(v) => v,
            Outcome::This => unreachable!("an iterator gives no `this`"),
        };
        Some(item)
    }

    /// `it.next()`: `{ value, done }`.
    pub(super) fn iterator_next(&mut self, ptr: usize) -> JsValue {
        let (value, done) = match self.step(ptr) {
            Some(v) => (v, false),
            None => (JsValue::Undefined, true),
        };
        let mut props = crate::vm::value::PropMap::new();
        props.insert("value".to_string(), value);
        props.insert("done".to_string(), JsValue::Boolean(done));
        JsValue::Object(self.alloc(HeapData::Object(props)))
    }

    /// The `Map` or `Set` an iterator walks, and whether it is a `Map`.
    pub(crate) fn iterator_source(
        &self,
        it: &Iter<JsValue>,
    ) -> Option<(&Table<JsValue>, Receiver)> {
        let JsValue::Object(ptr) = it.source else {
            return None;
        };
        match &self.heap.get(ptr)?.data {
            HeapData::Map(table) => Some((table, Receiver::Map)),
            HeapData::Set(table) => Some((table, Receiver::Set)),
            _ => None,
        }
    }

    /// Which collection `ctor` makes, if it is the global `Map` or `Set`.
    pub(super) fn collection_constructor(&self, ctor: &JsValue) -> Option<Receiver> {
        let JsValue::Object(ptr) = ctor else {
            return None;
        };
        let HeapData::Object(props) = &self.heap.get(*ptr)?.data else {
            return None;
        };
        match props.get("__type__") {
            Some(JsValue::String(t)) if t == "Map" => Some(Receiver::Map),
            Some(JsValue::String(t)) if t == "Set" => Some(Receiver::Set),
            _ => None,
        }
    }

    /// `new Map(items)` or `new Set(items)`.
    pub(super) fn construct_collection(
        &mut self,
        collection: Receiver,
        items: JsValue,
    ) -> Result<JsValue, JsValue> {
        let items = match items {
            JsValue::Undefined | JsValue::Null => Vec::new(),
            items => self.iterate(&items)?,
        };
        let mut table = Table::default();
        if let Err(e) = collection::fill(&VmHost(&self.heap), &mut table, collection, items) {
            return Err(self.make_error(e.kind, e.message));
        }
        let data = if collection == Receiver::Map {
            HeapData::Map(table)
        } else {
            HeapData::Set(table)
        };
        Ok(JsValue::Object(self.alloc(data)))
    }

    /// What `[...v]` gives: an array's elements, a string's characters, a
    /// `Map`'s entries, a `Set`'s values, or what an iterator has left.
    /// A TypeError for a value that is not iterable.
    pub(super) fn iterate(&mut self, v: &JsValue) -> Result<Vec<JsValue>, JsValue> {
        let ptr = match v {
            JsValue::String(s) => {
                return Ok(s.chars().map(|c| JsValue::String(c.into())).collect());
            }
            JsValue::Object(ptr) => *ptr,
            _ => usize::MAX,
        };
        let yields = match self.heap.get(ptr).map(|o| &o.data) {
            Some(HeapData::Array(items)) => return Ok(items.clone()),
            Some(HeapData::Map(_)) => Some(Yields::Entries),
            Some(HeapData::Set(_)) => Some(Yields::Values),
            Some(HeapData::Iterator(it)) if !it.for_of => None,
            _ => {
                let shown = crate::stdlib::inspect(self, v);
                return Err(self.make_error("TypeError", format!("{shown} is not iterable")));
            }
        };
        let iterator = match yields {
            Some(yields) => self.alloc(HeapData::Iterator(Iter::new(v.clone(), yields))),
            None => ptr,
        };
        Ok(std::iter::from_fn(|| self.step(iterator)).collect())
    }

    /// `GetIterator`: what a `for...of` loop walks `v` with. An array or a
    /// string is walked as it is; a `Map`, a `Set` or an iterator through
    /// a cursor (`collection::Iter`). Any other value is left as it is.
    pub(super) fn for_of_cursor(&mut self, v: JsValue) -> JsValue {
        let JsValue::Object(ptr) = v else {
            return v;
        };
        let yields = match self.heap.get(ptr).map(|o| &o.data) {
            Some(HeapData::Map(_)) => Some(Yields::Entries),
            Some(HeapData::Set(_)) => Some(Yields::Values),
            Some(HeapData::Iterator(it)) if !it.for_of => None,
            _ => return v,
        };
        let iterator = match yields {
            Some(yields) => JsValue::Object(self.alloc(HeapData::Iterator(Iter::new(v, yields)))),
            None => v,
        };
        JsValue::Object(self.alloc(HeapData::Iterator(Iter::for_of(iterator))))
    }

    /// A `for...of` cursor's `length` (`collection::Iter`).
    pub(super) fn cursor_length(&self, it: &Iter<JsValue>) -> f64 {
        let more = match &it.source {
            JsValue::Object(ptr) => match self.heap.get(*ptr).map(|o| &o.data) {
                Some(HeapData::Iterator(inner)) => self
                    .iterator_source(inner)
                    .is_some_and(|(table, _)| table.peek(inner.cursor).is_some()),
                _ => false,
            },
            _ => false,
        };
        collection::cursor_length(more)
    }

    /// Reading an element of the `for...of` cursor at `ptr`: its next item.
    /// `None` if `ptr` is not a cursor.
    pub(super) fn cursor_take(&mut self, ptr: usize) -> Option<JsValue> {
        let Some(HeapData::Iterator(Iter {
            source: JsValue::Object(inner),
            for_of: true,
            ..
        })) = self.heap.get(ptr).map(|o| &o.data)
        else {
            return None;
        };
        let inner = *inner;
        Some(self.step(inner).unwrap_or(JsValue::Undefined))
    }
}
