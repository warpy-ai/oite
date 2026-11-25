//! Global variables: the runtime's own (`console`, `Math`, ...) and the
//! slots compiled code registers.

use super::*;

// =========================================================================
// Globals
// =========================================================================

thread_local! {
    pub(super) static GLOBALS: RefCell<HashMap<String, u64>> = RefCell::new(initial_globals());
    /// The slots native code keeps the program's global variables in, by
    /// name: compiled code reads and writes them directly, and every lookup
    /// by name here sees the same value.
    pub(super) static GLOBAL_SLOTS: RefCell<HashMap<String, *mut u64>> = RefCell::new(HashMap::new());
}

/// The global `name`: its slot once assigned, else the runtime's own
/// globals (`console`, `Math`, ...). `None`: not defined.
pub(super) fn global(name: &str) -> Option<u64> {
    let slot = GLOBAL_SLOTS.with(|s| s.borrow().get(name).copied());
    if let Some(slot) = slot {
        let v = unsafe { *slot };
        if v != UNBOUND_BITS {
            return Some(v);
        }
    }
    GLOBALS.with(|g| g.borrow().get(name).copied())
}

/// Register the slot that holds the global variable `key` (at program
/// start, before any code runs). It holds `UNBOUND_BITS` until assigned.
#[unsafe(no_mangle)]
pub extern "C" fn ot_register_global(key: *const u8, key_len: usize, slot: *mut u64) {
    let key = unsafe { name(key, key_len) };
    GLOBAL_SLOTS.with(|s| {
        s.borrow_mut().insert(key.to_string(), slot);
    });
}

/// `Array.prototype[key]`, if it is a function.
pub(super) fn array_proto_method(key: &str) -> Option<u64> {
    let class = global("Array")?;
    let (ptr, ObjectKind::Object) = object(OtValue::from_bits(class))? else {
        return None;
    };
    let proto = lookup(ptr, "prototype")?;
    let (proto, ObjectKind::Object) = object(OtValue::from_bits(proto))? else {
        return None;
    };
    let method = lookup(proto, key)?;
    is_function(method).then_some(method)
}

pub(super) fn builtin(code: NativeFn) -> u64 {
    ot_make_function(code as *const u8)
}

/// An object holding the built-in methods of `receiver`, for
/// `String.prototype`, `Number.prototype` and `Array.prototype`: the same
/// values `"a".trim`, `(1).toFixed` and `[].push` are.
fn builtin_prototype(receiver: builtins::Receiver) -> u64 {
    let proto = ot_alloc_object();
    for name in receiver.methods() {
        let method = builtin_method(receiver, name).expect("a listed method");
        set_property(OtValue::from_bits(proto), name, method);
    }
    proto
}

pub(super) fn initial_globals() -> HashMap<String, u64> {
    let mut g = HashMap::new();
    let console = ot_alloc_object();
    let c = OtValue::from_bits(console);
    // `info` and `debug` print like `log`, `warn` like `error`, as in Node.
    let (log, error) = (
        builtin(ot_builtin_console_log),
        builtin(ot_builtin_console_error),
    );
    for name in ["log", "info", "debug"] {
        set_property(c, name, log);
    }
    for name in ["error", "warn"] {
        set_property(c, name, error);
    }
    g.insert("console".to_string(), console);
    let object = ot_alloc_object();
    set_property(
        OtValue::from_bits(object),
        "keys",
        builtin(ot_builtin_object_keys),
    );
    g.insert("Object".to_string(), object);
    // `Array`: on `Array.prototype`, the methods built into the runtime,
    // and the ones core does not implement, which the prelude
    // (`@rolls/std`) or a program adds.
    let array_class = ot_alloc_object();
    set_property(
        OtValue::from_bits(array_class),
        "prototype",
        builtin_prototype(builtins::Receiver::Array),
    );
    g.insert("Array".to_string(), array_class);
    // `Math`: one built-in serves every function member; its environment
    // is the member's index in `MATH_FUNCTIONS`.
    let math = ot_alloc_object();
    let m = OtValue::from_bits(math);
    for (i, name) in builtins::MATH_FUNCTIONS.iter().enumerate() {
        let f = ot_make_closure(ot_builtin_math as *const u8, number(i as f64));
        set_property(m, name, f);
    }
    for (name, value) in builtins::MATH_CONSTANTS {
        set_property(m, name, number(*value));
    }
    g.insert("Math".to_string(), math);
    // `String(v)` and `Number(v)`, with the built-in methods on their
    // `prototype`.
    for (name, code, receiver) in [
        (
            "String",
            ot_builtin_string as NativeFn,
            builtins::Receiver::String,
        ),
        ("Number", ot_builtin_number, builtins::Receiver::Number),
    ] {
        let f = builtin(code);
        set_property(
            OtValue::from_bits(f),
            "prototype",
            builtin_prototype(receiver),
        );
        g.insert(name.to_string(), f);
    }
    let promise = ot_alloc_object();
    let pr = OtValue::from_bits(promise);
    for (i, name) in ["resolve", "reject", "all", "race"].iter().enumerate() {
        let f = ot_make_closure(ot_builtin_promise_static as *const u8, number(i as f64));
        set_property(pr, name, f);
    }
    set_property(pr, "__promise_ctor__", boolean(true));
    g.insert("Promise".to_string(), promise);
    g.insert("Map".to_string(), collection_constructor(0));
    g.insert("Set".to_string(), collection_constructor(1));
    for (i, name) in ["setTimeout", "clearTimeout"].iter().enumerate() {
        let f = ot_make_closure(ot_builtin_timer as *const u8, number(i as f64));
        g.insert(name.to_string(), f);
    }
    let date = ot_alloc_object();
    set_property(
        OtValue::from_bits(date),
        "now",
        builtin(ot_builtin_date_now),
    );
    g.insert("Date".to_string(), date);
    g.insert("undefined".to_string(), undefined());
    g.insert("NaN".to_string(), number(f64::NAN));
    g.insert("Infinity".to_string(), number(f64::INFINITY));
    g
}

/// Read a global variable; a `ReferenceError` if it was never set.
#[unsafe(no_mangle)]
pub extern "C" fn ot_get_global(key: *const u8, key_len: usize) -> u64 {
    let key = unsafe { name(key, key_len) };
    match global(key) {
        Some(v) => v,
        None => throw_error("ReferenceError", &format!("{key} is not defined")),
    }
}

/// `typeof name` of a global: "undefined" if it was never set.
#[unsafe(no_mangle)]
pub extern "C" fn ot_typeof_global(key: *const u8, key_len: usize) -> u64 {
    let key = unsafe { name(key, key_len) };
    match global(key) {
        Some(v) => ot_typeof(v),
        None => new_string("undefined"),
    }
}

/// Write a global variable.
#[unsafe(no_mangle)]
pub extern "C" fn ot_set_global(key: *const u8, key_len: usize, value: u64) -> u64 {
    let key = unsafe { name(key, key_len) };
    match GLOBAL_SLOTS.with(|s| s.borrow().get(key).copied()) {
        Some(slot) => unsafe { *slot = value },
        None => GLOBALS.with(|g| {
            g.borrow_mut().insert(key.to_string(), value);
        }),
    }
    undefined()
}
