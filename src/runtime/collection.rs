//! `Map` and `Set`, shared by the VM and native code.
//!
//! A collection is a `Table`: its entries in insertion order, found by
//! hashing their keys. Keys are equal when they are SameValueZero (`===`,
//! except that NaN equals NaN), so objects compare by identity, strings
//! by content, and `-0` is the key `0`. A `Set` keeps each value as both
//! key and value, which is what `entries()` and `forEach` give.
//!
//! Iteration is live, as in JavaScript: an iterator visits entries added
//! after it started and skips ones deleted before its turn. An iterator
//! stands on the entry it gave last (`Cursor`). Deleting that entry keeps
//! its slot, so the iterator can still step past it, until no iterator
//! stands there.

use super::builtins::{Error, Host, Outcome, Receiver};
use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};

/// No slot.
const NONE: u32 = u32::MAX;

/// The slot that starts and ends the list of entries; it holds none.
const HEAD: u32 = 0;

/// `a` and `b` are the same key: SameValueZero.
pub fn same_value_zero<H: Host>(h: &H, a: &H::Value, b: &H::Value) -> bool {
    match (h.number(a), h.number(b)) {
        (Some(x), Some(y)) => x == y || (x.is_nan() && y.is_nan()),
        _ => h.strict_equal(a, b),
    }
}

/// A hash that keys equal under `same_value_zero` share.
fn hash_key<H: Host>(h: &H, key: &H::Value) -> u64 {
    let raw = if let Some(n) = h.number(key) {
        if n.is_nan() {
            f64::NAN.to_bits()
        } else if n == 0.0 {
            0
        } else {
            n.to_bits()
        }
    } else if let Some(s) = h.string(key) {
        let mut hasher = std::hash::DefaultHasher::new();
        hasher.write(s.as_bytes());
        hasher.finish()
    } else {
        h.identity(key)
    };
    // splitmix64's finalizer: every bit of the key reaches the bits the
    // index picks a bucket with.
    let mut x = raw;
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    x ^ (x >> 31)
}

/// The index's hasher: keys arrive hashed (`hash_key`).
#[derive(Default)]
struct Prehashed(u64);

impl Hasher for Prehashed {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, _: &[u8]) {
        unreachable!("the index hashes only u64 keys");
    }

    fn write_u64(&mut self, n: u64) {
        self.0 = n;
    }
}

#[derive(Clone, Debug)]
struct Slot<V> {
    /// The key and value; `None` once deleted (or free).
    entry: Option<(V, V)>,
    hash: u64,
    /// The next slot whose key has the same hash.
    chain: u32,
    /// Neighbours in insertion order; the list is a ring through `HEAD`.
    /// A free slot's `next` is the next free slot.
    prev: u32,
    next: u32,
    /// How many iterators stand on this slot.
    pins: u32,
}

impl<V> Slot<V> {
    fn head() -> Self {
        Slot {
            entry: None,
            hash: 0,
            chain: NONE,
            prev: HEAD,
            next: HEAD,
            pins: 0,
        }
    }
}

/// Where an iterator is: the slot of the entry it gave last, `HEAD`
/// before the first, or done.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cursor(u32);

impl Cursor {
    /// Before the first entry.
    pub const START: Cursor = Cursor(HEAD);

    pub fn is_done(self) -> bool {
        self.0 == NONE
    }
}

/// The entries of a `Map` or a `Set`.
#[derive(Clone, Debug)]
pub struct Table<V> {
    /// Slot 0 is `HEAD`.
    slots: Vec<Slot<V>>,
    /// A key's hash: the newest slot with that hash.
    index: HashMap<u64, u32, BuildHasherDefault<Prehashed>>,
    /// The first free slot.
    free: u32,
    len: usize,
}

impl<V> Default for Table<V> {
    fn default() -> Self {
        Table {
            slots: vec![Slot::head()],
            index: HashMap::default(),
            free: NONE,
            len: 0,
        }
    }
}

impl<V: Clone> Table<V> {
    /// How many entries it has: `size`.
    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// The slot holding `key`.
    fn find<H: Host<Value = V>>(&self, h: &H, key: &V, hash: u64) -> Option<u32> {
        let mut i = *self.index.get(&hash)?;
        while i != NONE {
            let slot = &self.slots[i as usize];
            if let Some((k, _)) = &slot.entry
                && slot.hash == hash
                && same_value_zero(h, k, key)
            {
                return Some(i);
            }
            i = slot.chain;
        }
        None
    }

