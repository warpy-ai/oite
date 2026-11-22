//! `Map`, `Set`, their iterators, and iterating them: `new Map(items)`,
//! spread and `for...of`. The tables and their methods are shared with
//! the VM (`collection`).

use super::collection::{self, Cursor, Iter, Table, Yields};
use super::*;
use builtins::{Outcome, Receiver};

/// The table of the `Map` or `Set` at `ptr`.
fn table(ptr: HeapPtr) -> &'static mut Table<u64> {
    unsafe { &mut *ptr.as_ref::<NativeCollection>().table }
}

/// The `Map` or `Set` behind `v`: its table, and which it is.
fn collection_of(v: u64) -> Option<(&'static mut Table<u64>, Receiver)> {
    match object(OtValue::from_bits(v))? {
        (ptr, ObjectKind::Map) => Some((table(ptr), Receiver::Map)),
        (ptr, ObjectKind::Set) => Some((table(ptr), Receiver::Set)),
        _ => None,
    }
}

/// The iterator state behind `v`, if it is an iterator or a cursor.
pub(super) fn iter_of(v: u64) -> Option<&'static mut Iter<u64>> {
    match object(OtValue::from_bits(v))? {
        (ptr, ObjectKind::Iterator) => Some(&mut unsafe { ptr.as_mut::<NativeIterator>() }.iter),
        _ => None,
    }
}

fn new_iterator(iter: Iter<u64>) -> u64 {
    match heap().alloc_iterator(iter) {
        Some(ptr) => OtValue::pointer(ptr).to_bits(),
        None => fatal_error("out of memory"),
    }
}

/// The kind of `Map` or `Set` iterator `v` is, for `receiver_of`.
pub(super) fn iterator_receiver(v: u64) -> Option<Receiver> {
    let it = iter_of(v)?;
    if it.for_of {
        return None;
    }
    match collection_of(it.source)?.1 {
        Receiver::Map => Some(Receiver::MapIterator),
        _ => Some(Receiver::SetIterator),
    }
}

/// `this.name(...args)` for the `Map` or `Set` `this`.
pub(super) fn collection_method(this: u64, name: &str, a: &[u64]) -> Option<u64> {
    let (table, collection) = collection_of(this)?;
    if let Some(yields) = Yields::of_method(name) {
        return Some(new_iterator(Iter::new(this, yields)));
    }
    if name == "forEach" {
        return Some(for_each(this, a));
    }
    let result = collection::method(&NativeHost, table, collection, name, a)?;
    Some(outcome(result, this))
}

/// `forEach(callback, thisArg)`: `callback(value, key, collection)` for
/// each entry, including ones it adds.
fn for_each(this: u64, a: &[u64]) -> u64 {
    let callback = a.first().copied().unwrap_or_else(undefined);
    let this_arg = a.get(1).copied().unwrap_or_else(undefined);
    if !matches!(
        object(OtValue::from_bits(callback)),
        Some((_, ObjectKind::Function))
    ) {
        let shown = inspect(OtValue::from_bits(callback));
        return throw_error("TypeError", &format!("{shown} is not a function"));
    }
    // The table is looked up again after each call, which may change it.
    let table = || collection_of(this).map(|(table, _)| table);
    let mut cursor = Cursor::START;
    while let Some((key, value)) = table().and_then(|t| t.advance(&mut cursor)) {
        let args = [value, key, this];
        if ot_call(callback, this_arg, args.as_ptr(), args.len()) == EXCEPTION_BITS {
            if let Some(t) = table() {
                t.release(cursor);
            }
            return EXCEPTION_BITS;
        }
    }
    undefined()
}

/// The iterator `it` gives its next item; `None` when it is done.
fn step(it: &mut Iter<u64>) -> Option<u64> {
    let (table, _) = collection_of(it.source)?;
    let (key, value) = table.advance(&mut it.cursor)?;
    Some(match it.yields.item(key, value) {
        Outcome::Value(v) => v,
        pair => outcome(Ok(pair), undefined()),
    })
}

/// `it.next()`: `{ value, done }`.
pub(super) fn iterator_next(it: u64) -> u64 {
    let item = iter_of(it).and_then(step);
    let result = ot_alloc_object();
    let r = OtValue::from_bits(result);
    set_property(r, "value", item.unwrap_or_else(undefined));
    set_property(r, "done", boolean(item.is_none()));
    result
}

/// `Map` and `Set`: `env` says which. `new Map(items)` makes one; called
/// without `new`, `this` is `undefined` and it is a TypeError.
pub extern "C" fn ot_builtin_collection(this: u64, argv: *const u64, argc: usize, env: u64) -> u64 {
    let (kind, collection) = if OtValue::from_bits(env).as_number_unchecked() == 0.0 {
        (ObjectKind::Map, Receiver::Map)
    } else {
        (ObjectKind::Set, Receiver::Set)
    };
    if OtValue::from_bits(this).is_undefined() {
        let e = collection::requires_new(collection);
        return throw_error(e.kind, &e.message);
    }
    let items = unsafe { args(argv, argc) }
        .first()
        .copied()
        .unwrap_or_else(undefined);
    let i = OtValue::from_bits(items);
    let items = if i.is_undefined() || i.is_null() {
        Vec::new()
    } else {
        match iterate(items) {
            Some(items) => items,
            None => return not_iterable(items),
        }
    };
    let Some(ptr) = heap().alloc_collection(kind) else {
        fatal_error("out of memory");
    };
    if let Err(e) = collection::fill(&NativeHost, table(ptr), collection, items) {
        return throw_error(e.kind, &e.message);
    }
    OtValue::pointer(ptr).to_bits()
}

