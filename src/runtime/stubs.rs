//! Runtime stubs callable from native-compiled code
//!
//! These `extern "C"` functions are the runtime of Oite's native backends.
//! The Cranelift JIT calls them in-process; LLVM AOT binaries link them from
//! the static library that `build.rs` compiles from this same file. Keeping
//! the semantics here, instead of in each backend, is what makes the two
//! backends agree.
//!
//! The calling convention is:
//! - All values are passed and returned as `u64` (NaN-boxed `OtValue`).
//! - Every function value, compiled or built in, has the signature
//!   `extern "C" fn(this: u64, argv: *const u64, argc: usize) -> u64`
//!   (see `heap::NativeFn`). Missing arguments read as `undefined`.
//! - Names (properties, globals) are passed as a UTF-8 pointer and length.

use super::abi::{EXCEPTION_BITS, OtValue};
use super::builtins;
use super::builtins::is_hidden_key;
use super::heap::{
    HeapPtr, NativeArray, NativeFn, NativeFunction, NativeObject, NativeString, ObjectHeader,
    ObjectKind, PropertyMap, heap,
};
use std::cell::RefCell;
use std::collections::HashMap;
use std::io::Write;

// =========================================================================
// Helpers
// =========================================================================

fn undefined() -> u64 {
    OtValue::undefined().to_bits()
}

fn boolean(b: bool) -> u64 {
    OtValue::boolean(b).to_bits()
}

fn number(n: f64) -> u64 {
    OtValue::number(n).to_bits()
}

/// An unrecoverable condition (out of memory): report it and exit.
fn fatal_error(msg: &str) -> ! {
    let _ = std::io::stdout().flush();
    eprintln!("Uncaught {}", msg);
    std::process::exit(1);
}

// =========================================================================
// Exceptions
// =========================================================================
//
// Native code has no unwinding. A function that throws stores the thrown
// value in `PENDING` and returns `EXCEPTION_BITS`; the code that called it
// sees that result and jumps to its `catch`/`finally`, or returns
// `EXCEPTION_BITS` itself. A `catch` takes the value back out of `PENDING`.

thread_local! {
    static PENDING: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
    /// Prototype objects of the error kinds, created on first use.
    static ERROR_PROTOS: RefCell<HashMap<&'static str, u64>> = RefCell::new(HashMap::new());
}

/// The lowest stack address compiled code may reach: every compiled
/// function compares its stack pointer with it on entry and throws a
/// RangeError below it. 0 (no limit) until `ot_stack_init`.
#[unsafe(no_mangle)]
pub static OT_STACK_LIMIT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Stack that recursion in compiled code may use, below where the program
/// starts. Main threads get 8MB by default; the rest is left for the
/// runtime and its callers.
const STACK_BUDGET: usize = 4 << 20;

/// Set `OT_STACK_LIMIT` from the current stack position.
#[unsafe(no_mangle)]
pub extern "C" fn ot_stack_init() {
    let marker = 0u8;
    let here = std::hint::black_box(&marker) as *const u8 as usize;
    OT_STACK_LIMIT.store(
        here.saturating_sub(STACK_BUDGET),
        std::sync::atomic::Ordering::Relaxed,
    );
}

/// A compiled function found the stack exhausted.
#[unsafe(no_mangle)]
pub extern "C" fn ot_stack_overflow() -> u64 {
    throw_error("RangeError", "Maximum call stack size exceeded")
}

/// `throw value`: returns `EXCEPTION_BITS`, which the caller propagates.
#[unsafe(no_mangle)]
pub extern "C" fn ot_throw(value: u64) -> u64 {
    PENDING.with(|p| p.set(value));
    TRACE.with(|t| {
        let mut t = t.borrow_mut();
        if t.value == Some(value) {
            // A rethrow continues the trace; the function rethrowing was
            // recorded when it caught the value.
            t.skip = true;
        } else {
            t.value = Some(value);
            t.frames.clear();
            t.skip = false;
        }
    });
    EXCEPTION_BITS
}

// Stack traces. Compiled code calls `ot_trace` where an exception passes
// an op that threw it: in the function that throws it, at each call it
// unwinds through, and in the function that catches it (the same frames
// as the VM, `vm/trace.rs`).

/// A stack frame as compiled code passes it: `name (file:line:col)`.
#[repr(C)]
pub struct TraceSite {
    pub ptr: *const u8,
    pub len: usize,
}

#[derive(Default)]
struct Trace {
    /// The exception the frames are of.
    value: Option<u64>,
    frames: Vec<&'static str>,
    /// Skip the next frame (a rethrow's own function).
    skip: bool,
}

thread_local! {
    static TRACE: RefCell<Trace> = RefCell::new(Trace::default());
}

/// Frames kept, as in V8 (`Error.stackTraceLimit`).
const TRACE_LIMIT: usize = 10;

/// The exception in flight passed `site` (null: a place with no known
/// position).
#[unsafe(no_mangle)]
pub extern "C" fn ot_trace(site: *const TraceSite) {
    TRACE.with(|t| {
        let mut t = t.borrow_mut();
        if std::mem::take(&mut t.skip) || site.is_null() || t.frames.len() >= TRACE_LIMIT {
            return;
        }
        // Sites are emitted with the code and live as long as it does.
        let frame: &'static str = unsafe {
            let site = &*site;
            std::str::from_utf8_unchecked(std::slice::from_raw_parts(site.ptr, site.len))
        };
        t.frames.push(frame);
    });
}

/// Print the frames recorded for `value`, if it is the traced exception.
fn print_trace(value: u64) {
    TRACE.with(|t| {
        let t = t.borrow();
        if t.value == Some(value) {
            for frame in &t.frames {
                eprintln!("    at {}", frame);
            }
        }
    });
}

/// The exception in flight, for a `catch` (or a `finally` that will
/// rethrow it). Clears it.
#[unsafe(no_mangle)]
pub extern "C" fn ot_take_exception() -> u64 {
    PENDING.with(|p| p.replace(undefined()))
}

/// End of a `finally`: rethrow `value` if the `finally` was entered because
/// of an exception (`thrown` is true); otherwise continue (`undefined`).
#[unsafe(no_mangle)]
pub extern "C" fn ot_rethrow(thrown: u64, value: u64) -> u64 {
    if ot_truthy(thrown) != 0 {
        ot_throw(value)
    } else {
        undefined()
    }
}

/// An exception reached the top of the program: report it and exit 1.
#[unsafe(no_mangle)]
pub extern "C" fn ot_report_uncaught() {
    let bits = ot_take_exception();
    let exception = OtValue::from_bits(bits);
    let text = match as_str(exception) {
        Some(s) => s.to_string(),
        None => inspect(exception),
    };
    let _ = std::io::stdout().flush();
    eprintln!("Uncaught {}", text);
    print_trace(bits);
    std::process::exit(1);
}

/// The prototype for errors of `kind`: the `prototype` of the class of
/// that name (std/core/error.ot, compiled into every native program), or
/// `{ name: kind }` marked as an error without it.
fn error_proto(kind: &'static str) -> u64 {
    if let Some(class) = GLOBALS.with(|g| g.borrow().get(kind).copied())
        && let Some((ptr, ObjectKind::Object)) = object(OtValue::from_bits(class))
        && let Some(proto) = lookup(ptr, "prototype")
    {
        return proto;
    }
    ERROR_PROTOS.with(|protos| {
        *protos.borrow_mut().entry(kind).or_insert_with(|| {
            let proto = ot_alloc_object();
            let p = OtValue::from_bits(proto);
            set_property(p, "name", new_string(kind));
            set_property(p, "__error__", boolean(true));
            proto
        })
    })
}

/// Throw a new `kind` error ("TypeError", ...) with `message`, as the VM's
/// error classes do: `e.name`, `e.message`, printed as `TypeError: message`.
fn throw_error(kind: &'static str, message: &str) -> u64 {
    let error = ot_alloc_object();
    let e = OtValue::from_bits(error);
    set_property(e, "message", new_string(message));
    set_property(e, "__proto__", error_proto(kind));
    ot_throw(error)
}

/// Whether `ptr` is an error object: `Error.prototype` is on its
/// prototype chain (or, without the error classes, the chain is marked).
fn is_error(ptr: HeapPtr) -> bool {
    let error_proto = GLOBALS
        .with(|g| g.borrow().get("Error").copied())
        .and_then(|c| object(OtValue::from_bits(c)))
        .and_then(|(c, _)| lookup(c, "prototype"));
    if let Some(target) = error_proto {
        let mut cur = ptr;
        for _ in 0..1000 {
            match props(cur).iter().find(|(k, _)| k == "__proto__") {
                Some((_, p)) if *p == target => return true,
                Some((_, p)) => match object(OtValue::from_bits(*p)) {
                    Some((next, ObjectKind::Object)) => cur = next,
                    _ => return false,
                },
                None => return false,
            }
        }
        return false;
    }
    lookup(ptr, "__error__") == Some(boolean(true))
}

/// `TypeError: message`, as `Error.prototype.toString` gives it.
fn error_summary(ptr: HeapPtr) -> String {
    let v = OtValue::pointer(ptr);
    let name = value_to_string(OtValue::from_bits(get_property(v, "name")));
    let message = match OtValue::from_bits(get_property(v, "message")) {
        m if m.is_undefined() => String::new(),
        m => value_to_string(m),
    };
    match (name.is_empty(), message.is_empty()) {
        (_, true) => name,
        (true, false) => message,
        (false, false) => format!("{name}: {message}"),
    }
}

/// The heap object behind a value, with its kind.
fn object(v: OtValue) -> Option<(HeapPtr, ObjectKind)> {
    let ptr = v.as_pointer()?;
    let kind = unsafe { ptr.as_ref::<ObjectHeader>().kind };
    Some((ptr, kind))
}

/// The string behind a value, if it is a string.
fn as_str(v: OtValue) -> Option<&'static str> {
    match object(v)? {
        (ptr, ObjectKind::String) => Some(unsafe { ptr.as_ref::<NativeString>().as_str() }),
        _ => None,
    }
}

fn new_string(s: &str) -> u64 {
    match heap().alloc_string(s) {
        Some(ptr) => OtValue::pointer(ptr).to_bits(),
        None => undefined(),
    }
}

/// A name passed as pointer + length.
unsafe fn name<'a>(ptr: *const u8, len: usize) -> &'a str {
    unsafe { std::str::from_utf8_unchecked(std::slice::from_raw_parts(ptr, len)) }
}