    /// The value of `key`.
    pub fn get<H: Host<Value = V>>(&self, h: &H, key: &V) -> Option<&V> {
        let i = self.find(h, key, hash_key(h, key))?;
        self.slots[i as usize].entry.as_ref().map(|(_, v)| v)
    }

    pub fn has<H: Host<Value = V>>(&self, h: &H, key: &V) -> bool {
        self.find(h, key, hash_key(h, key)).is_some()
    }

    /// Set `key` to `value`. A new key goes last; a key already there
    /// keeps its place.
    pub fn insert<H: Host<Value = V>>(&mut self, h: &H, key: V, value: V) {
        let hash = hash_key(h, &key);
        if let Some(i) = self.find(h, &key, hash) {
            if let Some((_, v)) = &mut self.slots[i as usize].entry {
                *v = value;
            }
            return;
        }
        // The key -0 is stored as 0.
        let key = if h.number(&key) == Some(0.0) {
            h.make_number(0.0)
        } else {
            key
        };
        let tail = self.slots[HEAD as usize].prev;
        let slot = Slot {
            entry: Some((key, value)),
            hash,
            chain: self.index.get(&hash).copied().unwrap_or(NONE),
            prev: tail,
            next: HEAD,
            pins: 0,
        };
        let i = if self.free != NONE {
            let i = self.free;
            self.free = self.slots[i as usize].next;
            self.slots[i as usize] = slot;
            i
        } else {
            self.slots.push(slot);
            (self.slots.len() - 1) as u32
        };
        self.slots[tail as usize].next = i;
        self.slots[HEAD as usize].prev = i;
        self.index.insert(hash, i);
        self.len += 1;
    }

    /// Delete `key`; whether it was there.
    pub fn remove<H: Host<Value = V>>(&mut self, h: &H, key: &V) -> bool {
        let hash = hash_key(h, key);
        let Some(i) = self.find(h, key, hash) else {
            return false;
        };
        let chain = self.slots[i as usize].chain;
        let first = self.index[&hash];
        if first == i {
            if chain == NONE {
                self.index.remove(&hash);
            } else {
                self.index.insert(hash, chain);
            }
        } else {
            let mut p = first;
            while self.slots[p as usize].chain != i {
                p = self.slots[p as usize].chain;
            }
            self.slots[p as usize].chain = chain;
        }
        self.slots[i as usize].entry = None;
        self.len -= 1;
        if self.slots[i as usize].pins == 0 {
            self.unlink(i);
        }
        true
    }

    /// Delete every entry.
    pub fn clear(&mut self) {
        let mut i = self.slots[HEAD as usize].next;
        while i != HEAD {
            let next = self.slots[i as usize].next;
            self.slots[i as usize].entry = None;
            if self.slots[i as usize].pins == 0 {
                self.unlink(i);
            }
            i = next;
        }
        self.index.clear();
        self.len = 0;
        // No iterator stands on any slot: start afresh.
        if self.slots[HEAD as usize].next == HEAD {
            *self = Table::default();
        }
    }

    /// Take the deleted slot `i` out of the list and free it.
    fn unlink(&mut self, i: u32) {
        let Slot { prev, next, .. } = self.slots[i as usize];
        self.slots[prev as usize].next = next;
        self.slots[next as usize].prev = prev;
        let slot = &mut self.slots[i as usize];
        slot.chain = NONE;
        slot.prev = NONE;
        slot.next = self.free;
        self.free = i;
    }

    /// The first entry after the slot `from`, skipping deleted ones.
    fn after(&self, from: u32) -> Option<u32> {
        let mut i = self.slots[from as usize].next;
        while i != HEAD {
            if self.slots[i as usize].entry.is_some() {
                return Some(i);
            }
            i = self.slots[i as usize].next;
        }
        None
    }

    /// The entry an iterator at `cursor` gives next, without moving it.
    pub fn peek(&self, cursor: Cursor) -> Option<(&V, &V)> {
        if cursor.is_done() {
            return None;
        }
        let i = self.after(cursor.0)?;
        self.slots[i as usize].entry.as_ref().map(|(k, v)| (k, v))
    }

    /// Move the iterator at `cursor` to its next entry and give it; at the
    /// end, the cursor is done.
    pub fn advance(&mut self, cursor: &mut Cursor) -> Option<(V, V)> {
        if cursor.is_done() {
            return None;
        }
        let next = self.after(cursor.0);
        self.release(*cursor);
        let Some(i) = next else {
            *cursor = Cursor(NONE);
            return None;
        };
        self.slots[i as usize].pins += 1;
        *cursor = Cursor(i);
        self.slots[i as usize].entry.clone()
    }

