//! Promises, the microtask queue, and suspended async functions.
//!
//! Settling a promise queues its reactions as microtasks; the VM runs them
//! after the main script and after each task (`run_event_loop`), as
//! JavaScript does. An `await` suspends its async function: the frame, its
//! part of the operand stack and its exception handlers are put aside, the
//! function's promise is returned to its caller, and a microtask resumes it
//! once the awaited promise settles.

use std::collections::HashMap;

use super::value::{HeapData, HeapObject, JsValue, Promise, PromiseState, Reaction};
use super::{ExceptionHandler, Frame, VM};

/// Local holding an async function's own promise.
pub const PROMISE_LOCAL: &str = "%promise";
/// Local holding where an async function's part of the operand stack
/// starts.
pub const STACK_BASE_LOCAL: &str = "%stackbase";

/// A microtask: a reaction to run with the value its promise settled with.
pub struct Microtask {
    pub reaction: Reaction,
    pub value: JsValue,
    pub fulfilled: bool,
}

/// An async function waiting for a promise.
pub struct Suspended {
    pub frame: Frame,
    /// Its part of the operand stack.
    pub stack: Vec<JsValue>,
    /// Its exception handlers, with depths relative to its stack part and
    /// frame.
    pub handlers: Vec<ExceptionHandler>,
    pub resume_ip: usize,
}

/// A `Promise.all` in progress.
pub struct AllState {
    pub result: Promise,
    pub values: Vec<JsValue>,
    pub remaining: usize,
}

impl VM {
    /// Fulfill or reject `p`, once; its reactions are queued.
    pub fn settle_promise(&mut self, p: &Promise, value: JsValue, fulfilled: bool) {
        let (reactions, handled) = {
            let mut internal = p.state.lock().unwrap();
            if internal.state != PromiseState::Pending {
                return;
            }
            internal.state = if fulfilled {
                PromiseState::Fulfilled
            } else {
                PromiseState::Rejected
            };
            internal.value = Some(value.clone());
            (std::mem::take(&mut internal.reactions), internal.handled)
        };
        if !fulfilled && !handled {
            self.unhandled_rejections.push(p.clone());
        }
        for reaction in reactions {
            self.microtasks.push_back(Microtask {
                reaction,
                value: value.clone(),
                fulfilled,
            });
        }
    }

    /// Resolve `p` with `value`: a promise is adopted (`p` settles the way
    /// it does), anything else fulfills `p`.
    pub fn resolve_promise(&mut self, p: &Promise, value: JsValue) {
        if let JsValue::Promise(inner) = &value {
            if inner == p {
                let e = self.make_error(
                    "TypeError",
                    "Chaining cycle detected for promise".to_string(),
                );
                self.settle_promise(p, e, false);
                return;
            }
            // As in JavaScript, adopting takes a microtask of its own before
            // the reaction is attached (so the ordering matches).
            self.microtasks.push_back(Microtask {
                reaction: Reaction::AdoptLater(inner.clone(), p.clone()),
                value: JsValue::Undefined,
                fulfilled: true,
            });
            return;
        }
        self.settle_promise(p, value, true);
    }

    /// Run `reaction` when `p` settles (in a microtask, also if it already
    /// has).
    pub fn add_reaction(&mut self, p: &Promise, reaction: Reaction) {
        let settled = {
            let mut internal = p.state.lock().unwrap();
            internal.handled = true;
            match internal.state {
                PromiseState::Pending => {
                    internal.reactions.push(reaction.clone());
                    None
                }
                PromiseState::Fulfilled => Some((internal.value.clone(), true)),
                PromiseState::Rejected => Some((internal.value.clone(), false)),
            }
        };
        self.unhandled_rejections.retain(|q| q != p);
        if let Some((value, fulfilled)) = settled {
            self.microtasks.push_back(Microtask {
                reaction,
                value: value.unwrap_or(JsValue::Undefined),
                fulfilled,
            });
        }
    }

    /// `p.then(onFulfilled, onRejected)`
    pub fn promise_then(
        &mut self,
        p: &Promise,
        on_fulfilled: JsValue,
        on_rejected: JsValue,
    ) -> Promise {
        let result = Promise::new();
        self.add_reaction(
            p,
            Reaction::Then {
                on_fulfilled,
                on_rejected,
                result: result.clone(),
            },
        );
        result
    }