/// The arguments passed to a native function.
unsafe fn args<'a>(argv: *const u64, argc: usize) -> &'a [u64] {
    if argv.is_null() || argc == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(argv, argc) }
    }
}

/// JavaScript truthiness.
fn truthy(v: OtValue) -> bool {
    if v.is_falsy() {
        return false;
    }
    !matches!(as_str(v), Some(""))
}

/// JavaScript `ToNumber`, for primitives.
fn to_number(v: OtValue) -> f64 {
    if v.is_number() {
        return v.as_number_unchecked();
    }
    if v.is_boolean() {
        return if v.as_boolean_unchecked() { 1.0 } else { 0.0 };
    }
    if v.is_null() {
        return 0.0;
    }
    if let Some(s) = as_str(v) {
        let t = s.trim();
        if t.is_empty() {
            return 0.0;
        }
        return t.parse::<f64>().unwrap_or(f64::NAN);
    }
    f64::NAN
}

/// JavaScript `ToInt32`.
fn to_int32(v: OtValue) -> i32 {
    let n = to_number(v);
    if !n.is_finite() {
        return 0;
    }
    (n.trunc() as i64 as u64 as u32) as i32
}

/// Number to string, as the language prints numbers.
pub fn number_to_string(n: f64) -> String {
    builtins::number_to_string(n)
}

/// `String(v)`: the conversion used by `+` with a string operand.
fn value_to_string(v: OtValue) -> String {
    if v.is_number() {
        return number_to_string(v.as_number_unchecked());
    }
    if v.is_boolean() {
        return v.as_boolean_unchecked().to_string();
    }
    if v.is_null() {
        return "null".to_string();
    }
    if v.is_undefined() {
        return "undefined".to_string();
    }
    match object(v) {
        Some((ptr, ObjectKind::String)) => unsafe { ptr.as_ref::<NativeString>().as_str() }.into(),
        Some((ptr, ObjectKind::Array)) => {
            let arr = unsafe { ptr.as_ref::<NativeArray>() };
            (0..arr.len as usize)
                .map(|i| {
                    let e = OtValue::from_bits(unsafe { *arr.elements.add(i) });
                    if e.is_null() || e.is_undefined() {
                        String::new()
                    } else {
                        value_to_string(e)
                    }
                })
                .collect::<Vec<_>>()
                .join(",")
        }
        Some((_, ObjectKind::Function)) => "function".to_string(),
        Some((ptr, ObjectKind::Object)) if is_error(ptr) => error_summary(ptr),
        _ => "[object Object]".to_string(),
    }
}

fn props(ptr: HeapPtr) -> &'static mut PropertyMap {
    let obj = unsafe { ptr.as_mut::<NativeObject>() };
    if obj.properties.is_null() {
        obj.properties = Box::into_raw(Box::new(PropertyMap::new()));
    }
    unsafe { &mut *obj.properties }
}

fn array(ptr: HeapPtr) -> &'static mut NativeArray {
    unsafe { ptr.as_mut::<NativeArray>() }
}

/// Make room for at least `needed` elements.
fn array_reserve(arr: &mut NativeArray, needed: usize) {
    use std::alloc::{Layout, realloc};
    if needed <= arr.capacity as usize {
        return;
    }
    let new_cap = needed.max(arr.capacity as usize * 2).max(4);
    let old = Layout::array::<u64>(arr.capacity as usize).unwrap();
    let new_size = Layout::array::<u64>(new_cap).unwrap().size();
    let p = unsafe { realloc(arr.elements as *mut u8, old, new_size) } as *mut u64;
    if p.is_null() {
        fatal_error("RangeError: out of memory growing an array");
    }
    arr.elements = p;
    arr.capacity = new_cap as u32;
}

fn array_set(arr: &mut NativeArray, idx: usize, value: u64) {
    array_reserve(arr, idx + 1);
    for i in arr.len as usize..idx {
        unsafe { *arr.elements.add(i) = undefined() };
    }
    unsafe { *arr.elements.add(idx) = value };
    if idx >= arr.len as usize {
        arr.len = (idx + 1) as u32;
    }
}

fn array_push(arr: &mut NativeArray, value: u64) -> usize {
    let len = arr.len as usize;
    array_set(arr, len, value);
    len + 1
}

