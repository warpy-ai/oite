//! Atoms: property names interned as integers, so that comparing two names
//! is comparing two numbers. Interned names live for the whole run.

use std::cell::RefCell;
use std::collections::HashMap;

/// An interned property name. Accessor names (`getter:x`, `setter:x`) have
/// `ACCESSOR_NAME` set, so telling them from plain names needs no lookup.
pub type Atom = u32;

const ACCESSOR_NAME: Atom = 1 << 31;

/// Whether `atom` is an ordinary property name (not `getter:x`/`setter:x`).
#[inline]
pub fn is_plain(atom: Atom) -> bool {
    atom & ACCESSOR_NAME == 0
}

/// What a name stands for. The compiler defines an accessor by storing a
/// function under `getter:x` or `setter:x`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Role {
    Plain,
    /// `getter:<name>`: the getter of the property `name`.
    Getter(Atom),
    /// `setter:<name>`: the setter of the property `name`.
    Setter(Atom),
}

#[derive(Clone, Copy)]
pub struct AtomInfo {
    pub name: &'static str,
    pub role: Role,
    /// Left out of `Object.keys` and `console.log` (`builtins::is_hidden_key`).
    pub hidden: bool,
}

/// Names the runtime itself asks for, interned first, in this order.
const WELL_KNOWN: &[&str] = &[
    "__proto__",
    "length",
    "then",
    "catch",
    "finally",
    "__promise__",
    "__class__",
    "prototype",
    "constructor",
    "__promise_ctor__",
];
pub const PROTO: Atom = 0;
pub const LENGTH: Atom = 1;
pub const THEN: Atom = 2;
pub const CATCH: Atom = 3;
pub const FINALLY: Atom = 4;
pub const PROMISE: Atom = 5;
pub const CLASS: Atom = 6;
pub const PROTOTYPE: Atom = 7;
pub const CONSTRUCTOR: Atom = 8;
pub const PROMISE_CTOR: Atom = 9;

/// Entries in the cache of names compiled code passes (a power of two).
const STATIC_CACHE: usize = 1024;

struct Table {
    by_name: HashMap<&'static str, Atom>,
    infos: Vec<AtomInfo>,
}

impl Table {
    fn new() -> Self {
        let mut t = Table {
            by_name: HashMap::new(),
            infos: Vec::new(),
        };
        for name in WELL_KNOWN {
            t.intern(name);
        }
        t
    }

    fn intern(&mut self, name: &str) -> Atom {
        if let Some(a) = self.by_name.get(name) {
            return *a;
        }
        let role = if let Some(rest) = name.strip_prefix("getter:") {
            Role::Getter(self.intern(rest))
        } else if let Some(rest) = name.strip_prefix("setter:") {
            Role::Setter(self.intern(rest))
        } else {
            Role::Plain
        };
        let name: &'static str = Box::leak(name.to_string().into_boxed_str());
        let mut atom = self.infos.len() as Atom;
        if role != Role::Plain {
            atom |= ACCESSOR_NAME;
        }
        self.infos.push(AtomInfo {
            name,
            role,
            hidden: super::super::builtins::is_hidden_key(name),
        });
        self.by_name.insert(name, atom);
        atom
    }
}

thread_local! {
    static TABLE: RefCell<Table> = RefCell::new(Table::new());
    /// (pointer, length, atom) of names in compiled code: those are
    /// constants, so the same address is the same name.
    static STATICS: std::cell::UnsafeCell<[(usize, usize, Atom); STATIC_CACHE]> =
        const { std::cell::UnsafeCell::new([(0, 0, 0); STATIC_CACHE]) };
}

/// The atom of `name`.
pub fn atom(name: &str) -> Atom {
    TABLE.with(|t| t.borrow_mut().intern(name))
}

/// The atom of a name constant in compiled code.
///
/// # Safety
/// `ptr..ptr+len` must be valid UTF-8 that never changes or moves.
pub unsafe fn static_atom(ptr: *const u8, len: usize) -> Atom {
    STATICS.with(|cache| {
        let p = ptr as usize;
        let slot = (p ^ (p >> 10) ^ len) & (STATIC_CACHE - 1);
        // Nothing else holds a reference into the cache.
        let entry = unsafe { &mut (*cache.get())[slot] };
        if entry.0 == p && entry.1 == len && len != 0 {
            return entry.2;
        }
        let name = unsafe { std::str::from_utf8_unchecked(std::slice::from_raw_parts(ptr, len)) };
        let a = atom(name);
        *entry = (p, len, a);
        a
    })
}

pub fn info(atom: Atom) -> AtomInfo {
    TABLE.with(|t| t.borrow().infos[(atom & !ACCESSOR_NAME) as usize])
}

pub fn name(atom: Atom) -> &'static str {
    info(atom).name
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interning() {
        assert_eq!(atom("__proto__"), PROTO);
        assert_eq!(atom("length"), LENGTH);
        assert_eq!(atom("__class__"), CLASS);
        assert_eq!(atom("prototype"), PROTOTYPE);
        assert_eq!(atom("constructor"), CONSTRUCTOR);
        assert_eq!(atom("__promise_ctor__"), PROMISE_CTOR);
        let x = atom("x");
        assert_eq!(atom("x"), x);
        assert_ne!(atom("y"), x);
        assert_eq!(name(x), "x");
        assert!(is_plain(x) && !is_plain(atom("getter:x")));
        assert_eq!(info(atom("getter:x")).role, Role::Getter(x));
        assert_eq!(info(atom("setter:x")).role, Role::Setter(x));
        assert!(info(atom("#p@1")).hidden);
        assert!(!info(x).hidden);
        let s = "static name";
        let a = unsafe { static_atom(s.as_ptr(), s.len()) };
        assert_eq!(a, atom("static name"));
        assert_eq!(unsafe { static_atom(s.as_ptr(), s.len()) }, a);
    }
}
