//! Async functions (resumable functions with heap frames) and rest
//! arguments.

use super::*;

/// `...rest`: the arguments from the `n`th on, as a new array.
#[unsafe(no_mangle)]
pub extern "C" fn ot_rest_args(argv: *const u64, argc: usize, n: usize) -> u64 {
    let a = unsafe { args(argv, argc) };
    let rest = a.get(n..).unwrap_or(&[]);
    let arr = ot_alloc_array(rest.len());
    if let Some((ptr, _)) = object(OtValue::from_bits(arr)) {
        let arr_ref = array(ptr);
        for &v in rest {
            array_push(arr_ref, v);
        }
    }
    arr
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
