---
sidebar_position: 8
title: Performance
description: The goal of being faster than Bun, where native Oite stands today, why, and the changes that close the gap.
keywords: [performance, benchmarks, bun, native, llvm, types, objects, memory]
---

# Performance: faster than Bun

> **Status:** proposal. The numbers below were measured on one machine
> (4-core Intel Xeon at 2.1 GHz, Linux x86_64) with
> `benches/perf/run.sh`. They are a baseline, not a promise.

## The goal

**Native Oite programs run faster than the same program under Bun.**
Native means `oitec build --release`; the VM is for tests and debugging and
is not part of the goal.

Bun runs JavaScript on JavaScriptCore: an interpreter, then up to three
JIT tiers that recompile hot code using the types they observed at run
time, with inline caches, hidden classes and a generational garbage
collector. It is one of the fastest JavaScript runtimes, so beating it
means beating a mature optimising JIT, and doing it ahead of time.

### How it is measured

- **The suite:** `benches/perf/*.ot`. Each program is valid TypeScript, so
  Bun and Node run the same file. The suite covers numeric loops,
  recursion, calls, closures, arrays, objects, classes, allocation and
  strings, plus two programs from the Computer Language Benchmarks Game
  (`nbody`, `binary_trees`) and process start (`startup`).
- **The runner:** `benches/perf/run.sh [--oitec PATH]... [--runs N]` builds
  each program with every given `oitec`, runs it, and runs Bun and Node.
  It reports the best wall time of N runs and checks that every runtime
  printed the same result.
- **Faster than Bun** means: on every benchmark in the suite, the native
  binary's best time is at most Bun's, with the same output, on the CI
  machine. Peak memory is tracked next to time: a program that is fast
  because it never frees memory does not count.
- **In CI:** a job runs the suite against the Bun version CI pins and
  reports the table. Once a benchmark beats Bun, a regression on it fails
  the build.

## Where Oite stands