thread_local! {
    /// Whether a getter or setter was ever defined: until then property
    /// access skips looking for them.
    static HAS_ACCESSORS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// `key` on the object at `ptr` or its prototype chain, without accessors.
fn lookup(ptr: HeapPtr, key: &str) -> Option<u64> {
    let mut cur = ptr;
    for _ in 0..1000 {
        let map = props(cur);
        if let Some((_, bits)) = map.iter().find(|(k, _)| k == key) {
            return Some(*bits);
        }
        let (_, proto) = map.iter().find(|(k, _)| k == "__proto__")?;
        match object(OtValue::from_bits(*proto)) {
            Some((p, ObjectKind::Object)) => cur = p,
            _ => return None,
        }
    }
    None
}

/// Whether the object at `ptr` is a class (marked `__class__`).
fn is_class(ptr: HeapPtr) -> bool {
    props(ptr)
        .iter()
        .any(|(k, v)| k == "__class__" && *v == boolean(true))
}

/// `[class C]` or `[class C extends P]`, as Node prints a class.
fn class_summary(ptr: HeapPtr) -> String {
    let name_of = |p: HeapPtr| match lookup(p, "name").and_then(|n| as_str(OtValue::from_bits(n))) {
        Some(n) => n.to_string(),
        None => "(anonymous)".to_string(),
    };
    let parent = props(ptr)
        .iter()
        .find(|(k, _)| k == "__proto__")
        .and_then(|(_, p)| object(OtValue::from_bits(*p)))
        .filter(|(p, kind)| *kind == ObjectKind::Object && is_class(*p));
    match parent {
        Some((p, _)) => format!("[class {} extends {}]", name_of(ptr), name_of(p)),
        None => format!("[class {}]", name_of(ptr)),
    }
}

/// The result of `new`: what the constructor returned if it is an object,
/// otherwise `this`.
fn construct_result(result: u64, this: u64) -> u64 {
    if result == EXCEPTION_BITS {
        return result;
    }
    match object(OtValue::from_bits(result)) {
        Some((_, kind)) if kind != ObjectKind::String => result,
        _ => this,
    }
}

/// Run `ctor` (a class, or a plain function) as a constructor on `this`.
fn run_constructor(ctor: u64, this: u64, argv: *const u64, argc: usize) -> Option<u64> {
    let c = OtValue::from_bits(ctor);
    let code = match object(c)? {
        (ptr, ObjectKind::Object) if is_class(ptr) => lookup(ptr, "constructor")?,
        (_, ObjectKind::Function) => ctor,
        _ => return None,
    };
    Some(construct_result(ot_call(code, this, argv, argc), this))
}

/// `new ctor(...args)`
#[unsafe(no_mangle)]
pub extern "C" fn ot_construct(ctor: u64, argv: *const u64, argc: usize) -> u64 {
    if let Some((ptr, ObjectKind::Object)) = object(OtValue::from_bits(ctor))
        && lookup(ptr, "__promise_ctor__").is_some()
    {
        let executor = unsafe { args(argv, argc) }
            .first()
            .copied()
            .unwrap_or_else(undefined);
        return construct_promise(executor);
    }
    let this = ot_alloc_object();
    if let Some((ptr, ObjectKind::Object)) = object(OtValue::from_bits(ctor))
        && let Some(proto) = lookup(ptr, "prototype")
    {
        set_property(OtValue::from_bits(this), "__proto__", proto);
    }
    match run_constructor(ctor, this, argv, argc) {
        Some(r) => r,
        None => throw_error(
            "TypeError",
            &format!("{} is not a constructor", inspect(OtValue::from_bits(ctor))),
        ),
    }
}

/// `super(...args)`: the parent class's constructor on the current `this`.
#[unsafe(no_mangle)]
pub extern "C" fn ot_call_super(parent: u64, this: u64, argv: *const u64, argc: usize) -> u64 {
    match run_constructor(parent, this, argv, argc) {
        Some(r) => r,
        None => throw_error("TypeError", "Super constructor is not a constructor"),
    }
}

/// `super.key`: looked up from `base`; a getter runs with `this`.
#[unsafe(no_mangle)]
pub extern "C" fn ot_get_super_prop(this: u64, base: u64, key: *const u8, key_len: usize) -> u64 {
    let key = unsafe { name(key, key_len) };
    let Some((ptr, ObjectKind::Object)) = object(OtValue::from_bits(base)) else {
        return undefined();
    };
    match lookup(ptr, &format!("getter:{key}")) {
        Some(g)
            if matches!(
                object(OtValue::from_bits(g)),
                Some((_, ObjectKind::Function))
            ) =>
        {
            ot_call(g, this, std::ptr::null(), 0)
        }
        _ => lookup(ptr, key).unwrap_or_else(undefined),
    }
}

/// `#x` from a private member's key (`#x@<class>`), for messages.
fn private_name(key: &str) -> &str {
    key.split('@').next().unwrap_or(key)
}

/// `obj.#x`: a TypeError if the object lacks the private member.
#[unsafe(no_mangle)]
pub extern "C" fn ot_get_private(obj: u64, key: *const u8, key_len: usize) -> u64 {
    let key = unsafe { name(key, key_len) };
    if let Some((ptr, ObjectKind::Object)) = object(OtValue::from_bits(obj)) {
        if let Some(v) = lookup(ptr, key) {
            return v;
        }
        if let Some(getter) = lookup(ptr, &format!("getter:{key}")) {
            return ot_call(getter, obj, std::ptr::null(), 0);
        }
    }
    throw_error(
        "TypeError",
        &format!(
            "Cannot read private member {} from an object whose class did not declare it",
            private_name(key)
        ),
    )
}

/// `obj.#x = value`: a TypeError if the object lacks the private field.
#[unsafe(no_mangle)]
pub extern "C" fn ot_set_private(obj: u64, key: *const u8, key_len: usize, value: u64) -> u64 {
    let key = unsafe { name(key, key_len) };
    if let Some((ptr, ObjectKind::Object)) = object(OtValue::from_bits(obj)) {
        if let Some(entry) = props(ptr).iter_mut().find(|(k, _)| k == key) {
            entry.1 = value;
            return undefined();
        }
        if let Some(setter) = lookup(ptr, &format!("setter:{key}")) {
            let r = ot_call(setter, obj, &value, 1);
            return if r == EXCEPTION_BITS { r } else { undefined() };
        }
    }
    throw_error(
        "TypeError",
        &format!(
            "Cannot write private member {} to an object whose class did not declare it",
            private_name(key)
        ),
    )
}

/// `key in obj`: the object or its prototype chain has the property (a
/// getter or setter counts).
#[unsafe(no_mangle)]
pub extern "C" fn ot_in(key: u64, obj: u64) -> u64 {
    let key = value_to_string(OtValue::from_bits(key));
    match object(OtValue::from_bits(obj)) {
        Some((ptr, ObjectKind::Object)) => boolean(
            lookup(ptr, &key).is_some()
                || lookup(ptr, &format!("getter:{key}")).is_some()
                || lookup(ptr, &format!("setter:{key}")).is_some(),
        ),
        Some((ptr, ObjectKind::Array)) => boolean(
            key == "length"
                || key
                    .parse::<usize>()
                    .is_ok_and(|i| i < array(ptr).len as usize),
        ),
        _ => throw_error(
            "TypeError",
            &format!(
                "Cannot use 'in' operator to search for '{}' in {}",
                key,
                inspect(OtValue::from_bits(obj))
            ),
        ),
    }
}

/// `value instanceof ctor`: whether `ctor.prototype` is on `value`'s
/// prototype chain. (A plain function counts as matching, as on the VM.)
#[unsafe(no_mangle)]
pub extern "C" fn ot_instanceof(value: u64, ctor: u64) -> u64 {
    // `x instanceof Object` holds for every object.
    if GLOBALS.with(|g| g.borrow().get("Object").copied()) == Some(ctor) {
        return boolean(
            matches!(object(OtValue::from_bits(value)), Some((_, kind)) if kind != ObjectKind::String),
        );
    }
    let proto = match object(OtValue::from_bits(ctor)) {
        Some((_, ObjectKind::Function)) => return boolean(true),
        Some((ptr, ObjectKind::Object)) => lookup(ptr, "prototype"),
        _ => None,
    };
    let Some(proto) = proto else {
        return boolean(false);
    };
    let mut cur = OtValue::from_bits(value);
    for _ in 0..1000 {
        let Some((ptr, ObjectKind::Object)) = object(cur) else {
            return boolean(false);
        };
        match props(ptr).iter().find(|(k, _)| k == "__proto__") {
            Some((_, p)) if *p == proto => return boolean(true),
            Some((_, p)) => cur = OtValue::from_bits(*p),
            None => return boolean(false),
        }
    }
    boolean(false)
}

/// Property lookup, following `__proto__`.
fn get_property(v: OtValue, key: &str) -> u64 {
    let Some((ptr, kind)) = object(v) else {
        if v.is_null() || v.is_undefined() {
            return throw_error(
                "TypeError",
                &format!(
                    "Cannot read properties of {} (reading '{}')",
                    value_to_string(v),
                    key
                ),
            );
        }
        return undefined();
    };
    match kind {
        ObjectKind::Object => {
            if matches!(key, "then" | "catch" | "finally") && promise_id(v.to_bits()).is_some() {
                // Promise methods run through `ot_call_method`; a function
                // so that `typeof p.then` is "function".
                return builtin(ot_builtin_promise_static);
            }
            if HAS_ACCESSORS.with(|a| a.get())
                && let Some(getter) = lookup(ptr, &format!("getter:{key}"))
                && matches!(
                    object(OtValue::from_bits(getter)),
                    Some((_, ObjectKind::Function))
                )
            {
                return ot_call(getter, v.to_bits(), std::ptr::null(), 0);
            }
            let mut cur = ptr;
            for _ in 0..1000 {
                let map = props(cur);
                if let Some((_, bits)) = map.iter().find(|(k, _)| k == key) {
                    return *bits;
                }
                match map.iter().find(|(k, _)| k == "__proto__") {
                    Some((_, proto)) => match OtValue::from_bits(*proto).as_pointer() {
                        Some(p) => cur = p,
                        None => break,
                    },
                    None => break,
                }
            }
            undefined()
        }
        ObjectKind::Array => {
            let arr = array(ptr);
            if key == "length" {
                return number(arr.len as f64);
            }
            match key.parse::<usize>() {
                Ok(i) if i < arr.len as usize => unsafe { *arr.elements.add(i) },
                _ => undefined(),
            }
        }
        ObjectKind::String => {
            let s = unsafe { ptr.as_ref::<NativeString>().as_str() };
            if key == "length" {
                // Byte length, matching the VM until the string model is
                // decided (#133).
                return number(s.len() as f64);
            }
            match key.parse::<usize>() {
                Ok(i) if i < s.len() => new_string(&s[i..i + 1]),
                _ => undefined(),
            }
        }
        _ => undefined(),
    }
}

/// `v.key = value`; `undefined`, or `EXCEPTION_BITS` if it throws.
fn set_property(v: OtValue, key: &str, value: u64) -> u64 {
    match object(v) {
        Some((ptr, ObjectKind::Object)) => {
            if key.starts_with("getter:") || key.starts_with("setter:") {
                HAS_ACCESSORS.with(|a| a.set(true));
            } else if HAS_ACCESSORS.with(|a| a.get())
                && let Some(setter) = lookup(ptr, &format!("setter:{key}"))
                && matches!(
                    object(OtValue::from_bits(setter)),
                    Some((_, ObjectKind::Function))
                )
            {
                let r = ot_call(setter, v.to_bits(), &value, 1);
                return if r == EXCEPTION_BITS { r } else { undefined() };
            }
            let map = props(ptr);
            if let Some(entry) = map.iter_mut().find(|(k, _)| k == key) {
                entry.1 = value;
            } else {
                map.push((key.to_string(), value));
            }
        }
        Some((ptr, ObjectKind::Array)) => {
            let arr = array(ptr);
            if key == "length" {
                let n = to_number(OtValue::from_bits(value));
                if n >= 0.0 && n.fract() == 0.0 && n <= u32::MAX as f64 {
                    let n = n as usize;
                    if n < arr.len as usize {
                        arr.len = n as u32;
                    } else {
                        for i in arr.len as usize..n {
                            array_set(arr, i, undefined());
                        }
                    }
                }
            } else if let Ok(i) = key.parse::<usize>() {
                array_set(arr, i, value);
            }
        }
        Some(_) => {}
        None => {
            if v.is_null() || v.is_undefined() {
                return throw_error(
                    "TypeError",
                    &format!(
                        "Cannot set properties of {} (setting '{}')",
                        value_to_string(v),
                        key
                    ),
                );
            }
        }
    }
    undefined()
}

/// An index or key value as a property name.
fn key_string(key: OtValue) -> String {
    value_to_string(key)
}

/// A number key usable as an array index.
fn index_of(key: OtValue) -> Option<usize> {
    let n = key.as_number()?;
    if n >= 0.0 && n.fract() == 0.0 && n < u32::MAX as f64 {
        Some(n as usize)
    } else {
        None
    }
}

// =========================================================================
// Allocation
// =========================================================================

/// Allocate a new empty object.
#[unsafe(no_mangle)]
pub extern "C" fn ot_alloc_object() -> u64 {
    match heap().alloc_object() {
        Some(ptr) => OtValue::pointer(ptr).to_bits(),
        None => undefined(),
    }
}

/// Allocate a new empty array with room for `capacity` elements.
#[unsafe(no_mangle)]
pub extern "C" fn ot_alloc_array(capacity: usize) -> u64 {
    match heap().alloc_array(capacity) {
        Some(ptr) => OtValue::pointer(ptr).to_bits(),
        None => undefined(),
    }
}

/// Allocate a new string from UTF-8 bytes.
#[unsafe(no_mangle)]
pub extern "C" fn ot_alloc_string(data: *const u8, len: usize) -> u64 {
    if data.is_null() {
        return new_string("");
    }
    let bytes = unsafe { std::slice::from_raw_parts(data, len) };
    match std::str::from_utf8(bytes) {
        Ok(s) => new_string(s),
        Err(_) => undefined(),
    }
}

/// Wrap compiled code (with the `NativeFn` signature) as a function value.
#[unsafe(no_mangle)]
pub extern "C" fn ot_make_function(code: *const u8) -> u64 {
    ot_make_closure(code, undefined())
}

/// A closure: compiled code plus the environment object it captured.
#[unsafe(no_mangle)]
pub extern "C" fn ot_make_closure(code: *const u8, env: u64) -> u64 {
    match heap().alloc_function(code, env) {
        Some(ptr) => OtValue::pointer(ptr).to_bits(),
        None => undefined(),
    }
}

// =========================================================================
// Properties and elements
// =========================================================================

/// `obj.key`
#[unsafe(no_mangle)]
pub extern "C" fn ot_get_prop(obj: u64, key: *const u8, key_len: usize) -> u64 {
    get_property(OtValue::from_bits(obj), unsafe { name(key, key_len) })
}

/// `obj.key = value`
#[unsafe(no_mangle)]
pub extern "C" fn ot_set_prop(obj: u64, key: *const u8, key_len: usize, value: u64) -> u64 {
    set_property(
        OtValue::from_bits(obj),
        unsafe { name(key, key_len) },
        value,
    )
}

/// `obj[key]`
#[unsafe(no_mangle)]
pub extern "C" fn ot_get_index(obj: u64, key: u64) -> u64 {
    let o = OtValue::from_bits(obj);
    let k = OtValue::from_bits(key);
    if let (Some((ptr, ObjectKind::Array)), Some(i)) = (object(o), index_of(k)) {
        let arr = array(ptr);
        return if i < arr.len as usize {
            unsafe { *arr.elements.add(i) }
        } else {
            undefined()
        };
    }
    get_property(o, &key_string(k))
}

/// `obj[key] = value`
#[unsafe(no_mangle)]
pub extern "C" fn ot_set_index(obj: u64, key: u64, value: u64) -> u64 {
    let o = OtValue::from_bits(obj);
    let k = OtValue::from_bits(key);
    if let (Some((ptr, ObjectKind::Array)), Some(i)) = (object(o), index_of(k)) {
        array_set(array(ptr), i, value);
        return undefined();
    }
    set_property(o, &key_string(k), value)
}

/// `delete obj.key`
#[unsafe(no_mangle)]
pub extern "C" fn ot_delete_prop(obj: u64, key: *const u8, key_len: usize) -> u64 {
    if let Some((ptr, ObjectKind::Object)) = object(OtValue::from_bits(obj)) {
        let key = unsafe { name(key, key_len) };
        props(ptr).retain(|(k, _)| k != key);
    }
    boolean(true)
}

/// `arr.length` (or `.length` of anything else).
#[unsafe(no_mangle)]
pub extern "C" fn ot_array_len(arr: u64) -> u64 {
    get_property(OtValue::from_bits(arr), "length")
}

/// `arr.push(value)`; returns the new length.
#[unsafe(no_mangle)]
pub extern "C" fn ot_array_push(arr: u64, value: u64) -> u64 {
    match object(OtValue::from_bits(arr)) {
        Some((ptr, ObjectKind::Array)) => number(array_push(array(ptr), value) as f64),
        _ => throw_error("TypeError", "push called on a non-array"),
    }
}

// =========================================================================
// Globals
// =========================================================================

thread_local! {
    static GLOBALS: RefCell<HashMap<String, u64>> = RefCell::new(initial_globals());
}

fn builtin(code: NativeFn) -> u64 {
    ot_make_function(code as *const u8)
}

fn initial_globals() -> HashMap<String, u64> {
    let mut g = HashMap::new();
    let console = ot_alloc_object();
    let c = OtValue::from_bits(console);
    set_property(c, "log", builtin(ot_builtin_console_log));
    set_property(c, "error", builtin(ot_builtin_console_error));
    g.insert("console".to_string(), console);
    let object = ot_alloc_object();
    set_property(
        OtValue::from_bits(object),
        "keys",
        builtin(ot_builtin_object_keys),
    );
    g.insert("Object".to_string(), object);
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
    g.insert("String".to_string(), builtin(ot_builtin_string));
    let promise = ot_alloc_object();
    let pr = OtValue::from_bits(promise);
    for (i, name) in ["resolve", "reject", "all", "race"].iter().enumerate() {
        let f = ot_make_closure(ot_builtin_promise_static as *const u8, number(i as f64));
        set_property(pr, name, f);
    }
    set_property(pr, "__promise_ctor__", boolean(true));
    g.insert("Promise".to_string(), promise);
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
    match GLOBALS.with(|g| g.borrow().get(key).copied()) {
        Some(v) => v,
        None => throw_error("ReferenceError", &format!("{key} is not defined")),
    }
}

/// `typeof name` of a global: "undefined" if it was never set.
#[unsafe(no_mangle)]
pub extern "C" fn ot_typeof_global(key: *const u8, key_len: usize) -> u64 {
    let key = unsafe { name(key, key_len) };
    match GLOBALS.with(|g| g.borrow().get(key).copied()) {
        Some(v) => ot_typeof(v),
        None => new_string("undefined"),
    }
}

/// Write a global variable.
#[unsafe(no_mangle)]
pub extern "C" fn ot_set_global(key: *const u8, key_len: usize, value: u64) -> u64 {
    let key = unsafe { name(key, key_len) };
    GLOBALS.with(|g| {
        g.borrow_mut().insert(key.to_string(), value);
    });
    undefined()
}

// =========================================================================
// Calls
// =========================================================================

/// Call a function value: `f(...args)` with the given `this`.
#[unsafe(no_mangle)]
pub extern "C" fn ot_call(func: u64, this: u64, argv: *const u64, argc: usize) -> u64 {
    match object(OtValue::from_bits(func)) {
        Some((ptr, ObjectKind::Function)) => {
            let func = unsafe { ptr.as_ref::<NativeFunction>() };
            let f: NativeFn = unsafe { std::mem::transmute(func.code) };
            f(this, argv, argc, func.env)
        }
        _ => throw_error(
            "TypeError",
            &format!("{} is not a function", inspect(OtValue::from_bits(func))),
        ),
    }
}

/// `recv.name(...args)`: looks up the method and calls it with `this = recv`.
#[unsafe(no_mangle)]
pub extern "C" fn ot_call_method(
    recv: u64,
    key: *const u8,
    key_len: usize,
    argv: *const u64,
    argc: usize,
) -> u64 {
    let r = OtValue::from_bits(recv);
    let key = unsafe { name(key, key_len) };

    // Built-in array methods.
    if let Some((ptr, ObjectKind::Array)) = object(r) {
        let arr = array(ptr);
        let a = unsafe { args(argv, argc) };
        match key {
            "push" => {
                let mut len = arr.len as usize;
                for &v in a {
                    len = array_push(arr, v);
                }
                return number(len as f64);
            }
            "pop" => {
                if arr.len == 0 {
                    return undefined();
                }
                arr.len -= 1;
                return unsafe { *arr.elements.add(arr.len as usize) };
            }
            _ => {}
        }
        // Every other array method is shared with the VM. It works on a
        // copy of the elements, which is written back afterwards.
        let mut elems: Vec<u64> = (0..arr.len as usize)
            .map(|i| unsafe { *arr.elements.add(i) })
            .collect();
        if let Some(result) = builtins::array_method(&NativeHost, &mut elems, key, a) {
            array_reserve(arr, elems.len());
            for (i, v) in elems.iter().enumerate() {
                unsafe { *arr.elements.add(i) = *v };
            }
            arr.len = elems.len() as u32;
            return outcome(result, recv);
        }
    }

    // Promise methods.
    if let Some(id) = promise_id(recv)
        && let Some(result) = promise_method(id, key, unsafe { args(argv, argc) })
    {
        return result;
    }

    // `f.call(thisArg, ...args)`, `f.apply(thisArg, array)`
    if let Some((_, ObjectKind::Function)) = object(r)
        && (key == "call" || key == "apply")
    {
        let a = unsafe { args(argv, argc) };
        let this = a.first().copied().unwrap_or_else(undefined);
        if key == "call" {
            let rest = if a.is_empty() { a } else { &a[1..] };
            return ot_call(recv, this, rest.as_ptr(), rest.len());
        }
        let list: Vec<u64> = match a.get(1).and_then(|v| object(OtValue::from_bits(*v))) {
            Some((ptr, ObjectKind::Array)) => {
                let arr = array(ptr);
                (0..arr.len as usize)
                    .map(|i| unsafe { *arr.elements.add(i) })
                    .collect()
            }
            _ => Vec::new(),
        };
        return ot_call(recv, this, list.as_ptr(), list.len());
    }

    // Number methods, shared with the VM.
    if r.is_number() {
        let a = unsafe { args(argv, argc) };
        if let Some(result) = builtins::number_method(&NativeHost, r.as_number_unchecked(), key, a)
        {
            return outcome(result, recv);
        }
    }

    // String methods, shared with the VM.
    if let Some(s) = as_str(r) {
        let a = unsafe { args(argv, argc) };
        if let Some(result) = builtins::string_method(&NativeHost, s, key, a) {
            return outcome(result, recv);
        }
    }

    let method = get_property(r, key);
    if method == EXCEPTION_BITS {
        return method;
    }
    if let Some((_, ObjectKind::Function)) = object(OtValue::from_bits(method)) {
        return ot_call(method, recv, argv, argc);
    }
    throw_error(
        "TypeError",
        &format!("{}.{} is not a function", inspect(r), key),
    )
}

/// The native value representation, for the shared built-in methods.
struct NativeHost;

impl builtins::Host for NativeHost {
    type Value = u64;

    fn number(&self, v: &u64) -> Option<f64> {
        OtValue::from_bits(*v).as_number()
    }

    fn string<'a>(&'a self, v: &'a u64) -> Option<&'a str> {
        as_str(OtValue::from_bits(*v))
    }

    fn to_string(&self, v: &u64) -> String {
        value_to_string(OtValue::from_bits(*v))
    }

    fn strict_equal(&self, a: &u64, b: &u64) -> bool {
        strict_equal(OtValue::from_bits(*a), OtValue::from_bits(*b))
    }

    fn array_elements(&self, v: &u64) -> Option<Vec<u64>> {
        match object(OtValue::from_bits(*v)) {
            Some((ptr, ObjectKind::Array)) => {
                let arr = array(ptr);
                Some(
                    (0..arr.len as usize)
                        .map(|i| unsafe { *arr.elements.add(i) })
                        .collect(),
                )
            }
            _ => None,
        }
    }

    fn make_number(&self, n: f64) -> u64 {
        number(n)
    }

    fn make_string(&self, s: String) -> u64 {
        new_string(&s)
    }

    fn make_bool(&self, b: bool) -> u64 {
        boolean(b)
    }

    fn undefined(&self) -> u64 {
        undefined()
    }
}

/// Turn a built-in method's result into a value.
fn outcome(result: Result<builtins::Outcome<u64>, builtins::Error>, recv: u64) -> u64 {
    match result {
        Ok(builtins::Outcome::Value(v)) => v,
        Ok(builtins::Outcome::This) => recv,
        Ok(builtins::Outcome::NewArray(elems)) => {
            let out = ot_alloc_array(elems.len());
            if let Some((ptr, _)) = object(OtValue::from_bits(out)) {
                let arr = array(ptr);
                for v in elems {
                    array_push(arr, v);
                }
            }
            out
        }
        Err(e) => throw_error(e.kind, &e.message),
    }
}

// =========================================================================
// Operators
// =========================================================================

/// `a + b`
#[unsafe(no_mangle)]
pub extern "C" fn ot_add_any(a: u64, b: u64) -> u64 {
    let mut va = OtValue::from_bits(a);
    let mut vb = OtValue::from_bits(b);
    if va.is_number() && vb.is_number() {
        return number(va.as_number_unchecked() + vb.as_number_unchecked());
    }
    // An object with its own `valueOf` / `toString` converts through it.
    for v in [&mut va, &mut vb] {
        match to_primitive(*v, false) {
            Ok(Some(p)) => *v = p,
            Ok(None) => {}
            Err(e) => return e,
        }
    }
    let a_str =
        as_str(va).is_some() || matches!(object(va), Some((_, k)) if k != ObjectKind::String);
    let b_str =
        as_str(vb).is_some() || matches!(object(vb), Some((_, k)) if k != ObjectKind::String);
    if a_str || b_str {
        let mut s = value_to_string(va);
        s.push_str(&value_to_string(vb));
        return new_string(&s);
    }
    number(to_number(va) + to_number(vb))
}

fn numeric(a: u64, b: u64, f: impl Fn(f64, f64) -> f64) -> u64 {
    number(f(
        to_number(OtValue::from_bits(a)),
        to_number(OtValue::from_bits(b)),
    ))
}

#[unsafe(no_mangle)]
pub extern "C" fn ot_sub_any(a: u64, b: u64) -> u64 {
    numeric(a, b, |x, y| x - y)
}

#[unsafe(no_mangle)]
pub extern "C" fn ot_mul_any(a: u64, b: u64) -> u64 {
    numeric(a, b, |x, y| x * y)
}

#[unsafe(no_mangle)]
pub extern "C" fn ot_div_any(a: u64, b: u64) -> u64 {
    numeric(a, b, |x, y| x / y)
}

#[unsafe(no_mangle)]
pub extern "C" fn ot_mod_any(a: u64, b: u64) -> u64 {
    numeric(a, b, |x, y| x % y)
}

/// `a ** b`
#[unsafe(no_mangle)]
pub extern "C" fn ot_pow(a: u64, b: u64) -> u64 {
    numeric(a, b, f64::powf)
}

/// `-a`
#[unsafe(no_mangle)]
pub extern "C" fn ot_neg(a: u64) -> u64 {
    number(-to_number(OtValue::from_bits(a)))
}

fn int_op(a: u64, b: u64, f: impl Fn(i32, i32) -> i32) -> u64 {
    number(f(
        to_int32(OtValue::from_bits(a)),
        to_int32(OtValue::from_bits(b)),
    ) as f64)
}

#[unsafe(no_mangle)]
pub extern "C" fn ot_bit_and(a: u64, b: u64) -> u64 {
    int_op(a, b, |x, y| x & y)
}

#[unsafe(no_mangle)]
pub extern "C" fn ot_bit_or(a: u64, b: u64) -> u64 {
    int_op(a, b, |x, y| x | y)
}

#[unsafe(no_mangle)]
pub extern "C" fn ot_bit_xor(a: u64, b: u64) -> u64 {
    int_op(a, b, |x, y| x ^ y)
}

#[unsafe(no_mangle)]
pub extern "C" fn ot_shl(a: u64, b: u64) -> u64 {
    int_op(a, b, |x, y| x.wrapping_shl((y & 31) as u32))
}

#[unsafe(no_mangle)]
pub extern "C" fn ot_shr(a: u64, b: u64) -> u64 {
    int_op(a, b, |x, y| x.wrapping_shr((y & 31) as u32))
}

/// `a >>> b`
#[unsafe(no_mangle)]
pub extern "C" fn ot_shr_u(a: u64, b: u64) -> u64 {
    let x = to_int32(OtValue::from_bits(a)) as u32;
    let y = (to_int32(OtValue::from_bits(b)) & 31) as u32;
    number(x.wrapping_shr(y) as f64)
}

/// Strict equality as a raw `bool`.
fn strict_equal(va: OtValue, vb: OtValue) -> bool {
    if va.is_number() && vb.is_number() {
        return va.as_number_unchecked() == vb.as_number_unchecked();
    }
    if let (Some(x), Some(y)) = (as_str(va), as_str(vb)) {
        return x == y;
    }
    va.to_bits() == vb.to_bits()
}

/// Loose equality (`==`) as a raw `bool`.
fn loose_equal(va: OtValue, vb: OtValue) -> bool {
    let nullish = |v: OtValue| v.is_null() || v.is_undefined();
    if nullish(va) || nullish(vb) {
        return nullish(va) && nullish(vb);
    }
    let primitive = |v: OtValue| v.is_number() || v.is_boolean() || as_str(v).is_some();
    if primitive(va) && primitive(vb) {
        if let (Some(x), Some(y)) = (as_str(va), as_str(vb)) {
            return x == y;
        }
        return to_number(va) == to_number(vb);
    }
    strict_equal(va, vb)
}

/// `a === b`
#[unsafe(no_mangle)]
pub extern "C" fn ot_eq_strict(a: u64, b: u64) -> u64 {
    boolean(strict_equal(OtValue::from_bits(a), OtValue::from_bits(b)))
}

/// `a == b`
#[unsafe(no_mangle)]
pub extern "C" fn ot_eq_loose(a: u64, b: u64) -> u64 {
    boolean(loose_equal(OtValue::from_bits(a), OtValue::from_bits(b)))
}

fn compare(a: u64, b: u64, f: impl Fn(std::cmp::Ordering) -> bool) -> u64 {
    let va = OtValue::from_bits(a);
    let vb = OtValue::from_bits(b);
    if let (Some(x), Some(y)) = (as_str(va), as_str(vb)) {
        return boolean(f(x.cmp(y)));
    }
    match to_number(va).partial_cmp(&to_number(vb)) {
        Some(ord) => boolean(f(ord)),
        None => boolean(false), // NaN compares false
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn ot_lt(a: u64, b: u64) -> u64 {
    compare(a, b, |o| o.is_lt())
}

#[unsafe(no_mangle)]
pub extern "C" fn ot_gt(a: u64, b: u64) -> u64 {
    compare(a, b, |o| o.is_gt())
}

#[unsafe(no_mangle)]
pub extern "C" fn ot_lte(a: u64, b: u64) -> u64 {
    compare(a, b, |o| o.is_le())
}

#[unsafe(no_mangle)]
pub extern "C" fn ot_gte(a: u64, b: u64) -> u64 {
    compare(a, b, |o| o.is_ge())
}

/// `!a`
#[unsafe(no_mangle)]
pub extern "C" fn ot_not(a: u64) -> u64 {
    boolean(!truthy(OtValue::from_bits(a)))
}

/// Truthiness as a raw `0` / `1`, for branches.
#[unsafe(no_mangle)]
pub extern "C" fn ot_truthy(a: u64) -> u64 {
    truthy(OtValue::from_bits(a)) as u64
}

/// `typeof a`
#[unsafe(no_mangle)]
pub extern "C" fn ot_typeof(a: u64) -> u64 {
    let v = OtValue::from_bits(a);
    let t = if v.is_number() {
        "number"
    } else if v.is_boolean() {
        "boolean"
    } else if v.is_undefined() {
        "undefined"
    } else {
        match object(v) {
            Some((_, ObjectKind::String)) => "string",
            Some((_, ObjectKind::Function)) => "function",
            Some((ptr, ObjectKind::Object)) if is_class(ptr) => "function",
            _ => "object",
        }
    };
    new_string(t)
}

/// `Boolean(a)`
#[unsafe(no_mangle)]
pub extern "C" fn ot_to_boolean(a: u64) -> u64 {
    boolean(truthy(OtValue::from_bits(a)))
}

/// `Number(a)`
#[unsafe(no_mangle)]
pub extern "C" fn ot_to_number(a: u64) -> u64 {
    number(to_number(OtValue::from_bits(a)))
}

/// `new.target` (not tracked natively yet).
#[unsafe(no_mangle)]
pub extern "C" fn ot_new_target() -> u64 {
    undefined()
}

/// Flush buffered output. Called when a native program exits.
#[unsafe(no_mangle)]
pub extern "C" fn ot_runtime_exit() {
    let _ = std::io::stdout().flush();
}

// =========================================================================
// Console
// =========================================================================

/// Nesting depth beyond which objects print as `[Object]` / `[Array]`.
const INSPECT_DEPTH: usize = 2;

/// Human-readable rendering of a value, in the style of Node's
/// `util.inspect`. Must stay identical to `stdlib::inspect` in the VM, so
/// `console.log` prints the same on every backend: `[ 1, 2, 3 ]`,
/// `{ x: 1, y: 's' }`, `[Circular]`. Object keys are sorted, and keys
/// starting with `__` are internal and hidden.
pub fn inspect(v: OtValue) -> String {
    let mut seen = Vec::new();
    inspect_inner(v, 0, &mut seen)
}

fn is_identifier(key: &str) -> bool {
    let mut chars = key.chars();
    matches!(chars.next(), Some(c) if c.is_alphabetic() || c == '_' || c == '$')
        && chars.all(|c| c.is_alphanumeric() || c == '_' || c == '$')
}

fn quote(s: &str) -> String {
    format!(
        "'{}'",
        s.replace('\\', "\\\\")
            .replace('\'', "\\'")
            .replace('\n', "\\n")
    )
}

fn inspect_inner(v: OtValue, depth: usize, seen: &mut Vec<usize>) -> String {
    let Some((ptr, kind)) = object(v) else {
        return value_to_string(v);
    };
    match kind {
        ObjectKind::String => quote(unsafe { ptr.as_ref::<NativeString>().as_str() }),
        ObjectKind::Function => "[Function]".to_string(),
        ObjectKind::ByteStream => "ByteStream".to_string(),
        ObjectKind::Array | ObjectKind::Object => {
            let addr = ptr.as_usize();
            if seen.contains(&addr) {
                return "[Circular]".to_string();
            }
            let too_deep = depth > INSPECT_DEPTH;
            seen.push(addr);
            let out = if kind == ObjectKind::Array {
                let arr = array(ptr);
                if arr.len == 0 {
                    "[]".to_string()
                } else if too_deep {
                    "[Array]".to_string()
                } else {
                    let parts: Vec<String> = (0..arr.len as usize)
                        .map(|i| {
                            let e = OtValue::from_bits(unsafe { *arr.elements.add(i) });
                            inspect_inner(e, depth + 1, seen)
                        })
                        .collect();
                    format!("[ {} ]", parts.join(", "))
                }
            } else if let Some(id) = promise_id(v.to_bits()) {
                promise_summary(id, depth, seen)
            } else if is_class(ptr) {
                class_summary(ptr)
            } else if is_error(ptr) {
                // `TypeError: message`, then any other own properties.
                let summary = error_summary(ptr);
                let map = props(ptr);
                let mut keys: Vec<&(String, u64)> = map
                    .iter()
                    .filter(|(k, _)| !is_hidden_key(k) && k != "message")
                    .collect();
                keys.sort_by(|a, b| a.0.cmp(&b.0));
                if keys.is_empty() || too_deep {
                    summary
                } else {
                    let parts: Vec<String> = keys
                        .into_iter()
                        .map(|(k, bits)| {
                            let val = inspect_inner(OtValue::from_bits(*bits), depth + 1, seen);
                            format!("{}: {}", k, val)
                        })
                        .collect();
                    format!("{} {{ {} }}", summary, parts.join(", "))
                }
            } else {
                let map = props(ptr);
                let mut keys: Vec<&(String, u64)> =
                    map.iter().filter(|(k, _)| !is_hidden_key(k)).collect();
                keys.sort_by(|a, b| a.0.cmp(&b.0));
                if keys.is_empty() {
                    "{}".to_string()
                } else if too_deep {
                    "[Object]".to_string()
                } else {
                    let parts: Vec<String> = keys
                        .into_iter()
                        .map(|(k, bits)| {
                            let key = if is_identifier(k) {
                                k.clone()
                            } else {
                                quote(k)
                            };
                            let val = inspect_inner(OtValue::from_bits(*bits), depth + 1, seen);
                            format!("{}: {}", key, val)
                        })
                        .collect();
                    format!("{{ {} }}", parts.join(", "))
                }
            };
            seen.pop();
            out
        }
    }
}

/// `console.log` arguments joined by a space; top-level strings print raw.
fn format_log_args(a: &[u64]) -> String {
    a.iter()
        .map(|&bits| {
            let v = OtValue::from_bits(bits);
            match as_str(v) {
                Some(s) => s.to_string(),
                None => inspect(v),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// `console.log(...)`
pub extern "C" fn ot_builtin_console_log(
    _this: u64,
    argv: *const u64,
    argc: usize,
    _env: u64,
) -> u64 {
    let line = format_log_args(unsafe { args(argv, argc) });
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{}", line);
    undefined()
}

/// `console.error(...)`
pub extern "C" fn ot_builtin_console_error(
    _this: u64,
    argv: *const u64,
    argc: usize,
    _env: u64,
) -> u64 {
    let line = format_log_args(unsafe { args(argv, argc) });
    let _ = std::io::stdout().flush();
    eprintln!("{}", line);
    undefined()
}

/// A `Math` function; `env` is its index in `MATH_FUNCTIONS`.
pub extern "C" fn ot_builtin_math(_this: u64, argv: *const u64, argc: usize, env: u64) -> u64 {
    let a = unsafe { args(argv, argc) };
    let nums: Vec<f64> = a
        .iter()
        .map(|v| {
            let v = OtValue::from_bits(*v);
            if object(v).is_some() && as_str(v).is_none() {
                to_number(OtValue::from_bits(new_string(&value_to_string(v))))
            } else {
                to_number(v)
            }
        })
        .collect();
    let id = OtValue::from_bits(env).as_number_unchecked() as usize;
    let name = builtins::MATH_FUNCTIONS.get(id).copied().unwrap_or("");
    number(builtins::math_call(name, &nums))
}

/// `String(value)`
pub extern "C" fn ot_builtin_string(_this: u64, argv: *const u64, argc: usize, _env: u64) -> u64 {
    let Some(v) = unsafe { args(argv, argc) }.first() else {
        return new_string("");
    };
    let v = match to_primitive(OtValue::from_bits(*v), true) {
        Ok(Some(p)) => p,
        Ok(None) => OtValue::from_bits(*v),
        Err(e) => return e,
    };
    new_string(&value_to_string(v))
}

/// `String(v)`, as in a template literal.
#[unsafe(no_mangle)]
pub extern "C" fn ot_to_str(v: u64) -> u64 {
    let value = OtValue::from_bits(v);
    if as_str(value).is_some() {
        return v;
    }
    let value = match to_primitive(value, true) {
        Ok(Some(p)) => p,
        Ok(None) => value,
        Err(e) => return e,
    };
    new_string(&value_to_string(value))
}

/// JavaScript `ToPrimitive` for an object with a compiled `valueOf` or
/// `toString` (`prefer_string`: `toString` first). `Ok(None)`: no such
/// method; `Err`: it threw (`EXCEPTION_BITS`).
fn to_primitive(v: OtValue, prefer_string: bool) -> Result<Option<OtValue>, u64> {
    let Some((ptr, ObjectKind::Object)) = object(v) else {
        return Ok(None);
    };
    let order = if prefer_string {
        ["toString", "valueOf"]
    } else {
        ["valueOf", "toString"]
    };
    for name in order {
        let Some(method) = lookup(ptr, name) else {
            continue;
        };
        if !matches!(
            object(OtValue::from_bits(method)),
            Some((_, ObjectKind::Function))
        ) {
            continue;
        }
        let result = ot_call(method, v.to_bits(), std::ptr::null(), 0);
        if result == EXCEPTION_BITS {
            return Err(result);
        }
        let r = OtValue::from_bits(result);
        if !matches!(object(r), Some((_, kind)) if kind != ObjectKind::String) {
            return Ok(Some(r));
        }
    }
    Ok(None)
}

/// `Date.now()`
pub extern "C" fn ot_builtin_date_now(
    _this: u64,
    _argv: *const u64,
    _argc: usize,
    _env: u64,
) -> u64 {
    number(builtins::date_now())
}

/// `Object.keys(obj)`: own property names, in insertion order. (`for...in`
/// compiles to this.)
pub extern "C" fn ot_builtin_object_keys(
    _this: u64,
    argv: *const u64,
    argc: usize,
    _env: u64,
) -> u64 {
    let a = unsafe { args(argv, argc) };
    let out = ot_alloc_array(0);
    let Some((out_ptr, _)) = object(OtValue::from_bits(out)) else {
        return out;
    };
    let target = a.first().map(|v| OtValue::from_bits(*v));
    match target.and_then(object) {
        Some((ptr, ObjectKind::Object)) => {
            let names: Vec<String> = props(ptr)
                .iter()
                .filter(|(k, _)| !is_hidden_key(k))
                .map(|(k, _)| k.clone())
                .collect();
            for k in names {
                array_push(array(out_ptr), new_string(&k));
            }
        }
        Some((ptr, ObjectKind::Array)) => {
            let len = array(ptr).len;
            for i in 0..len {
                array_push(array(out_ptr), new_string(&i.to_string()));
            }
        }
        _ => {}
    }
    out
}

/// Print one value, as `console.log(value)` does.
#[unsafe(no_mangle)]
pub extern "C" fn ot_console_log(value: u64) {
    ot_builtin_console_log(undefined(), &value, 1, undefined());
}

// =========================================================================
// File system (used by the self-hosted compiler)
// =========================================================================

#[unsafe(no_mangle)]
pub extern "C" fn ot_fs_exists(path: u64) -> u64 {
    let val = OtValue::from_bits(path);
    if let Some(ptr) = val.as_pointer() {
        unsafe {
            let header = ptr.as_ref::<ObjectHeader>();
            if header.kind == ObjectKind::String {
                let path_str = ptr.as_ref::<NativeString>().as_str();
                return OtValue::boolean(std::path::Path::new(path_str).exists()).to_bits();
            }
        }
    }
    OtValue::boolean(false).to_bits()
}

#[unsafe(no_mangle)]
pub extern "C" fn ot_fs_read_file(path: u64) -> u64 {
    let val = OtValue::from_bits(path);
    if let Some(ptr) = val.as_pointer() {
        unsafe {
            let header = ptr.as_ref::<ObjectHeader>();
            if header.kind == ObjectKind::String {
                let path_str = ptr.as_ref::<NativeString>().as_str();
                if let Ok(content) = std::fs::read_to_string(path_str)
                    && let Some(ptr) = heap().alloc_string(&content)
                {
                    return OtValue::pointer(ptr).to_bits();
                }
            }
        }
    }
    OtValue::undefined().to_bits()
}

#[unsafe(no_mangle)]
pub extern "C" fn ot_fs_write_file(path: u64, content: u64) -> u64 {
    let path_val = OtValue::from_bits(path);
    let content_val = OtValue::from_bits(content);

    let (path_str, content_str) = unsafe {
        let p = path_val.as_pointer().and_then(|ptr| {
            if ptr.as_ref::<ObjectHeader>().kind == ObjectKind::String {
                Some(ptr.as_ref::<NativeString>().as_str().to_string())
            } else {
                None
            }
        });
        let c = content_val.as_pointer().and_then(|ptr| {
            if ptr.as_ref::<ObjectHeader>().kind == ObjectKind::String {
                Some(ptr.as_ref::<NativeString>().as_str().to_string())
            } else {
                None
            }
        });
        (p, c)
    };

    if let (Some(p), Some(c)) = (path_str, content_str) {
        return OtValue::boolean(std::fs::write(&p, &c).is_ok()).to_bits();
    }
    OtValue::boolean(false).to_bits()
}

#[unsafe(no_mangle)]
pub extern "C" fn ot_fs_readdir(path: u64) -> u64 {
    let val = OtValue::from_bits(path);
    if let Some(ptr) = val.as_pointer() {
        unsafe {
            let header = ptr.as_ref::<ObjectHeader>();
            if header.kind == ObjectKind::String {
                let path_str = ptr.as_ref::<NativeString>().as_str();
                if let Ok(entries) = std::fs::read_dir(path_str) {
                    let names: Vec<String> = entries
                        .flatten()
                        .filter_map(|e| e.file_name().to_str().map(String::from))
                        .collect();
                    if let Some(arr_ptr) = heap().alloc_array(names.len()) {
                        let arr = arr_ptr.as_mut::<NativeArray>();
                        for (i, name) in names.iter().enumerate() {
                            let str_val = heap()
                                .alloc_string(name)
                                .map(|p| OtValue::pointer(p).to_bits())
                                .unwrap_or(OtValue::undefined().to_bits());
                            *arr.elements.add(i) = str_val;
                        }
                        arr.len = names.len() as u32;
                        return OtValue::pointer(arr_ptr).to_bits();
                    }
                }
                if let Some(arr_ptr) = heap().alloc_array(0) {
                    return OtValue::pointer(arr_ptr).to_bits();
                }
            }
        }
    }
    OtValue::undefined().to_bits()
}

#[unsafe(no_mangle)]
pub extern "C" fn ot_fs_stat(path: u64) -> u64 {
    let val = OtValue::from_bits(path);
    if let Some(ptr) = val.as_pointer() {
        unsafe {
            let header = ptr.as_ref::<ObjectHeader>();
            if header.kind == ObjectKind::String {
                let path_str = ptr.as_ref::<NativeString>().as_str();
                if let Ok(metadata) = std::fs::metadata(path_str)
                    && let Some(obj_ptr) = heap().alloc_object()
                {
                    let obj = obj_ptr.as_mut::<NativeObject>();
                    if obj.properties.is_null() {
                        obj.properties = Box::into_raw(Box::new(PropertyMap::new()));
                    }
                    let props = &mut *obj.properties;
                    props.push((
                        "__isDir".to_string(),
                        OtValue::boolean(metadata.is_dir()).to_bits(),
                    ));
                    props.push((
                        "size".to_string(),
                        OtValue::number(metadata.len() as f64).to_bits(),
                    ));
                    return OtValue::pointer(obj_ptr).to_bits();
                }
            }
        }
    }
    OtValue::null().to_bits()
}

#[unsafe(no_mangle)]
pub extern "C" fn ot_stat_is_directory(stat: u64) -> u64 {
    let val = OtValue::from_bits(stat);
    if let Some(ptr) = val.as_pointer() {
        unsafe {
            let header = ptr.as_ref::<ObjectHeader>();
            if header.kind == ObjectKind::Object {
                let obj = ptr.as_ref::<NativeObject>();
                if !obj.properties.is_null() {
                    for (key, value) in (*obj.properties).iter() {
                        if key == "__isDir" {
                            return *value;
                        }
                    }
                }
            }
        }
    }
    OtValue::boolean(false).to_bits()
}

#[unsafe(no_mangle)]
pub extern "C" fn ot_fs_mkdir(path: u64, recursive: u64) -> u64 {
    let val = OtValue::from_bits(path);
    let is_recursive = !OtValue::from_bits(recursive).is_falsy();

    if let Some(ptr) = val.as_pointer() {
        unsafe {
            let header = ptr.as_ref::<ObjectHeader>();
            if header.kind == ObjectKind::String {
                let path_str = ptr.as_ref::<NativeString>().as_str();
                let result = if is_recursive {
                    std::fs::create_dir_all(path_str)
                } else {
                    std::fs::create_dir(path_str)
                };
                return OtValue::boolean(result.is_ok()).to_bits();
            }
        }
    }
    OtValue::boolean(false).to_bits()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_console_log() {
        ot_console_log(OtValue::number(42.0).to_bits());
        ot_console_log(OtValue::boolean(true).to_bits());
        ot_console_log(OtValue::null().to_bits());
    }

    #[test]
    fn test_arithmetic_stubs() {
        let a = OtValue::number(10.0).to_bits();
        let b = OtValue::number(3.0).to_bits();

        assert_eq!(OtValue::from_bits(ot_add_any(a, b)).as_number(), Some(13.0));
        assert_eq!(OtValue::from_bits(ot_sub_any(a, b)).as_number(), Some(7.0));
        assert_eq!(OtValue::from_bits(ot_mul_any(a, b)).as_number(), Some(30.0));

        let div = OtValue::from_bits(ot_div_any(a, b)).as_number().unwrap();
        assert!((div - 3.333333).abs() < 0.001);
    }

    #[test]
    fn test_object_property() {
        let obj_bits = ot_alloc_object();
        assert!(!OtValue::from_bits(obj_bits).is_undefined());

        let key = "foo";
        let value = OtValue::number(42.0).to_bits();

        ot_set_prop(obj_bits, key.as_ptr(), key.len(), value);
        let retrieved = ot_get_prop(obj_bits, key.as_ptr(), key.len());

        assert_eq!(OtValue::from_bits(retrieved).as_number(), Some(42.0));
    }
}

// =========================================================================
// Promises, microtasks, timers, async functions
// =========================================================================
//
// The same semantics as the VM (`vm::promise`). A promise is an object
// whose hidden `__promise__` property is the index of its state here.
// Settling queues its reactions as microtasks, which run once `main`
// returns and after each timer (`ot_run_event_loop`).
//
// An async function is compiled as a resumable function: its locals live
// in a heap frame (`ot_async_frame`), an `await` registers it to be called
// again (`ot_await`) and returns its promise, and the runtime calls it back
// with `argc = ASYNC_RESUME_ARGC` and the frame as `argv`; it then jumps to
// the code after that `await` (`ot_await_result` gives the awaited value).

/// `argc` that tells a compiled async function it is being resumed.
pub const ASYNC_RESUME_ARGC: usize = usize::MAX;

/// Words before the locals in an async function's frame: the resume
/// point, the awaited value, and whether it was a rejection.
pub const ASYNC_FRAME_HEADER: usize = 3;

#[derive(Clone)]
enum Reaction {
    Then {
        on_fulfilled: u64,
        on_rejected: u64,
        result: usize,
    },
    Finally {
        on_finally: u64,
        result: usize,
    },
    Adopt(usize),
    AdoptLater(usize, usize),
    Resume {
        code: u64,
        frame: usize,
        this: u64,
        env: u64,
    },
    All {
        id: usize,
        index: usize,
    },
}

struct PromiseState {
    /// 0 pending, 1 fulfilled, 2 rejected.
    state: u8,
    value: u64,
    reactions: Vec<Reaction>,
    handled: bool,
}

struct AllState {
    result: usize,
    values: Vec<u64>,
    remaining: usize,
}

thread_local! {
    static PROMISES: RefCell<Vec<PromiseState>> = const { RefCell::new(Vec::new()) };
    static MICROTASKS: RefCell<std::collections::VecDeque<(Reaction, u64, bool)>> =
        const { RefCell::new(std::collections::VecDeque::new()) };
    static UNHANDLED: RefCell<Vec<usize>> = const { RefCell::new(Vec::new()) };
    static ALLS: RefCell<HashMap<usize, AllState>> = RefCell::new(HashMap::new());
    static TIMERS: RefCell<Vec<(std::time::Instant, usize, u64)>> = const { RefCell::new(Vec::new()) };
    static NEXT_ID: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

fn next_id() -> usize {
    NEXT_ID.with(|n| {
        let id = n.get();
        n.set(id + 1);
        id
    })
}

/// A new pending promise: (the object, its state index).
fn new_promise() -> (u64, usize) {
    let id = PROMISES.with(|p| {
        let mut p = p.borrow_mut();
        p.push(PromiseState {
            state: 0,
            value: undefined(),
            reactions: Vec::new(),
            handled: false,
        });
        p.len() - 1
    });
    let obj = ot_alloc_object();
    set_property(OtValue::from_bits(obj), "__promise__", number(id as f64));
    (obj, id)
}

/// The state index of a promise object.
fn promise_id(v: u64) -> Option<usize> {
    let (ptr, ObjectKind::Object) = object(OtValue::from_bits(v))? else {
        return None;
    };
    let id = props(ptr).iter().find(|(k, _)| k == "__promise__")?.1;
    OtValue::from_bits(id).as_number().map(|n| n as usize)
}

fn settle(id: usize, value: u64, fulfilled: bool) {
    let taken = PROMISES.with(|p| {
        let mut p = p.borrow_mut();
        let s = &mut p[id];
        if s.state != 0 {
            return None;
        }
        s.state = if fulfilled { 1 } else { 2 };
        s.value = value;
        Some((std::mem::take(&mut s.reactions), s.handled))
    });
    let Some((reactions, handled)) = taken else {
        return;
    };
    if !fulfilled && !handled {
        UNHANDLED.with(|u| u.borrow_mut().push(id));
    }
    MICROTASKS.with(|m| {
        let mut m = m.borrow_mut();
        for r in reactions {
            m.push_back((r, value, fulfilled));
        }
    });
}

/// Resolve with `value`: another promise is adopted (after a microtask of
/// its own, as in JavaScript), anything else fulfills.
fn resolve(id: usize, value: u64) {
    match promise_id(value) {
        Some(inner) if inner == id => {
            let e = make_error_value("TypeError", "Chaining cycle detected for promise");
            settle(id, e, false);
        }
        Some(inner) => MICROTASKS.with(|m| {
            m.borrow_mut()
                .push_back((Reaction::AdoptLater(inner, id), undefined(), true))
        }),
        None => settle(id, value, true),
    }
}

fn add_reaction(id: usize, reaction: Reaction) {
    let settled = PROMISES.with(|p| {
        let mut p = p.borrow_mut();
        let s = &mut p[id];
        s.handled = true;
        match s.state {
            0 => {
                s.reactions.push(reaction.clone());
                None
            }
            state => Some((s.value, state == 1)),
        }
    });
    UNHANDLED.with(|u| u.borrow_mut().retain(|x| *x != id));
    if let Some((value, fulfilled)) = settled {
        MICROTASKS.with(|m| m.borrow_mut().push_back((reaction, value, fulfilled)));
    }
}

/// `value` as a promise id: its own if it is one, else a fulfilled one.
fn to_promise(value: u64) -> usize {
    match promise_id(value) {
        Some(id) => id,
        None => {
            let (_, id) = new_promise();
            settle(id, value, true);
            id
        }
    }
}

fn promise_then(id: usize, on_fulfilled: u64, on_rejected: u64) -> u64 {
    let (obj, result) = new_promise();
    add_reaction(
        id,
        Reaction::Then {
            on_fulfilled,
            on_rejected,
            result,
        },
    );
    obj
}

fn is_function(v: u64) -> bool {
    matches!(
        object(OtValue::from_bits(v)),
        Some((_, ObjectKind::Function))
    )
}

/// An error object, as `throw_error` makes, without throwing it.
fn make_error_value(kind: &'static str, message: &str) -> u64 {
    throw_error(kind, message);
    ot_take_exception()
}

fn run_job(reaction: Reaction, value: u64, fulfilled: bool) {
    match reaction {
        Reaction::Then {
            on_fulfilled,
            on_rejected,
            result,
        } => {
            let handler = if fulfilled { on_fulfilled } else { on_rejected };
            if is_function(handler) {
                let r = ot_call(handler, undefined(), &value, 1);
                if r == EXCEPTION_BITS {
                    settle(result, ot_take_exception(), false);
                } else {
                    resolve(result, r);
                }
            } else if fulfilled {
                resolve(result, value);
            } else {
                settle(result, value, false);
            }
        }
        Reaction::Finally { on_finally, result } => {
            if is_function(on_finally)
                && ot_call(on_finally, undefined(), std::ptr::null(), 0) == EXCEPTION_BITS
            {
                settle(result, ot_take_exception(), false);
                return;
            }
            if fulfilled {
                resolve(result, value);
            } else {
                settle(result, value, false);
            }
        }
        Reaction::Adopt(target) => settle(target, value, fulfilled),
        Reaction::AdoptLater(source, target) => add_reaction(source, Reaction::Adopt(target)),
        Reaction::Resume {
            code,
            frame,
            this,
            env,
        } => {
            let f = frame as *mut u64;
            unsafe {
                *f.add(1) = value;
                *f.add(2) = boolean(!fulfilled);
            }
            let code: NativeFn = unsafe { std::mem::transmute(code as usize) };
            // The function's own implicit `catch` rejects its promise, so
            // it does not return an exception.
            code(this, f, ASYNC_RESUME_ARGC, env);
        }
        Reaction::All { id, index } => {
            if !fulfilled {
                if let Some(state) = ALLS.with(|a| a.borrow_mut().remove(&id)) {
                    settle(state.result, value, false);
                }
                return;
            }
            let done = ALLS.with(|a| {
                let mut a = a.borrow_mut();
                let state = a.get_mut(&id)?;
                state.values[index] = value;
                state.remaining -= 1;
                if state.remaining == 0 {
                    a.remove(&id)
                } else {
                    None
                }
            });
            if let Some(state) = done {
                let arr = ot_alloc_array(state.values.len());
                if let Some((ptr, _)) = object(OtValue::from_bits(arr)) {
                    for v in state.values {
                        array_push(array(ptr), v);
                    }
                }
                settle(state.result, arr, true);
            }
        }
    }
}

/// Run queued microtasks until there are none.
#[unsafe(no_mangle)]
pub extern "C" fn ot_run_microtasks() {
    while let Some((reaction, value, fulfilled)) = MICROTASKS.with(|m| m.borrow_mut().pop_front()) {
        run_job(reaction, value, fulfilled);
    }
}

/// After `main`: microtasks, then timers in order (each followed by its
/// microtasks), then a report of any rejection nothing handled.
#[unsafe(no_mangle)]
pub extern "C" fn ot_run_event_loop() {
    ot_run_microtasks();
    loop {
        let next = TIMERS.with(|t| {
            let mut t = t.borrow_mut();
            let i = (0..t.len()).min_by_key(|&i| (t[i].0, t[i].1))?;
            Some(t.remove(i))
        });
        let Some((due, _, callback)) = next else {
            break;
        };
        let now = std::time::Instant::now();
        if due > now {
            std::thread::sleep(due - now);
        }
        if ot_call(callback, undefined(), std::ptr::null(), 0) == EXCEPTION_BITS {
            ot_report_uncaught();
        }
        ot_run_microtasks();
    }
    if let Some(id) = UNHANDLED.with(|u| u.borrow().first().copied()) {
        let bits = PROMISES.with(|p| p.borrow()[id].value);
        let reason = OtValue::from_bits(bits);
        let text = match as_str(reason) {
            Some(s) => s.to_string(),
            None => inspect(reason),
        };
        let _ = std::io::stdout().flush();
        eprintln!("Uncaught (in promise) {}", text);
        print_trace(bits);
        std::process::exit(1);
    }
}

/// The promise of a new async function call.
#[unsafe(no_mangle)]
pub extern "C" fn ot_async_start() -> u64 {
    new_promise().0
}

/// `return value` in an async function: resolves its promise.
#[unsafe(no_mangle)]
pub extern "C" fn ot_async_resolve(promise: u64, value: u64) -> u64 {
    if let Some(id) = promise_id(promise) {
        resolve(id, value);
    }
    promise
}

/// An exception left an async function: rejects its promise.
#[unsafe(no_mangle)]
pub extern "C" fn ot_async_reject(promise: u64, reason: u64) -> u64 {
    if let Some(id) = promise_id(promise) {
        settle(id, reason, false);
    }
    promise
}

/// A frame for an async function's locals (and the resume header), all
/// undefined. Returned as a raw pointer, not a value.
#[unsafe(no_mangle)]
pub extern "C" fn ot_async_frame(slots: usize) -> u64 {
    let frame = vec![undefined(); ASYNC_FRAME_HEADER + slots].into_boxed_slice();
    Box::leak(frame).as_mut_ptr() as u64
}

/// `await value`: resume `code` (with `this`, `frame`, `env`) once it
/// settles.
#[unsafe(no_mangle)]
pub extern "C" fn ot_await(value: u64, frame: u64, code: u64, this: u64, env: u64) -> u64 {
    let id = to_promise(value);
    add_reaction(
        id,
        Reaction::Resume {
            code,
            frame: frame as usize,
            this,
            env,
        },
    );
    undefined()
}

/// The result of the `await` a resumed async function continues after:
/// the value, or the rejection thrown.
#[unsafe(no_mangle)]
pub extern "C" fn ot_await_result(frame: u64) -> u64 {
    let f = frame as *const u64;
    let (value, rejected) = unsafe { (*f.add(1), *f.add(2)) };
    if rejected == boolean(true) {
        ot_throw(value)
    } else {
        value
    }
}

/// `resolve(v)` / `reject(e)` made by `new Promise`; `env` is the
/// promise's id, times two, plus one for `reject`.
pub extern "C" fn ot_builtin_resolver(_this: u64, argv: *const u64, argc: usize, env: u64) -> u64 {
    let a = unsafe { args(argv, argc) };
    let value = a.first().copied().unwrap_or_else(undefined);
    let n = OtValue::from_bits(env).as_number_unchecked() as usize;
    let (id, reject) = (n / 2, n % 2 == 1);
    if reject {
        settle(id, value, false);
    } else {
        resolve(id, value);
    }
    undefined()
}

/// `new Promise(executor)`
fn construct_promise(executor: u64) -> u64 {
    if !is_function(executor) {
        return throw_error(
            "TypeError",
            &format!(
                "Promise resolver {} is not a function",
                inspect(OtValue::from_bits(executor))
            ),
        );
    }
    let (obj, id) = new_promise();
    let resolve_fn = ot_make_closure(ot_builtin_resolver as *const u8, number((id * 2) as f64));
    let reject_fn = ot_make_closure(
        ot_builtin_resolver as *const u8,
        number((id * 2 + 1) as f64),
    );
    let argv = [resolve_fn, reject_fn];
    if ot_call(executor, undefined(), argv.as_ptr(), 2) == EXCEPTION_BITS {
        settle(id, ot_take_exception(), false);
    }
    obj
}

/// `Promise.resolve` / `reject` / `all` / `race`; `env` picks which.
pub extern "C" fn ot_builtin_promise_static(
    _this: u64,
    argv: *const u64,
    argc: usize,
    env: u64,
) -> u64 {
    let a = unsafe { args(argv, argc) };
    let arg = a.first().copied().unwrap_or_else(undefined);
    let items = || -> Vec<u64> {
        match object(OtValue::from_bits(arg)) {
            Some((ptr, ObjectKind::Array)) => {
                let arr = array(ptr);
                (0..arr.len as usize)
                    .map(|i| unsafe { *arr.elements.add(i) })
                    .collect()
            }
            _ => Vec::new(),
        }
    };
    match OtValue::from_bits(env).as_number_unchecked() as u32 {
        0 => {
            if promise_id(arg).is_some() {
                return arg;
            }
            let (obj, id) = new_promise();
            resolve(id, arg);
            obj
        }
        1 => {
            let (obj, id) = new_promise();
            settle(id, arg, false);
            obj
        }
        2 => {
            let values = items();
            let (obj, result) = new_promise();
            if values.is_empty() {
                settle(result, ot_alloc_array(0), true);
                return obj;
            }
            let all = next_id();
            ALLS.with(|a| {
                a.borrow_mut().insert(
                    all,
                    AllState {
                        result,
                        values: vec![undefined(); values.len()],
                        remaining: values.len(),
                    },
                )
            });
            for (index, v) in values.into_iter().enumerate() {
                add_reaction(to_promise(v), Reaction::All { id: all, index });
            }
            obj
        }
        _ => {
            let (obj, result) = new_promise();
            for v in items() {
                add_reaction(to_promise(v), Reaction::Adopt(result));
            }
            obj
        }
    }
}

/// `p.then` / `catch` / `finally`, for `ot_call_method`.
fn promise_method(id: usize, key: &str, a: &[u64]) -> Option<u64> {
    let arg = |i: usize| a.get(i).copied().unwrap_or_else(undefined);
    Some(match key {
        "then" => promise_then(id, arg(0), arg(1)),
        "catch" => promise_then(id, undefined(), arg(0)),
        "finally" => {
            let (obj, result) = new_promise();
            add_reaction(
                id,
                Reaction::Finally {
                    on_finally: arg(0),
                    result,
                },
            );
            obj
        }
        _ => return None,
    })
}

/// `Promise { 1 }`, `Promise { <pending> }`, `Promise { <rejected> e }`.
fn promise_summary(id: usize, depth: usize, seen: &mut Vec<usize>) -> String {
    let (state, value) = PROMISES.with(|p| {
        let p = p.borrow();
        (p[id].state, p[id].value)
    });
    let mut shown = || inspect_inner(OtValue::from_bits(value), depth + 1, seen);
    match state {
        0 => "Promise { <pending> }".to_string(),
        1 => format!("Promise {{ {} }}", shown()),
        _ => format!("Promise {{ <rejected> {} }}", shown()),
    }
}

/// `setTimeout(callback, ms)` / `clearTimeout(id)`; `env` 0 or 1.
pub extern "C" fn ot_builtin_timer(_this: u64, argv: *const u64, argc: usize, env: u64) -> u64 {
    let a = unsafe { args(argv, argc) };
    let arg = |i: usize| a.get(i).copied().unwrap_or_else(undefined);
    if OtValue::from_bits(env).as_number_unchecked() as u32 == 1 {
        let id = OtValue::from_bits(arg(0)).as_number().unwrap_or(-1.0);
        TIMERS.with(|t| t.borrow_mut().retain(|(_, tid, _)| *tid as f64 != id));
        return undefined();
    }
    let ms = OtValue::from_bits(arg(1))
        .as_number()
        .filter(|n| *n > 0.0)
        .unwrap_or(0.0);
    let id = next_id();
    let due = std::time::Instant::now() + std::time::Duration::from_millis(ms as u64);
    TIMERS.with(|t| t.borrow_mut().push((due, id, arg(0))));
    number(id as f64)
}

/// Entry of a compiled async function: its frame (`argv`, when resumed),
/// or a new one with `slots` locals.
#[unsafe(no_mangle)]
pub extern "C" fn ot_async_enter(argv: *const u64, argc: usize, slots: usize) -> u64 {
    if argc == ASYNC_RESUME_ARGC {
        argv as u64
    } else {
        ot_async_frame(slots)
    }
}
