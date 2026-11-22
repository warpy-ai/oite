//! Mark-sweep collection of the VM heap (#123). Phase 1 of
//! `docs/design/memory-reclamation.md`.
//!
//! A heap reference is a slot index into `VM::heap`. Every allocation goes
//! through [`VM::gc_alloc`], which reuses a freed slot before the heap
//! grows. Objects never move: a collection poisons each unreachable slot
//! (an empty `ByteStream`, which drops what the slot held) and puts it on
//! the free list.
//!
//! # When
//!
//! Only at an instruction boundary (the top of `exec_one`), and only while
//! no Rust frame has re-entered the VM. `depth` counts the active
//! `run_until_return_sentinel` calls (`call_sync`, `resume_async`) and
//! `execute_module` calls. A Rust frame that re-enters the VM holds values
//! in locals the collector cannot see: the right operand while `valueOf`
//! runs for `a + b`, the promise a running `.then` reaction resolves next,
//! `execute_module`'s saved operand stack. So code running inside a timer
//! callback, a promise reaction, a resumed async function or a module body
//! does not collect. The next instruction back at depth 0 does.
//!
//! A collection runs once the allocations since the last one reach
//! `threshold`, which is max(live objects, 100,000). That keeps the cost
//! at amortized O(1) per allocation. With `OITE_GC_STRESS` set, the
//! threshold is one allocation, so every safe point after an allocation
//! collects, and a missed root shows up as a freed object still in use.
//!
//! # Roots
//!
//! `mark_roots` names every `VM` field, so a new field does not compile
//! until it is classified there. Traced:
//!
//! - `stack`: the operand stack.
//! - `call_stack`: each frame's `locals`, `indexed_locals`,
//!   `this_context`, `new_target` and `env`.
//! - `modules`, and each `module_cache` entry's namespace object.
//! - `task_queue` and `timers`: each callback and its arguments.
//! - `current_exception`, `barrier_exception`, `native_throw` and
//!   `traced_exception`.
//! - `microtasks`: each reaction and the value it runs with.
//! - `suspended`: each async function's frame and operand stack.
//! - `promise_alls`: each result promise and the values so far.
//! - `unhandled_rejections`.
//! - Every promise reachable from these: its value, and each reaction's
//!   handlers and result promise. A promise is a Rust `Arc`, freed by
//!   Rust; it is traced only for the heap objects it holds.
//!
//! Not roots, because they hold no heap reference: `program` (the
//! compiler emits `Push(Function { env: None })` and no object
//! constants), `native_functions` (plain `fn` pointers, which capture
//! nothing), `async_task_tx` (nothing sends on it), the exception
//! handlers, and the remaining bookkeeping (addresses, counters, paths).

use std::collections::HashSet;
use std::sync::Arc;

use super::module_cache::CachedModule;
use super::promise::{AllState, Microtask, Suspended};
use super::value::{HeapData, HeapObject, JsValue, Promise, PromiseInternal, Reaction};
use super::{Frame, Task, TimerTask, VM};

/// The fewest allocations between two collections.
const MIN_THRESHOLD: usize = 100_000;

pub struct Gc {
    /// Freed slots, reused before the heap grows.
    free: Vec<usize>,
    /// Allocations since the last collection.
    allocated: usize,
    /// Collect once `allocated` reaches this.
    threshold: usize,
    /// `OITE_GC_STRESS`: collect at every safe point after an allocation.
    stress: bool,
    /// Rust frames that have re-entered the VM; collection waits for 0.
    pub(super) depth: usize,
}

impl Gc {
    pub fn from_env() -> Self {
        let stress = std::env::var_os("OITE_GC_STRESS").is_some();
        Self {
            free: Vec::new(),
            allocated: 0,
            threshold: if stress { 1 } else { MIN_THRESHOLD },
            stress,
            depth: 0,
        }
    }
}

impl VM {
    /// Put `data` in a heap slot and return the slot's index.
    pub(crate) fn gc_alloc(&mut self, data: HeapData) -> usize {
        self.gc.allocated += 1;
        let object = HeapObject { data };
        match self.gc.free.pop() {
            Some(i) => {
                self.heap[i] = object;
                i
            }
            None => {
                self.heap.push(object);
                self.heap.len() - 1
            }
        }
    }

    /// Collect if enough was allocated and no Rust frame holds values.
    /// Called between instructions.
    #[inline(always)]
    pub(super) fn gc_safe_point(&mut self) {
        if self.gc.allocated >= self.gc.threshold && self.gc.depth == 0 {
            self.collect();
        }
    }

    #[cold]
    #[inline(never)]
    fn collect(&mut self) {
        let mut marker = Marker {
            marks: vec![false; self.heap.len()],
            objects: Vec::new(),
            promises: Vec::new(),
            seen_promises: HashSet::new(),
        };
        self.mark_roots(&mut marker);
        marker.trace(&self.heap);

        self.gc.free.clear();
        for (i, object) in self.heap.iter_mut().enumerate() {
            if !marker.marks[i] {
                object.data = HeapData::ByteStream(Vec::new());
                self.gc.free.push(i);
            }
        }
        let live = self.heap.len() - self.gc.free.len();
        self.gc.allocated = 0;
        if !self.gc.stress {
            self.gc.threshold = live.max(MIN_THRESHOLD);
        }
    }