    /// `value` as a promise: itself if it is one, else fulfilled with it.
    pub fn to_promise(&mut self, value: JsValue) -> Promise {
        match value {
            JsValue::Promise(p) => p,
            other => Promise::with_value(other),
        }
    }

    /// Run queued microtasks until there are none.
    pub fn run_microtasks(&mut self) {
        while let Some(task) = self.microtasks.pop_front() {
            self.run_reaction(task);
        }
    }

    fn run_reaction(&mut self, task: Microtask) {
        let Microtask {
            reaction,
            value,
            fulfilled,
        } = task;
        match reaction {
            Reaction::Then {
                on_fulfilled,
                on_rejected,
                result,
            } => {
                let handler = if fulfilled { on_fulfilled } else { on_rejected };
                if self.is_callable(&handler) {
                    match self.call_value(&handler, JsValue::Undefined, vec![value]) {
                        Ok(v) => self.resolve_promise(&result, v),
                        Err(e) => self.settle_promise(&result, e, false),
                    }
                } else if fulfilled {
                    self.resolve_promise(&result, value);
                } else {
                    self.settle_promise(&result, value, false);
                }
            }
            Reaction::Finally { on_finally, result } => {
                if self.is_callable(&on_finally)
                    && let Err(e) = self.call_value(&on_finally, JsValue::Undefined, Vec::new())
                {
                    self.settle_promise(&result, e, false);
                    return;
                }
                if fulfilled {
                    self.resolve_promise(&result, value);
                } else {
                    self.settle_promise(&result, value, false);
                }
            }
            Reaction::Adopt(target) => self.settle_promise(&target, value, fulfilled),
            Reaction::AdoptLater(source, target) => {
                self.add_reaction(&source, Reaction::Adopt(target));
            }
            Reaction::Resume(id) => self.resume_async(id, value, fulfilled),
            Reaction::All { id, index } => {
                if !fulfilled {
                    if let Some(state) = self.promise_alls.remove(&id) {
                        self.settle_promise(&state.result, value, false);
                    }
                    return;
                }
                let done = match self.promise_alls.get_mut(&id) {
                    Some(state) => {
                        state.values[index] = value;
                        state.remaining -= 1;
                        state.remaining == 0
                    }
                    None => false,
                };
                if done && let Some(state) = self.promise_alls.remove(&id) {
                    let array = self.alloc_array(state.values);
                    self.settle_promise(&state.result, array, true);
                }
            }
        }
    }

    /// `Promise.all(values)`
    pub fn promise_all(&mut self, values: Vec<JsValue>) -> Promise {
        let result = Promise::new();
        if values.is_empty() {
            let array = self.alloc_array(Vec::new());
            self.settle_promise(&result, array, true);
            return result;
        }
        let id = self.next_async_id;
        self.next_async_id += 1;
        self.promise_alls.insert(
            id,
            AllState {
                result: result.clone(),
                values: vec![JsValue::Undefined; values.len()],
                remaining: values.len(),
            },
        );
        for (index, v) in values.into_iter().enumerate() {
            let p = self.to_promise(v);
            self.add_reaction(&p, Reaction::All { id, index });
        }
        result
    }

    pub(crate) fn alloc_array(&mut self, items: Vec<JsValue>) -> JsValue {
        let ptr = self.heap.len();
        self.heap.push(HeapObject {
            data: HeapData::Array(items),
        });
        JsValue::Object(ptr)
    }

    /// Functions, native functions and promise resolvers.
    pub fn is_callable(&self, v: &JsValue) -> bool {
        match v {
            JsValue::Function { .. } | JsValue::NativeFunction(_) => true,
            JsValue::Object(ptr) => self.resolver_of(*ptr).is_some(),
            _ => false,
        }
    }

    /// The promise and outcome of a `resolve` / `reject` function made for
    /// `new Promise`.
    pub fn resolver_of(&self, ptr: usize) -> Option<(Promise, bool)> {
        match self.heap.get(ptr) {
            Some(HeapObject {
                data: HeapData::Object(props),
            }) => match (props.get("__resolver__"), props.get("__fulfills__")) {
                (Some(JsValue::Promise(p)), Some(JsValue::Boolean(f))) => Some((p.clone(), *f)),
                _ => None,
            },
            _ => None,
        }
    }