Best of 3 runs, in milliseconds. *Before* is `oitec` before phase 1;
*now* is `master` with the first phase-1 changes (#156): number fast
paths, integer `%` and constant-time string indexing.

| Benchmark | What it stresses | Before | Now | Bun | Node | Now vs Bun |
| --------- | ---------------- | -----: | --: | --: | ---: | ---------: |
| `loop_local` | numbers and `%` in a function | 2,827 | 610 | 2,190 | 747 | **0.28×** |
| `startup` | starting a process | 4 | 5 | 8 | 61 | **0.6×** |
| `fib` | recursion | 69 | 31 | 25 | 83 | 1.2× |
| `strings` | concatenation, `.length` | 73 | 65 | 37 | 95 | 1.8× |
| `calls` | function calls | 150 | 66 | 24 | 74 | 2.8× |
| `sort` | quicksort with a comparator | 237 | 192 | 69 | 130 | 2.8× |
| `string_build` | `join`, `split` | 155 | 156 | 56 | 134 | 2.8× |
| `closures` | callbacks and captures | 269 | 152 | 53 | 162 | 2.9× |
| `loop_global` | the same loop at the top level | 10,775 | 8,005 | 2,193 | 756 | 3.7× |
| `array` | array reads and writes | 567 | 392 | 68 | 143 | 5.8× |
| `bytes` | `charCodeAt` over a string | 73,200 | 172 | 22 | 103 | 7.8× |
| `props` | object properties | 454 | 421 | 28 | 97 | 15× |
| `binary_trees` | allocation, recursion | 4,184 | 4,594 | 306 | 390 | 15× |
| `methods` | method calls on a class | 412 | 353 | 17 | 81 | 21× |
| `nbody` | floating point on objects | 5,190 | 4,131 | 118 | 151 | 35× |
| `alloc` | small short-lived objects | 1,201 | 1,187 | 22 | 77 | 54× |

Bun 1.4.4 with its default options (`run.sh` clears `BUN_OPTIONS`); Node
22.23 with `--experimental-strip-types`. `binary_trees` varies by about 20%
between runs because of its memory use.

Peak memory (MB):

| Benchmark | Oite | Bun | Node |
| --------- | ---- | --- | ---- |
| `alloc` | 1,260 | 29 | 64 |
| `binary_trees` | 3,775 | 50 | 111 |
| `string_build` | 48 | 50 | 106 |
| `closures` | 33 | 47 | 93 |
| `nbody` | 12 | 22 | 65 |
| `startup` | 12 | 12 | 56 |

What this says:

- **Oite beats Bun on numeric loops and start-up.** With integer `%`,
  `loop_local` runs in a quarter of Bun's time, and a process starts in
  half Bun's time.
- **Plain computation is close:** recursion, calls, closures and simple
  strings are 1.2 to 2.9 times Bun's time.
- **Everything that touches objects, methods or memory is 15 to 54 times
  slower**, and allocation-heavy programs never give memory back.

## Why: the causes, by cost

Each cause was found by reading the IR (`oitec ir`) and confirmed by a
benchmark that isolates it.

1. **Untyped values go through the runtime.** The compiler rarely knows
   that a value is a number, so `a + b`, `a * b` and `a < b` compiled to
   calls into the runtime (`ot_add_any`, ...). The runtime is shipped as
   precompiled machine code, so neither `--release` nor `--dist` (LTO) can
   inline those calls.
   *Fixed in part by #156, which tests both operands inline.*
2. **Objects are lists of named properties.** A native object is a header
   plus a separate `Vec<(String, value)>`: every property name is its own
   heap string, and every `o.x` is a linear search comparing strings. A
   two-field object literal costs about four allocations. `nbody`'s inner
   loop makes 34 such lookups per iteration; Bun makes 34 loads from known
   offsets.
3. **Nothing is freed.** There is no memory reclamation (#123), so
   allocation-heavy programs grow without bound, and every allocation is a
   `malloc` that also costs cache misses.
4. **Top-level variables are hash-table entries.** A top-level `let` is a
   global looked up by name on every access: the same loop is 3.5 times
   slower at the top level than inside a function.
5. **Calls and method calls are dynamic.** Every call goes through a
   function object with an argument array, and a method call looks the
   method up by name on every call, even when the callee is a known
   function.
6. **No integer arithmetic.** Numbers are always doubles, so `%` was a
   call to `fmod`: the `loop_local` benchmark spent about 90% of its time
   there. *`%` is fixed by #156; the bitwise operators remain.*
7. **Built-ins are reached through objects.** `Math.sqrt(x)` loads the
   global `Math`, looks up `sqrt` by name, and calls it, instead of
   emitting one `sqrtsd` instruction.
8. **String indexing copies the string.** The shared string methods turn
   the receiver into a vector of characters on every call, so
   `s.charCodeAt(i)`, `s.charAt(i)` and `s[i]` cost time proportional to
   `s.length`, and a loop over a string is quadratic. *Fixed by #156:
   ASCII strings are flagged when created and indexed directly.*

## How an ahead-of-time compiler beats a JIT

A JIT is fast because it **watches** the program: it sees that `p.x` is
always read from objects of one shape and that `a + b` always adds numbers,
and it recompiles for exactly that. An ahead-of-time compiler cannot watch,
so it has to **know**. Oite can know far more than JavaScript does,
because it has types:

- **Types are the AOT compiler's type feedback.** An Oite program declares
  `x: number` and `class Point { x: number; y: number }`. When those
  declarations are checked (#124) and inferred where missing, the compiler
  can emit unboxed doubles and integers, fixed object layouts and direct
  calls: what a JIT reaches only after warming up, and without the guards
  it needs in case it guessed wrong.
- **Whole-program compilation.** Modules are linked at compile time, so the
  compiler sees every function: it can inline across modules and drop
  unused code, which a JIT does only for hot code.
- **No warm-up and no deoptimisation.** A native binary is fast from its
  first instruction. That is already visible in `startup`.
- **Deterministic memory.** Ownership-based reclamation (#123) frees
  memory at known points, without a collector's pauses or its memory
  overhead.

Where Oite cannot know, it must still be competitive: code typed `any`
keeps the dynamic path, made cheap with inline type tests and inline
caches, the techniques a JIT uses.

## The changes

Ordered by how much of the gap each closes per unit of work. Each is a
separate pull request that adds or updates benchmarks.

### Phase 1: the dynamic path (weeks)

| Change | Closes | Expected effect |
| ------ | ------ | --------------- |
| **Done (#156): inline number fast paths** for arithmetic and comparisons on untyped values, in both native backends. | Cause 1 | Measured: `fib` and `calls` 2.2 times faster, `closures` 1.8, `array` and `nbody` 1.3. |
| **Done (#156): constant-time string indexing:** `charCodeAt`, `charAt` and `at` read an ASCII string's byte directly. | Cause 8 | Measured: `bytes` from 73 s to 172 ms. |
| **Integer fast path for `%`** (done, #156) and for `|`, `&`, `<<`, `>>` (to do) when both operands are integers. | Cause 6 | Measured for `%`: `loop_local` 4.6 times faster, ahead of Bun. |
| **Math as intrinsics in the generated code:** `Math.sqrt` becomes `llvm.sqrt`, `floor` becomes `llvm.floor`, and so on, when `Math` is not redefined. | Cause 7 | Part of `nbody`. |
| **Top-level variables as fixed slots** in a module's data, not hash entries. | Cause 4 | `loop_global` close to `loop_local`. |
| **Runtime helpers that can be inlined:** compile the hot runtime helpers (tag tests, array element access, string length) to LLVM bitcode linked before optimisation, or emit them inline. | Cause 1 | Every remaining runtime call on a hot path gets cheaper. |

### Phase 2: objects (1 to 2 months)

| Change | Closes | Expected effect |
| ------ | ------ | --------------- |
| **Shapes (hidden classes):** an object is a header, a pointer to its shape and its property values inline. A shape maps names to slot numbers; adding a property moves the object to the next shape. Property names are interned once. | Cause 2 | One allocation per object; no per-property strings. |
| **Inline caches** at every property read, write and method call: remember the last shape and slot, and test the shape before the next access. A hit is one compare and one load. | Causes 2, 5 | `props`, `methods`, `nbody` within reach of Bun. |
| **Classes get their shape at compile time** from their field declarations. | Causes 2, 5 | Field access on class instances needs no cache. |
| **Arrays inline in generated code:** bounds check, then a load or store; elements stored unboxed when every element is a number. | Cause 2 | `array`, `sort`, `closures`. |

### Phase 3: types (2 to 3 months)

| Change | Closes | Expected effect |
| ------ | ------ | --------------- |
| **A checked type system (#124):** annotations are checked, and an annotated `number`, `string`, `boolean`, array or class type is a guarantee the compiler can rely on. `any` and unannotated code that cannot be inferred stay dynamic. | Causes 1, 2, 5 | Typed code needs no guards. |
| **Inference** for locals, parameters and return values from their uses. | Causes 1, 6 | Unannotated numeric code runs unboxed. |
| **Typed code generation:** unboxed `f64` and `i32`/`i64` values in registers, fixed layouts for typed objects, direct calls to known functions, and inlining (`CallMono` in the IR is the start). | Causes 1, 5, 6 | Numeric code at C speed. |

### Phase 4: memory (2 to 3 months, designed first)

| Change | Closes | Expected effect |
| ------ | ------ | --------------- |
| **Escape analysis:** an object that does not outlive its function is allocated on the stack, or not at all (its fields become locals). | Cause 3 | `alloc` makes no heap allocations. |
| **Ownership-based reclamation (#123):** objects are freed when their owner goes away, at points the compiler knows, as designed before it is built. | Cause 3 | `binary_trees` memory bounded; peak memory below Bun's. |
| **A fast allocator:** size classes and thread-local free lists instead of `malloc`. | Cause 3 | Allocation-heavy programs. |

### Phase 5: strings and the rolls

| Change | Expected effect |
| ------ | --------------- |
| **Byte-level string primitives and a byte buffer type**, so `@rolls/string` is written in Oite as linear byte loops (prototype: within 1.5 times of Rust for `indexOf`, `slice`, `split`, `trim`). | The string methods move into the rolls without getting slower. |
| **String building:** a builder (or ropes) so repeated `+=` is linear. | `string_build`. |
| **SIMD searching** in the byte primitives that need it (`indexOf`). | String-heavy programs. |

## What this changes in the language

These need a decision before the phase that depends on them:

1. **Types become meaningful (phase 3).** Today type annotations are
   parsed and ignored. Checking them means a program that passes the
   wrong type no longer compiles. That is the point, but it is a language
   change: existing Oite code may need fixes, and `any` remains the escape
   hatch.
2. **Integers (phases 1 and 3).** Oite numbers keep JavaScript's semantics
   (a double), and integers are an internal representation used when the
   compiler can prove the value is an integer in range. No new number
   types are proposed.
3. **The object model (phase 2).** Shapes change the runtime's object
   layout and the ABI between native code and the runtime. Objects that
   are used as dictionaries (many dynamic keys, `delete`) fall back to a
   hash table.
4. **Benchmarks gate changes.** A pull request that makes a benchmark that
   beats Bun slower than Bun fails CI.

## Risks

- **A JIT is very good at long-running dynamic code.** Code that is
  genuinely dynamic (`any` everywhere, objects of many shapes) may stay
  slower than Bun even after phase 2. The plan's answer is types: the more
  of a program the compiler can type, the more of it beats Bun.
- **Phases 3 and 4 are large.** Each needs a written design before code,
  as #123 already requires.
- **The suite can be gamed.** It should grow with real programs (the
  self-hosted compiler, Unroll, the rolls), not only micro-benchmarks.

## Order of work

1. The rest of phase 1: top-level variables as slots, `Math` as
   instructions, inlinable runtime helpers and the integer bitwise
   operators, with `benches/perf` in CI. (The number fast paths, integer
   `%` and string indexing are done, in #156.)
2. Phase 2's shapes and inline caches, then arrays.
3. The designs for phases 3 and 4, reviewed before either is built.
4. Phase 5 alongside the prelude work in
   [Platform Architecture](platform-architecture.md), so that string and
   array methods move into the rolls only once they are as fast as today's
   built-ins.
