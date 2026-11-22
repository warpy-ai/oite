# Memory reclamation

Status: proposed. Phase 1 (the VM collector) is implemented. The rest needs a decision, because the README and the conformance page say Oite has no garbage collector.

Goal: a long-running Oite program must not grow memory forever ([#123](https://github.com/warpy-ai/oite/issues/123)), on the VM, the JIT and AOT.

## Where memory goes today

- **VM heap.** `VM::heap` is a `Vec<HeapObject>`. A reference is a slot index: `JsValue::Object(usize)`, or the `env` of a `JsValue::Function`. `HeapData` is an object, array, byte stream, `Map` or `Set`. Strings are Rust `String`s and promises are `Arc`s, so Rust frees both. Before phase 1, nothing removed a slot, and the #123 loop peaked at 1,049 MB for 3 million objects (about 350 bytes each).
- **Native heap.** `NativeHeap` (`src/runtime/heap.rs`) bump-allocates in chunks and never frees. Values are NaN-boxed `u64`s. `ObjectHeader.marked` exists, but nothing reads it. Array elements, overflow slots and dictionaries are separate `malloc` buffers. Async frames are leaked with `Box::leak` (`stubs/async_fns.rs`), and the `PROMISES` table never shrinks (`stubs/promises.rs`). Compiled code allocates only through runtime stubs, never inline. Exceptions are return codes, so there is no unwinding.
- **Per object in the #123 loop.** About 350 bytes on the VM and 56 bytes on AOT (a `NativeObject` with two inline slots).

## Why the borrow checker cannot decide frees

The ownership model suggests freeing a value at its `Drop`. The checker cannot prove that safe:

- It checks top-level statements only (`src/compiler/borrow_ck.rs`). A catch-all arm skips function and class bodies, `try`, `switch`, `for...of`, `for...in` and `do...while`.
- Only `let y = x`, `x = y` and `o.f = y` with a bare identifier count as moves. A call argument is a borrow that ends with the statement, so the callee may keep it. Array and object literal elements are plain uses. Closures that are not async are not analyzed.
- No IR lowering emits the `Move` or `Borrow` ops. `OpCode::Drop` lowers to nothing in native code (`src/ir/lower/instr.rs`) and only unbinds the name on the VM.
- The checker accepts `for (...) { let o = {...}; keep.push(o); }`. The `Drop` of `o` runs every iteration while `keep[i]` is still live, so freeing there would be a use-after-free.

## Options

### A. Free at `Drop` (ownership)

`Drop` would lower to an `IrOp::Free` that calls an `ot_free` stub. That needs a new checker that proves the value is unique at every `Drop`: it would cover every function body and track arguments, container stores, returns, captures and `this`. The checker lands twice (`borrow_ck.rs` and the self-hosted `borrow_ck.ot`) and `Free` lands in three emitters. Any gap in the proof is a dangling pointer. The checker would also reject ordinary code such as `arr.push(o); o.x = 1`, which contradicts "most JavaScript code works unchanged" in `docs/docs/ecmascript-conformance.md`.

Rejected as the main mechanism. Its sound subset, escape analysis in the IR, is phase 6.

### B. Reference counting with a cycle collector

On the VM, `JsValue::Object` becomes a counted handle, across code that clones values freely. Every emitter adds an increment or a decrement at each copy, store, overwrite, scope exit and exception return. Cycles are common (`C.prototype.constructor === C`, and a recursive closure whose environment holds itself), so B still needs a tracing collector for them. A missed decrement leaks and an extra one dangles.

Rejected. It costs the most and still needs C.

### C. Non-moving mark-sweep (recommended)

Trace from precise roots on the VM. In native code, trace the heap precisely and scan the native stack conservatively. No emitter changes, because the compiler proves nothing. That decides it: precise native stack maps would need Cranelift stack maps, LLVM statepoints and changes to the self-hosted `.ot` emitter.

- **Native allocator.** Size-class free lists inside the existing chunks, and an object-start bitmap per chunk. Cells are zeroed, because `ot_alloc_literal` returns objects whose slots compiled code fills in later.
- **Native tracing.** Follow NaN-boxed slots, `proto`, array elements, function `env` and async frames.
- **Native roots.** The runtime tables (`GLOBALS`, `GLOBAL_SLOTS`, `PROMISES`, `MICROTASKS`, `TIMERS`, `ALLS`, `PENDING`, `ERROR_PROTOS`), plus a conservative scan from the stack pointer to the base that `ot_stack_init` records, and the spilled callee-saved registers. A candidate word is a boxed `QNAN|addr` or a raw address. A chunk lookup and the bitmap validate it and resolve interior pointers. JIT string literals that compiled code embeds as constants stay marked forever.
- **Native trigger.** Only on entry to the allocation stubs that compiled code calls. Allocations inside the runtime never collect.
- **Risks.** A missed root is a use-after-free. `OITE_GC_STRESS`, poisoned slots and a stress run of the golden suite catch it. The conservative scan can keep dead objects alive, but only a bounded number. Compiled code might keep a raw `elements` pointer across a call, so `src/backend/*/arrays.rs` needs an audit.
- **Cost.** Amortized O(live objects) per collection, with the threshold at max(live, floor).

An arena per call or task was also rejected: it needs write barriers in every emitter to promote values that escape.

## Phase 1: the VM collector (implemented)

`src/vm/gc.rs`. Every VM allocation goes through `VM::gc_alloc`, which reuses a freed slot before the heap grows. A collection marks from the roots, then poisons each unmarked slot (an empty `ByteStream`, which drops what it held) and puts it on the free list. Objects never move, so a slot index stays valid while the object is live.

The collector runs only between instructions, and only while no Rust frame has re-entered the VM. `run_until_return_sentinel` (used by `call_sync` and `resume_async`) and `execute_module` count re-entries in `gc.depth`. A Rust frame that re-enters holds values in locals the collector cannot see. Two programs show it, and both are golden tests now:

- `a + { valueOf() {...} }`: the right operand lives in a Rust local while `a.valueOf()` runs (`tests/lang/gc/callback_operand.ot`).
- `.then(f).then(g)`: while `f` runs, the promise it resolves lives only in a Rust local, and that promise holds `g` (`tests/lang/gc/then_chain.ot`).

With the depth check removed, both fail under `OITE_GC_STRESS`. The first prints `string 1[object Object]` instead of `number 3`, and the second throws a `TypeError`. The check has a cost. Code inside a timer callback, a promise reaction, a resumed async function or a module body does not collect until control is back at the top level.

The module comment in `gc.rs` lists the roots. `mark_roots` destructures every `VM` field by name, so a new field does not compile until someone classifies it as a root or not.

A collection runs once the allocations since the last one reach max(live objects, 100,000). `OITE_GC_STRESS=1` lowers that to one allocation. The `golden_gc_stress` test in `tests/golden.rs` runs every VM golden program that way. In `tests/lang/gc/roots.ot` and `unhandled_reason.ot`, each case keeps a value alive through one root only. `tests/memory.rs` checks the peak RSS of the #123 loop.

Four roots cannot be reached by a collection today, so no test can isolate them. The compiler emits no `StoreLocal`, so `indexed_locals` stays empty. Dynamic `import()` is unsupported, so `module_cache` stays empty. The task queue and an unfinished `Promise.all` are non-empty only while a callback runs, at depth 1 or more. They stay roots because phase 2 collects there.

## Phases

Each phase lands alone and ends in a check.

1. VM mark-sweep, collecting at depth 0. Check: the #123 loop on the VM stays under 64 MB at 3 and 12 million objects, the golden suite passes normally and under stress, and JIT and AOT are unchanged.
2. A pin stack for the values Rust holds across a re-entry (the `call_value` and `call_sync` sites, async resume, the saved stack in `execute_module`), then remove the depth check. Check: the #123 loop inside `setTimeout` stays under 64 MB, and the two callback tests pass under stress.
3. The native allocator: size classes, the bitmap, zeroed cells, and finalizers for `elements`, overflow slots and dictionaries. It does not collect yet. Check: golden passes on JIT and AOT, an allocation microbenchmark stays within 5%, and RSS is unchanged.
4. The native collector: roots, the conservative stack scan and the stub trigger. Check: the #123 loop on JIT and AOT at 3 and 12 million objects stays under idle RSS plus 64 MB, and golden passes on JIT and AOT under stress.
5. Async frames and promise ids under the collector. Check: 1 million `await`s stay flat on all three backends.
6. Optional escape analysis in the IR, with scalar replacement. Check: the #123 loop allocates no objects on AOT.

## Decision needed

Adopting C changes what the docs promise. Two claims become false:

- `README.md`: "Variables are freed when their scope ends".
- `docs/docs/ecmascript-conformance.md`: "No GC — deterministic memory via ownership", and "ownership-based memory management (no garbage collector)".

Under C they would say that a tracing collector reclaims memory, and that the borrow checker is a lint against use after a move. Phase 1 changes neither claim.