/// The global `Map` (`collection` 0) or `Set` (1).
pub(super) fn collection_constructor(collection: u32) -> u64 {
    ot_make_closure(
        ot_builtin_collection as *const u8,
        number(collection as f64),
    )
}

/// Which collection `ctor` makes, if it is the global `Map` or `Set`.
pub(super) fn collection_constructor_of(ctor: u64) -> Option<Receiver> {
    let (ptr, ObjectKind::Function) = object(OtValue::from_bits(ctor))? else {
        return None;
    };
    let f = unsafe { ptr.as_ref::<NativeFunction>() };
    if f.code != ot_builtin_collection as *const u8 {
        return None;
    }
    Some(if OtValue::from_bits(f.env).as_number_unchecked() == 0.0 {
        Receiver::Map
    } else {
        Receiver::Set
    })
}

pub(super) fn not_iterable(v: u64) -> u64 {
    let shown = inspect(OtValue::from_bits(v));
    throw_error("TypeError", &format!("{shown} is not iterable"))
}

/// What `[...v]` gives: an array's elements, a string's characters, a
/// `Map`'s entries, a `Set`'s values, or what an iterator has left. `None`
/// for a value that is not iterable.
pub(super) fn iterate(v: u64) -> Option<Vec<u64>> {
    use builtins::Host;
    if let Some(s) = as_str(OtValue::from_bits(v)) {
        return Some(
            s.chars()
                .map(|c| new_string(c.encode_utf8(&mut [0; 4])))
                .collect(),
        );
    }
    if let Some(elems) = NativeHost.array_elements(&v) {
        return Some(elems);
    }
    let mut it = match (collection_of(v), iter_of(v)) {
        (Some((_, collection)), _) => Iter::new(v, collection::default_yields(collection)),
        (_, Some(it)) if !it.for_of => return Some(std::iter::from_fn(|| step(it)).collect()),
        _ => return None,
    };
    Some(std::iter::from_fn(|| step(&mut it)).collect())
}

/// What a `for...of` loop walks `v` with (`collection::Iter`): an array
/// or a string itself, a cursor for a `Map`, a `Set` or an iterator. Any
/// other value is left as it is.
#[unsafe(no_mangle)]
pub extern "C" fn ot_get_iterator(v: u64) -> u64 {
    let iterator = match (collection_of(v), iter_of(v)) {
        (Some((_, collection)), _) => {
            new_iterator(Iter::new(v, collection::default_yields(collection)))
        }
        (_, Some(it)) if !it.for_of => v,
        _ => return v,
    };
    new_iterator(Iter::for_of(iterator))
}

/// A `for...of` cursor's `length`.
pub(super) fn cursor_length(cursor: &Iter<u64>) -> u64 {
    let more = iter_of(cursor.source).is_some_and(|it| {
        collection_of(it.source).is_some_and(|(table, _)| table.peek(it.cursor).is_some())
    });
    number(collection::cursor_length(more))
}

/// Reading an element of the `for...of` cursor `cursor`: its next item.
pub(super) fn cursor_take(cursor: &Iter<u64>) -> u64 {
    iter_of(cursor.source)
        .and_then(step)
        .unwrap_or_else(undefined)
}

/// `size`, and the built-in methods, of a `Map`, a `Set` or an iterator;
/// a cursor's `length`.
pub(super) fn collection_property(v: u64, key: Atom) -> u64 {
    if let Some((table, _)) = collection_of(v)
        && atom::name(key) == "size"
    {
        return number(table.len() as f64);
    }
    if let Some(it) = iter_of(v)
        && it.for_of
    {
        return if key == atom::LENGTH {
            cursor_length(it)
        } else {
            undefined()
        };
    }
    receiver_of(v)
        .and_then(|r| builtin_method(r, atom::name(key)))
        .unwrap_or_else(undefined)
}

/// How `console.log` shows a `Map`, a `Set` or an iterator.
pub(super) fn collection_summary(v: u64, depth: usize, seen: &mut Vec<usize>) -> String {
    let show = |x: u64, depth: usize, seen: &mut Vec<usize>| {
        inspect_inner(OtValue::from_bits(x), depth, seen)
    };
    if let Some((table, collection)) = collection_of(v) {
        if depth > INSPECT_DEPTH {
            return format!("[{}]", collection.name());
        }
        let parts: Vec<String> = table
            .iter()
            .map(|(&k, &val)| match collection {
                Receiver::Map => format!(
                    "{} => {}",
                    show(k, depth + 1, seen),
                    show(val, depth + 1, seen)
                ),
                _ => show(k, depth + 1, seen),
            })
            .collect();
        return collection::summary(collection, &parts);
    }
    let Some(it) = iter_of(v) else {
        return "[Object]".to_string();
    };
    let Some((table, collection)) = collection_of(it.source) else {
        return "[Object]".to_string();
    };
    // What it has left, without taking it.
    let parts: Vec<String> = table
        .rest(it.cursor)
        .map(|(&k, &val)| match it.yields {
            Yields::Keys => show(k, depth + 1, seen),
            Yields::Values => show(val, depth + 1, seen),
            Yields::Entries => format!(
                "[ {}, {} ]",
                show(k, depth + 2, seen),
                show(val, depth + 2, seen)
            ),
        })
        .collect();
    collection::iterator_summary(collection, it.yields, &parts)
}