    /// The iterator at `cursor` is finished with this table.
    pub fn release(&mut self, cursor: Cursor) {
        let i = cursor.0;
        if i == HEAD || i == NONE {
            return;
        }
        let slot = &mut self.slots[i as usize];
        slot.pins -= 1;
        if slot.pins == 0 && slot.entry.is_none() {
            self.unlink(i);
        }
    }

    /// The entries in order.
    pub fn iter(&self) -> impl Iterator<Item = (&V, &V)> {
        let mut i = HEAD;
        std::iter::from_fn(move || {
            i = self.after(i)?;
            self.slots[i as usize].entry.as_ref().map(|(k, v)| (k, v))
        })
    }

    /// The entries an iterator at `cursor` has still to give.
    pub fn rest(&self, cursor: Cursor) -> impl Iterator<Item = (&V, &V)> {
        let mut i = cursor.0;
        std::iter::from_fn(move || {
            if i == NONE {
                return None;
            }
            i = self.after(i)?;
            self.slots[i as usize].entry.as_ref().map(|(k, v)| (k, v))
        })
    }
}

/// What an iterator gives for each entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Yields {
    Keys,
    Values,
    /// `[key, value]`
    Entries,
}

impl Yields {
    /// The iterator method that makes such an iterator, if `name` is one.
    pub fn of_method(name: &str) -> Option<Yields> {
        match name {
            "keys" => Some(Yields::Keys),
            "values" => Some(Yields::Values),
            "entries" => Some(Yields::Entries),
            _ => None,
        }
    }

    /// What an iterator of this kind gives for the entry `(key, value)`.
    pub fn item<V>(self, key: V, value: V) -> Outcome<V> {
        match self {
            Yields::Keys => Outcome::Value(key),
            Yields::Values => Outcome::Value(value),
            Yields::Entries => Outcome::NewArray(vec![key, value]),
        }
    }

    pub fn bits(self) -> u8 {
        self as u8
    }

    pub fn from_bits(bits: u8) -> Yields {
        match bits & 3 {
            0 => Yields::Keys,
            1 => Yields::Values,
            _ => Yields::Entries,
        }
    }
}

/// An iterator over a `Map` or a `Set` (`m.keys()`), or the cursor a
/// `for...of` loop walks a collection or such an iterator with.
///
/// A `for...of` loop walks every iterable the way it walks an array: while
/// `i < it.length`, the element `it[i]`. A cursor's `length` is Infinity
/// while there is an entry to give and 0 after, and reading any element
/// gives the next one: the loop reads one after each `length` that is not
/// 0. Arrays and strings are their own cursors.
#[derive(Clone, Debug)]
pub struct Iter<V> {
    /// The `Map` or `Set`; for a cursor, the iterator it advances.
    pub source: V,
    pub cursor: Cursor,
    pub yields: Yields,
    pub for_of: bool,
}

impl<V> Iter<V> {
    /// A new iterator over the collection `source`.
    pub fn new(source: V, yields: Yields) -> Self {
        Iter {
            source,
            cursor: Cursor::START,
            yields,
            for_of: false,
        }
    }

    /// A `for...of` cursor over the iterator `iterator`.
    pub fn for_of(iterator: V) -> Self {
        Iter {
            source: iterator,
            cursor: Cursor::START,
            yields: Yields::Values,
            for_of: true,
        }
    }
}

/// A cursor's `length`: Infinity while `more`, then 0.
pub fn cursor_length(more: bool) -> f64 {
    if more { f64::INFINITY } else { 0.0 }
}

/// `console.log` of an iterator over a `collection` that has `parts`
/// left: `[Map Iterator] { 'a', 'b' }`, `[Set Entries] { [ 1, 1 ] }`.
pub fn iterator_summary(collection: Receiver, yields: Yields, parts: &[String]) -> String {
    let tag = match (collection, yields) {
        (Receiver::Map, Yields::Entries) => "[Map Entries]",
        (Receiver::Set, Yields::Entries) => "[Set Entries]",
        (Receiver::Map, _) => "[Map Iterator]",
        _ => "[Set Iterator]",
    };
    format!("{tag} {{ {} }}", parts.join(", "))
}

/// The iterator a `for...of` loop or a spread walks a collection with: a
/// `Map` gives its entries, a `Set` its values.
pub fn default_yields(collection: Receiver) -> Yields {
    if collection == Receiver::Map {
        Yields::Entries
    } else {
        Yields::Values
    }
}

