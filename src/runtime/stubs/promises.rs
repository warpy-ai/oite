//! Promises, microtasks and timers.

use super::*;

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
pub(super) enum Reaction {
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

pub(super) struct PromiseState {
    /// 0 pending, 1 fulfilled, 2 rejected.
    state: u8,
    value: u64,
    reactions: Vec<Reaction>,
    handled: bool,
}

pub(super) struct AllState {
    result: usize,
    values: Vec<u64>,
    remaining: usize,
}

thread_local! {
    pub(super) static PROMISES: RefCell<Vec<PromiseState>> = const { RefCell::new(Vec::new()) };
    pub(super) static MICROTASKS: RefCell<std::collections::VecDeque<(Reaction, u64, bool)>> =
        const { RefCell::new(std::collections::VecDeque::new()) };
    pub(super) static UNHANDLED: RefCell<Vec<usize>> = const { RefCell::new(Vec::new()) };
    pub(super) static ALLS: RefCell<HashMap<usize, AllState>> = RefCell::new(HashMap::new());
    pub(super) static TIMERS: RefCell<Vec<(std::time::Instant, usize, u64)>> = const { RefCell::new(Vec::new()) };
    pub(super) static NEXT_ID: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

pub(super) fn next_id() -> usize {
    NEXT_ID.with(|n| {
        let id = n.get();
        n.set(id + 1);
        id
    })
}

/// A new pending promise: (the object, its state index).
pub(super) fn new_promise() -> (u64, usize) {
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
pub(super) fn promise_id(v: u64) -> Option<usize> {
    let (ptr, ObjectKind::Object) = object(OtValue::from_bits(v))? else {
        return None;
    };
    let id = props(ptr).iter().find(|(k, _)| k == "__promise__")?.1;
    OtValue::from_bits(id).as_number().map(|n| n as usize)
}

pub(super) fn settle(id: usize, value: u64, fulfilled: bool) {
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
pub(super) fn resolve(id: usize, value: u64) {
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

pub(super) fn add_reaction(id: usize, reaction: Reaction) {
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
pub(super) fn to_promise(value: u64) -> usize {
    match promise_id(value) {
        Some(id) => id,
        None => {
            let (_, id) = new_promise();
            settle(id, value, true);
            id
        }
    }
}

pub(super) fn promise_then(id: usize, on_fulfilled: u64, on_rejected: u64) -> u64 {
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

pub(super) fn is_function(v: u64) -> bool {
    matches!(
        object(OtValue::from_bits(v)),
        Some((_, ObjectKind::Function))
    )
}

/// An error object, as `throw_error` makes, without throwing it.
pub(super) fn make_error_value(kind: &'static str, message: &str) -> u64 {
    throw_error(kind, message);
    ot_take_exception()
}

pub(super) fn run_job(reaction: Reaction, value: u64, fulfilled: bool) {
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
pub(super) fn construct_promise(executor: u64) -> u64 {
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
pub(super) fn promise_method(id: usize, key: &str, a: &[u64]) -> Option<u64> {
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
pub(super) fn promise_summary(id: usize, depth: usize, seen: &mut Vec<usize>) -> String {
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