    /// A `resolve` (`fulfills`) or `reject` function for `p`.
    pub fn make_resolver(&mut self, p: &Promise, fulfills: bool) -> JsValue {
        let mut props = HashMap::new();
        props.insert("__resolver__".to_string(), JsValue::Promise(p.clone()));
        props.insert("__fulfills__".to_string(), JsValue::Boolean(fulfills));
        let ptr = self.heap.len();
        self.heap.push(HeapObject {
            data: HeapData::Object(props),
        });
        JsValue::Object(ptr)
    }

    /// Call any callable value to completion, resolvers included.
    pub fn call_value(
        &mut self,
        f: &JsValue,
        this: JsValue,
        args: Vec<JsValue>,
    ) -> Result<JsValue, JsValue> {
        if let JsValue::Object(ptr) = f
            && let Some((p, fulfills)) = self.resolver_of(*ptr)
        {
            let value = args.into_iter().next().unwrap_or(JsValue::Undefined);
            if fulfills {
                self.resolve_promise(&p, value);
            } else {
                self.settle_promise(&p, value, false);
            }
            return Ok(JsValue::Undefined);
        }
        self.call_sync(f, this, args)
    }

    /// `new Promise(executor)`: the executor runs now, with `resolve` and
    /// `reject`; if it throws, the promise is rejected.
    pub fn construct_promise(&mut self, executor: JsValue) -> Result<Promise, JsValue> {
        if !self.is_callable(&executor) {
            let shown = crate::stdlib::inspect(self, &executor);
            return Err(self.make_error(
                "TypeError",
                format!("Promise resolver {shown} is not a function"),
            ));
        }
        let p = Promise::new();
        let resolve = self.make_resolver(&p, true);
        let reject = self.make_resolver(&p, false);
        if let Err(e) = self.call_value(&executor, JsValue::Undefined, vec![resolve, reject]) {
            self.settle_promise(&p, e, false);
        }
        Ok(p)
    }

    /// Resume suspended async function `id` with the awaited outcome: the
    /// value is the `await`'s result, or a rejection is thrown there.
    fn resume_async(&mut self, id: usize, value: JsValue, fulfilled: bool) {
        let Some(s) = self.suspended.remove(&id) else {
            return;
        };
        let saved_ip = self.ip;
        let saved_handlers = self.exception_handlers.len();
        let base = self.stack.len();
        let depth = self.call_stack.len();
        // A barrier below the function's handlers: anything that escapes it
        // (its own implicit `catch` rejects its promise, so nothing should)
        // stops here.
        self.exception_handlers.push(ExceptionHandler {
            catch_addr: super::CALL_SYNC_BARRIER,
            finally_addr: 0,
            stack_depth: base,
            call_stack_depth: depth,
        });
        self.stack.extend(s.stack);
        for mut h in s.handlers {
            h.stack_depth += base;
            h.call_stack_depth += depth;
            self.exception_handlers.push(h);
        }
        let mut frame = s.frame;
        frame.return_address = usize::MAX;
        frame
            .locals
            .insert(STACK_BASE_LOCAL.to_string(), JsValue::Number(base as f64));
        self.call_stack.push(frame);
        self.ip = s.resume_ip;
        if fulfilled {
            self.stack.push(value);
            self.run_until_return_sentinel();
        } else if self.throw_exception_at_resume(value) {
            self.run_until_return_sentinel();
        }
        self.barrier_exception = None;
        self.stack.truncate(base);
        self.call_stack.truncate(depth);
        self.exception_handlers.truncate(saved_handlers);
        self.ip = saved_ip;
    }

    /// Throw a rejection where a resumed async function awaited; false if
    /// it went past the function.
    fn throw_exception_at_resume(&mut self, reason: JsValue) -> bool {
        !matches!(self.throw_exception(reason), super::ExecResult::Stop)
    }

    /// Report a rejection no handler was attached to, like an uncaught
    /// exception. True if there was one.
    pub fn report_unhandled_rejection(&mut self) -> bool {
        let Some(p) = self.unhandled_rejections.first().cloned() else {
            return false;
        };
        let reason = p.get_value().unwrap_or(JsValue::Undefined);
        let text = match &reason {
            JsValue::String(s) => s.clone(),
            other => crate::stdlib::inspect(self, other),
        };
        if self.exit_on_uncaught {
            use std::io::Write;
            let _ = std::io::stdout().flush();
            eprintln!("Uncaught (in promise) {text}");
            for line in self.trace_lines(&reason) {
                eprintln!("{line}");
            }
            std::process::exit(1);
        }
        true
    }
}