/// `get`, `set`, `has`, `delete`, `clear` of a `Map`, and `add`, `has`,
/// `delete`, `clear` of a `Set` (`collection`). `None` for any other
/// method: the backends make iterators and run `forEach`.
pub fn method<H: Host>(
    h: &H,
    table: &mut Table<H::Value>,
    collection: Receiver,
    name: &str,
    args: &[H::Value],
) -> Option<Result<Outcome<H::Value>, Error>> {
    let arg = |i: usize| args.get(i).cloned().unwrap_or_else(|| h.undefined());
    let value = match (collection, name) {
        (Receiver::Map, "get") => table
            .get(h, &arg(0))
            .cloned()
            .unwrap_or_else(|| h.undefined()),
        (Receiver::Map, "set") => {
            table.insert(h, arg(0), arg(1));
            return Some(Ok(Outcome::This));
        }
        (Receiver::Set, "add") => {
            table.insert(h, arg(0), arg(0));
            return Some(Ok(Outcome::This));
        }
        (_, "has") => h.make_bool(table.has(h, &arg(0))),
        (_, "delete") => h.make_bool(table.remove(h, &arg(0))),
        (_, "clear") => {
            table.clear();
            h.undefined()
        }
        _ => return None,
    };
    Some(Ok(Outcome::Value(value)))
}

/// Fill a new collection from what `new Map(items)` or `new Set(items)`
/// iterates. A `Map`'s items must be `[key, value]` arrays.
pub fn fill<H: Host>(
    h: &H,
    table: &mut Table<H::Value>,
    collection: Receiver,
    items: Vec<H::Value>,
) -> Result<(), Error> {
    for item in items {
        if collection == Receiver::Set {
            table.insert(h, item.clone(), item);
            continue;
        }
        let Some(pair) = h.array_elements(&item) else {
            return Err(Error::type_error(format!(
                "Iterator value {} is not an entry object",
                h.to_string(&item)
            )));
        };
        let mut pair = pair.into_iter();
        let key = pair.next().unwrap_or_else(|| h.undefined());
        let value = pair.next().unwrap_or_else(|| h.undefined());
        table.insert(h, key, value);
    }
    Ok(())
}

/// `Map(...)` without `new`.
pub fn requires_new(collection: Receiver) -> Error {
    Error::type_error(format!("Constructor {} requires 'new'", collection.name()))
}

