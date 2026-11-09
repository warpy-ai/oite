//! Exceptions: the pending exception, the stack limit, stack traces and
//! the error objects the runtime throws.

use super::*;

// =========================================================================
// Exceptions
// =========================================================================
//
// Native code has no unwinding. A function that throws stores the thrown
// value in `PENDING` and returns `EXCEPTION_BITS`; the code that called it
// sees that result and jumps to its `catch`/`finally`, or returns
// `EXCEPTION_BITS` itself. A `catch` takes the value back out of `PENDING`.

thread_local! {
    pub(super) static PENDING: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
    /// Prototype objects of the error kinds, created on first use.
    pub(super) static ERROR_PROTOS: RefCell<HashMap<&'static str, u64>> = RefCell::new(HashMap::new());
}

/// The lowest stack address compiled code may reach: every compiled
/// function compares its stack pointer with it on entry and throws a
/// RangeError below it. 0 (no limit) until `ot_stack_init`.
#[unsafe(no_mangle)]
pub static OT_STACK_LIMIT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Stack that recursion in compiled code may use, below where the program
/// starts. Main threads get 8MB by default; the rest is left for the
/// runtime and its callers.
pub(super) const STACK_BUDGET: usize = 4 << 20;

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
pub(super) struct Trace {
    /// The exception the frames are of.
    value: Option<u64>,
    frames: Vec<&'static str>,
    /// Skip the next frame (a rethrow's own function).
    skip: bool,
}

thread_local! {
    pub(super) static TRACE: RefCell<Trace> = RefCell::new(Trace::default());
}

/// Frames kept, as in V8 (`Error.stackTraceLimit`).
pub(super) const TRACE_LIMIT: usize = 10;

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
pub(super) fn print_trace(value: u64) {
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
pub(super) fn error_proto(kind: &'static str) -> u64 {
    if let Some(class) = global(kind)
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
pub(super) fn throw_error(kind: &'static str, message: &str) -> u64 {
    let error = ot_alloc_object();
    let e = OtValue::from_bits(error);
    set_property(e, "message", new_string(message));
    set_property(e, "__proto__", error_proto(kind));
    ot_throw(error)
}

/// Whether `ptr` is an error object: `Error.prototype` is on its
/// prototype chain (or, without the error classes, the chain is marked).
pub(super) fn is_error(ptr: HeapPtr) -> bool {
    let error_proto = global("Error")
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
pub(super) fn error_summary(ptr: HeapPtr) -> String {
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