    fn mark_roots(&self, m: &mut Marker) {
        let VM {
            stack,
            call_stack,
            heap: _,
            native_functions: _,
            task_queue,
            timers,
            program: _,
            modules,
            ip: _,
            function_call_counts: _,
            total_instructions: _,
            exception_handlers: _,
            current_exception,
            barrier_exception,
            native_throw,
            exit_on_uncaught: _,
            sources: _,
            exception_trace: _,
            traced_exception,
            current_module_path: _,
            async_runtime: _,
            async_task_tx: _,
            module_cache,
            compiler: _,
            microtasks,
            suspended,
            promise_alls,
            next_async_id: _,
            unhandled_rejections,
            gc: _,
        } = self;

        m.values(stack);
        for frame in call_stack {
            m.frame(frame);
        }
        m.values(modules.values());
        for CachedModule {
            path: _,
            source: _,
            hash: _,
            load_time: _,
            namespace_object,
        } in module_cache.entries.values()
        {
            m.object(*namespace_object);
        }
        for task in task_queue {
            m.task(task);
        }
        for TimerTask {
            id: _,
            due: _,
            task,
        } in timers
        {
            m.task(task);
        }
        m.values(
            [
                current_exception,
                barrier_exception,
                native_throw,
                traced_exception,
            ]
            .into_iter()
            .flatten(),
        );
        for Microtask {
            reaction,
            value,
            fulfilled: _,
        } in microtasks
        {
            m.reaction(reaction);
            m.value(value);
        }
        for Suspended {
            frame,
            stack,
            handlers: _,
            resume_ip: _,
        } in suspended.values()
        {
            m.frame(frame);
            m.values(stack);
        }
        for AllState {
            result,
            values,
            remaining: _,
        } in promise_alls.values()
        {
            m.promise(result);
            m.values(values);
        }
        for p in unhandled_rejections {
            m.promise(p);
        }
    }
}

struct Marker {
    marks: Vec<bool>,
    /// Marked objects whose contents are not traced yet.
    objects: Vec<usize>,
    /// Promises whose contents are not traced yet.
    promises: Vec<Promise>,
    seen_promises: HashSet<usize>,
}

impl Marker {
    fn value(&mut self, v: &JsValue) {
        match v {
            JsValue::Object(i) => self.object(*i),
            JsValue::Function { address: _, env } => {
                if let Some(env) = env {
                    self.object(*env);
                }
            }
            JsValue::Accessor(get, set) => {
                for f in [get, set].into_iter().flatten() {
                    self.value(f);
                }
            }
            JsValue::Promise(p) => self.promise(p),
            JsValue::Number(_)
            | JsValue::String(_)
            | JsValue::Boolean(_)
            | JsValue::NativeFunction(_)
            | JsValue::Null
            | JsValue::Undefined => {}
        }
    }

    fn values<'a>(&mut self, vs: impl IntoIterator<Item = &'a JsValue>) {
        for v in vs {
            self.value(v);
        }
    }

    fn object(&mut self, i: usize) {
        // An index past the heap is not an object, as everywhere in the VM.
        if let Some(mark) = self.marks.get_mut(i)
            && !*mark
        {
            *mark = true;
            self.objects.push(i);
        }
    }

    fn promise(&mut self, p: &Promise) {
        if self.seen_promises.insert(Arc::as_ptr(&p.state) as usize) {
            self.promises.push(p.clone());
        }
    }

    fn reaction(&mut self, r: &Reaction) {
        match r {
            Reaction::Then {
                on_fulfilled,
                on_rejected,
                result,
            } => {
                self.value(on_fulfilled);
                self.value(on_rejected);
                self.promise(result);
            }
            Reaction::Finally { on_finally, result } => {
                self.value(on_finally);
                self.promise(result);
            }
            Reaction::Adopt(target) => self.promise(target),
            Reaction::AdoptLater(source, target) => {
                self.promise(source);
                self.promise(target);
            }
            // `suspended` and `promise_alls` are roots themselves.
            Reaction::Resume(_) | Reaction::All { .. } => {}
        }
    }

    fn frame(&mut self, f: &Frame) {
        let Frame {
            return_address: _,
            locals,
            indexed_locals,
            this_context,
            new_target,
            super_called: _,
            resume_ip: _,
            env,
        } = f;
        self.values(locals.values());
        self.values(indexed_locals);
        self.value(this_context);
        self.values(new_target);
        if let Some(env) = env {
            self.object(*env);
        }
    }

    fn task(&mut self, t: &Task) {
        let Task { function_ptr, args } = t;
        self.value(function_ptr);
        self.values(args);
    }

    /// Mark everything reachable from what is marked so far.
    fn trace(&mut self, heap: &[HeapObject]) {
        loop {
            if let Some(i) = self.objects.pop() {
                match &heap[i].data {
                    HeapData::Object(props) => self.values(props.values()),
                    HeapData::Array(items) | HeapData::Set(items) => self.values(items),
                    HeapData::Map(pairs) => {
                        for (k, v) in pairs {
                            self.value(k);
                            self.value(v);
                        }
                    }
                    HeapData::ByteStream(_) => {}
                }
            } else if let Some(p) = self.promises.pop() {
                let internal = p.state.lock().unwrap();
                let PromiseInternal {
                    state: _,
                    value,
                    reactions,
                    handled: _,
                } = &*internal;
                self.values(value);
                for r in reactions {
                    self.reaction(r);
                }
            } else {
                return;
            }
        }
    }
}