/// `console.log` of a collection whose entries print as `parts`:
/// `Map(2) { 'a' => 1, 'b' => 2 }`, `Set(0) {}`.
pub fn summary(collection: Receiver, parts: &[String]) -> String {
    if parts.is_empty() {
        format!("{}(0) {{}}", collection.name())
    } else {
        format!(
            "{}({}) {{ {} }}",
            collection.name(),
            parts.len(),
            parts.join(", ")
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::builtins::Kind;

    /// Numbers and strings, as a test's values.
    #[derive(Clone, Debug, PartialEq)]
    enum V {
        N(f64),
        S(String),
        Undefined,
    }

    struct H;

    impl Host for H {
        type Value = V;
        fn number(&self, v: &V) -> Option<f64> {
            match v {
                V::N(n) => Some(*n),
                _ => None,
            }
        }
        fn string<'a>(&'a self, v: &'a V) -> Option<&'a str> {
            match v {
                V::S(s) => Some(s),
                _ => None,
            }
        }
        fn to_string(&self, v: &V) -> String {
            format!("{v:?}")
        }
        fn strict_equal(&self, a: &V, b: &V) -> bool {
            a == b
        }
        fn array_elements(&self, _: &V) -> Option<Vec<V>> {
            None
        }
        fn is_array(&self, _: &V) -> bool {
            false
        }
        fn kind(&self, _: &V) -> Kind {
            Kind::Object
        }
        fn own_keys(&self, _: &V) -> Vec<String> {
            Vec::new()
        }
        fn own_entries(&self, _: &V) -> Vec<(String, V)> {
            Vec::new()
        }
        fn identity(&self, _: &V) -> u64 {
            0
        }
        fn receiver(&self, _: &V) -> Option<Receiver> {
            None
        }
        fn make_number(&self, n: f64) -> V {
            V::N(n)
        }
        fn make_string(&self, s: String) -> V {
            V::S(s)
        }
        fn make_bool(&self, b: bool) -> V {
            V::N(b as u8 as f64)
        }
        fn undefined(&self) -> V {
            V::Undefined
        }
    }

    fn keys(t: &Table<V>) -> Vec<V> {
        t.iter().map(|(k, _)| k.clone()).collect()
    }

    #[test]
    fn keys_are_same_value_zero() {
        let mut t = Table::default();
        t.insert(&H, V::N(f64::NAN), V::N(1.0));
        t.insert(&H, V::N(-0.0), V::N(2.0));
        t.insert(&H, V::S("0".into()), V::N(3.0));
        assert_eq!(t.get(&H, &V::N(f64::NAN)), Some(&V::N(1.0)));
        assert_eq!(t.get(&H, &V::N(0.0)), Some(&V::N(2.0)));
        assert_eq!(t.get(&H, &V::S("0".into())), Some(&V::N(3.0)));
        // The key -0 is stored as 0.
        assert!(H.number(&keys(&t)[1]).unwrap().is_sign_positive());
    }

    #[test]
    fn order_and_reuse() {
        let mut t = Table::default();
        for i in 0..100 {
            t.insert(&H, V::N(i as f64), V::Undefined);
        }
        for i in (0..100).step_by(2) {
            assert!(t.remove(&H, &V::N(i as f64)));
        }
        assert!(!t.remove(&H, &V::N(0.0)));
        // Freed slots are reused, and new keys still go last.
        for i in 100..150 {
            t.insert(&H, V::N(i as f64), V::Undefined);
        }
        assert_eq!(t.slots.len(), 101);
        let want: Vec<V> = (1..100)
            .step_by(2)
            .chain(100..150)
            .map(|i| V::N(i as f64))
            .collect();
        assert_eq!(keys(&t), want);
        assert_eq!(t.len(), 100);
    }

    /// Every key with the same hash: lookups and deletes walk the chain.
    #[test]
    fn hash_collisions() {
        struct Same;
        impl Host for Same {
            type Value = V;
            fn number(&self, _: &V) -> Option<f64> {
                None
            }
            fn string<'a>(&'a self, _: &'a V) -> Option<&'a str> {
                None
            }
            fn to_string(&self, v: &V) -> String {
                H.to_string(v)
            }
            fn strict_equal(&self, a: &V, b: &V) -> bool {
                a == b
            }
            fn array_elements(&self, _: &V) -> Option<Vec<V>> {
                None
            }
            fn is_array(&self, _: &V) -> bool {
                false
            }
            fn kind(&self, _: &V) -> Kind {
                Kind::Object
            }
            fn own_keys(&self, _: &V) -> Vec<String> {
                Vec::new()
            }
            fn own_entries(&self, _: &V) -> Vec<(String, V)> {
                Vec::new()
            }
            fn identity(&self, _: &V) -> u64 {
                7
            }
            fn receiver(&self, _: &V) -> Option<Receiver> {
                None
            }
            fn make_number(&self, n: f64) -> V {
                V::N(n)
            }
            fn make_string(&self, s: String) -> V {
                V::S(s)
            }
            fn make_bool(&self, b: bool) -> V {
                H.make_bool(b)
            }
            fn undefined(&self) -> V {
                V::Undefined
            }
        }
        let mut t = Table::default();
        for i in 0..5 {
            t.insert(&Same, V::N(i as f64), V::N(i as f64 * 10.0));
        }
        assert!(t.remove(&Same, &V::N(2.0)));
        assert!(t.remove(&Same, &V::N(4.0)));
        assert!(t.remove(&Same, &V::N(0.0)));
        assert_eq!(t.get(&Same, &V::N(1.0)), Some(&V::N(10.0)));
        assert_eq!(t.get(&Same, &V::N(3.0)), Some(&V::N(30.0)));
        assert_eq!(t.get(&Same, &V::N(2.0)), None);
        assert_eq!(t.len(), 2);
    }

    /// An iterator steps past the entry it stands on after that entry is
    /// deleted, and sees entries added after a `clear`.
    #[test]
    fn live_iteration() {
        let mut t = Table::default();
        for i in 1..=3 {
            t.insert(&H, V::N(i as f64), V::Undefined);
        }
        let mut c = Cursor::START;
        assert_eq!(t.advance(&mut c).map(|e| e.0), Some(V::N(1.0)));
        t.remove(&H, &V::N(1.0));
        t.remove(&H, &V::N(2.0));
        assert_eq!(t.peek(c).map(|e| e.0.clone()), Some(V::N(3.0)));
        assert_eq!(t.advance(&mut c).map(|e| e.0), Some(V::N(3.0)));
        t.clear();
        t.insert(&H, V::N(9.0), V::Undefined);
        assert_eq!(t.advance(&mut c).map(|e| e.0), Some(V::N(9.0)));
        assert_eq!(t.advance(&mut c), None);
        assert!(c.is_done());
        // Nothing stands on a slot now: deleting frees it.
        t.remove(&H, &V::N(9.0));
        assert_eq!(t.slots[HEAD as usize].next, HEAD);
    }
}
