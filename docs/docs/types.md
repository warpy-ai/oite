---
sidebar_position: 10
title: Types
description: Phase 3 of the performance plan - type annotations become checked contracts, and compiled code uses them to drop its type checks.
keywords: [performance, types, type checker, inference, unboxed, specialization]
---

# Types (performance phase 3)

> **Status:** design agreed; step 2 (the checker, as warnings) is
> implemented. It is
> phase 3 of [Performance](performance.md) and closes #124. Agreed:
> annotations are contracts and unannotated code stays dynamic; `null`
> and `undefined` are not part of other types; `as` is checked; the
> checker reports warnings first.

## Why

After phase 2 ([Object model](object-model.md)) the benchmarks make almost
no runtime calls. What is left is compiled code checking types
(native `--release`):

| Benchmark | Oite | Bun | Where the time is |
| --------- | ---: | --: | ----------------- |
| `nbody` | 577 ms | 95 ms | 99% compiled code: about 1,000 instructions for 18 floating-point operations |
| `methods` | 78 ms | 12 ms | 100% compiled code: 150 instructions for `c.inc(1)` |
| `props` | 65 ms | 19 ms | compiled code |
| `calls` | 54 ms | 22 ms | arguments boxed into an array for every call |

Every value is a boxed 64-bit word. `a + b` checks that both are numbers
and has a path that calls the runtime; every call packs its arguments into
an array; every property read checks that the receiver is a pointer, an
object, and of the cached shape.

**Goal of phase 3:** code whose types the compiler knows runs without
those checks: numbers in floating-point registers, arguments in registers,
calls direct. `props`, `methods`, `nbody` and `closures` within 1.5 times
Bun's time; `fib` and `calls` ahead of Bun.

## How types work today

- **Annotations are parsed and dropped.** `let s: string = 42` compiles
  and runs (#124). Interfaces, type aliases, `as` and generics are erased
  in `src/compiler/mod.rs`.
- **The compiler's intermediate form (IR) is built from bytecode**, not
  from the syntax tree, and bytecode has no types. Every parameter, local
  and return value is `any` in the IR.
- **One pass infers types inside a function** (`src/ir/typecheck.rs`),
  from constants and operators only. It never rejects a program. It does
  not see parameters, calls, return values or properties, so a loop
  counter is a number but `fib(n)` is not.
- **A known type only removes a check.** The value stays a boxed word.
- **`src/types/` (about 1,900 lines of checker and inference) is never
  called.** Only its `Type` enum is used, by the borrow checker.
- **The self-hosted compiler (`compiler/`) has its own structural
  checker** in Oite (`compiler/passes/types.ot`). `oitec build` does not
  use it.

## The design

### 1. What a type means

**An annotation is a contract. No annotation means dynamic.**

- `x: number` promises that `x` holds a number whenever it is read. The
  compiler checks it, and compiled code relies on it.
- A variable, parameter or return value with no annotation is not
  checked. `let x = 0; x = "a"` stays legal. The compiler still infers
  what it can for speed (sections 5 and 6), but inference never rejects a
  program.
- `any` is the explicit dynamic type. A value of type `any` can be used
  anywhere, and is checked when it enters a typed place (section 3).

This is the difference from TypeScript: there, an unannotated `let x = 0`
is a `number` and a wrong annotation is only a lint. Here an annotation is
either true at run time or the program stops.

### 2. The type language

First version:

| Type | Notes |
| ---- | ----- |
| `number`, `string`, `boolean` | |
| `null`, `undefined`, `void` | **Not** part of other types: `number` is never `null`. Write `number \| null`. |
| `any` | Dynamic. `unknown` is `any` that must be narrowed before use. |
| `T[]`, `Array<T>` | |
| Class names | Nominal: an instance of the class or a subclass. |
| Object types and interfaces | Structural: has these properties with these types. |
| Function types | `(a: number) => string` |
| Unions | `A \| B`, narrowed by `typeof`, `=== null`, `instanceof`, truthiness. |
| Optional | `x?: T` is `T \| undefined`. |
| Type aliases, enums | Enums already exist at run time. |
| Generics | Checked; erased to `any` in compiled code in the first version. |

Not in the first version: intersections, conditional and mapped types,
overloads, tuples. They are reported as "not supported yet", not silently
accepted.

### 3. Soundness: checks where dynamic meets typed

A contract is only useful to the compiler if nothing can break it. Fully
static soundness is not possible while `any` exists, so the rule is:

**A value is checked when it moves from a dynamic place to a typed one.**

| Where | Check |
| ----- | ----- |
| `any` assigned or passed to a typed variable, parameter or return | The value's kind (number, string, boolean, null, undefined, array, function, object) or class |
| A typed function called from dynamic code | Its parameters, at a checked entry point (section 5) |
| A read from the heap in typed code (`p.x`, `a[i]`) | The tag of the value read |
| `x as T` | The same check as an assignment. It is no longer free. |

A failed check throws a `TypeError` that names the expected and actual
types and the source position.

The third row is what keeps this simple. Objects and arrays can be
reached from dynamic code, which may store anything in them. So typed code
does not trust the heap: it checks what it reads. That is one compare and
a branch that is never taken, against today's check on every operand of
every operator plus a runtime path. Between reads, values are trusted and
unboxed.

Structural types (object types, interfaces) and element types of arrays
are checked statically where both sides are typed, and at reads otherwise.
They are not checked deeply at the boundary: passing an `any` to a
parameter of type `{ x: number }` checks that it is an object, and `p.x`
checks the number when it is read.

Section 7 removes the heap-read check for class fields later.

#### What is checked today (step 4)

The checks are **off by default**. `oitec --strict-types <command>` (or
`OITE_STRICT_TYPES=1`) turns them on, and makes what the checker reports
an error. They are opt-in because a program that passes a wrong value through
`any` runs today and would stop.

The checker records where a check is needed
(`src/types/check/guards.rs`), and the compiler emits it as one
intrinsic, `check_type(value, kinds)`, so the VM, the JIT and native
binaries run the same check. In compiled code it is an instruction of
its own (`IrOp::CheckType`): a few compares and a branch, with a call to
the runtime only when the check fails or the type is a class (step 5).

| Where | Checked |
| ----- | ------- |
| A parameter with a type | On entry, after its default value is applied. Always: the caller may be dynamic code. |
| An `any` stored in a typed variable or property, or returned from a function with a declared result | The value stored or returned |
| `x as T`, `<T>x` | Unless the checker knows `x` is a `T` |
| `x!` | That `x` is not `null` or `undefined` |
| `p.x`, `a[i]`, `p?.x` read from an object or an array whose type is known | The value read, against the declared type of the property or element |
| The result of a call | Unless the callee is a function declaration, a constant whose initialiser is a function, or a method of a string or a number: anything else can be replaced by dynamic code |

A check tests the **kind** of the value: number, string, boolean, `null`,
`undefined`, array, function, or another object. A failed check throws
`TypeError: Expected number | null, got string` at the expression, or at
the parameter inside the called function.

Not yet, each a follow-up:

- **A class type is checked as "an object".** Instances do not carry
  their class in a form a check can test cheaply until class layouts
  (section 7). Step 8 makes this check exact.
- **A string literal type is checked as "a string"**, and a tuple as "an
  array".
- **`for (const x: T of xs)`** and compound assignment to a property
  (`p.x += v`) are not checked.
- **What control flow established about a property** (`if (p.x !== null)`)
  is not trusted across a call by the run-time check, but still is by
  the checker. Step 5 must not rely on it; neither may it rely on a
  narrowed variable that a closure assigns.
- **Existing code that fails with the checks on**, to fix before they
  can be the default: `bootstrap/codegen.ot` passes a `null` register as
  a `number`, and the `json` roll's `stringify` is called with a `null`
  replacer, which its type does not allow.

### 4. The checker

- **Runs on the syntax tree**, before code generation, next to the borrow
  checker, for every backend and for `oitec check`.
- **Local inference, in one direction.** Declared types flow down into
  expressions; expression types flow up. No global inference, no
  unification variables. An unannotated variable has the type `any` to
  the checker.
- **Narrowing** by control flow, as in section 2.
- **Errors use the existing diagnostics** (file, line, column, snippet).
- **Its output is a type table**: for each function, the types of its
  parameters and return value; for each typed local and each class, its
  declared types. Lowering to IR reads this table (the bytecode stays
  untyped; the table is keyed by function address and source site, which
  bytecode already carries for stack traces).
- **`src/types/`:** the checker is in `src/types/check/`, with its own
  representation of types (`ty.rs`). The old `Type` enum stays for the
  borrow checker, which is its only user. The unused `checker.rs`,
  `inference.rs` and `convert.rs` are removed.

#### What the checker does today (step 2)

It reports, as warnings on `build`, `jit` and a plain run, and as errors
on `oitec check`:

- a value of a known type stored where another type is declared: a
  variable, a property, an array element, a class field, a `return`;
- an argument that does not fit a parameter, and a call that leaves out
  an argument for a typed parameter;
- a call of something that is a number, a string, a boolean or an array;
- a type it does not model (tuples, intersections, conditional and
  mapped types), which it treats as `any`.

It narrows by `typeof`, comparison with `null` and `undefined`,
truthiness, `instanceof`, `Array.isArray`, `&&`, `||`, `!`, `?:`, `??`,
early `return`/`throw`, and assignment. Warnings in the files of a roll
a program imports are not shown when compiling that program.

A module sees the types of what it imports from another module of the
program: functions, classes, constants, namespaces (`import * as`), and
interfaces and aliases imported with `import type`.

Not yet, each a follow-up:

- **Generics are not checked.** A type parameter is `any`.
- **A property that a type does not declare is `any`**, not an error.
- **An array literal is accepted as a tuple** when each element could be
  one of the tuple's elements, whatever its position: `[1, 2]` passes as
  a `[string, number]`.
- **Intersection and conditional types** are reported as not supported
  and are `any`. A mapped type is an object whose properties all have
  the mapped type.
- **The type table for code generation** is built in step 5, when
  lowering first reads it.

### 5. Typed code generation

With types in the IR:

- **Unboxed numbers.** A value of type `number` is an `f64` in the IR and
  in a floating-point register. `a + b`, `a < b` and `Math.sqrt(a)` are
  one instruction each, with no check and no runtime path. A number is
  boxed only when it enters a dynamic place (an `any`, the heap, a
  dynamic call); boxing a number is a bit copy.
- **Booleans** are unboxed the same way.
- **Two entry points per typed function:**
  - a **typed entry**, with unboxed numbers and booleans in registers and
    no argument array, used by direct calls from code that knows the
    function and has checked types;
  - the **dynamic entry** (`this, argv, argc, env`, as today), which
    checks the arguments, then calls the typed entry. Method calls
    through caches and calls from dynamic code use it.
- **Integers stay internal.** A `number` proven to be an integer in range
  (loop counters, array indexes) is kept as an integer, as in phase 1.
  There is no integer type in the language.
- **Strings, objects and arrays** stay pointers. Knowing the type removes
  the "is it a pointer of this kind" checks on them.

#### What compiled code relies on today (step 5)

With `--strict-types`, the checker also records what it has **proved**
about a value, and the compiler passes it on as `assume_type(value,
kinds)`. Lowering to IR turns that into the type of the value and emits
nothing; the VM runs it as a check, so a mistake in the checker shows up
as an error there instead of a wrong result in a binary.

| Proved | Because |
| ------ | ------- |
| A read of a parameter declared `number`, `boolean` or `string` | It was checked on entry |
| A read of a `let` or `const` declared with one of those types and an initialiser | Every store to it is checked |
| The result of calling a function declaration, or a constant whose initialiser is a function, declared to return one of those types | Its `return`s are checked |

The declared type is used, not what control flow narrowed the variable
to, and a `var` is left out (it exists, as `undefined`, before its
initialiser runs).

The optimizer removes a check of a value that is already known to have
the type, but never on the strength of what that same check proved.

Typed programs (`benches/perf/typed/`, `benches/perf/run.sh --typed`),
native `--release`, the same source with and without `--strict-types`:

| Benchmark | Instructions, strict against not | Strict | Not strict | Bun |
| --------- | -------------------------------: | -----: | ---------: | --: |
| `calls` | −39% | 47 ms | 55 ms | 19 ms |
| `fib(38)` | −9% | 352 ms | 335 ms | 216 ms |
| `nbody` | −5% | 483 ms | 519 ms | 96 ms |
| `loop_local` | not counted | 511 ms | 500 ms | 1,773 ms |

Times are the best of 7 on a machine whose noise is about 15%; the
instruction counts (callgrind) are exact. `fib` runs fewer instructions
but not less time, which is not explained yet.

Step 5 removes the type tests around arithmetic, which lets LLVM inline
small typed functions (`calls`). What these programs still spend their
time on is the call itself (`fib`: step 6) and property access (`nbody`:
step 8).

#### Direct calls today (step 6, first part)

Every function has the dynamic entry `(this, argv, argc, env)`, which is
what a function object holds. A function that never looks at its
argument array (no rest parameter, not async, at most 8 parameters) also
has a **fast entry**, `(this, arg0 .. argN, env)`, and its code is
compiled into that one; the dynamic entry unpacks the array and calls
it. A call that knows which function it calls (a function declaration, a
closure created in the same function) calls the fast entry: no argument
array, a missing argument passed as `undefined`, an extra one dropped.
This needs no types and no `--strict-types`. It is in the LLVM backend
(`src/backend/llvm/codegen/fast.rs`); the JIT still passes the array.

A function that calls nothing that can run code has no stack-overflow
check: it cannot be what exhausts the stack.

Native `--release`, best of 5, before and after:

| Benchmark | Before | After | Bun |
| --------- | -----: | ----: | --: |
| `fib` | 22 ms | 19 ms | 19 ms |
| `fib(38)` | 338 ms (7.46 G instructions) | 285 ms (5.12 G) | 215 ms |
| `binary_trees` | 355 ms | 340 ms | 218 ms |
| typed `fib`, `--strict-types` | 23 ms | 18 ms | 19 ms |
| typed `fib(38)`, `--strict-types` | 357 ms (6.77 G) | 281 ms (4.11 G) | 215 ms |

The other benchmarks do not change (their calls were already inlined, or
they do not call).

One case gets slower, in strict mode only: typed `calls` goes from 47 ms
to 66 ms, with the same number of instructions (460 M before, 440 M
after). Its loop inlines `add` together with `add`'s parameter checks,
and the code LLVM now produces for that loop is slower for a reason not
identified. The second part of this step removes those checks from such
a call (the caller has proved the types), which removes the cause.

### 6. Speed for code without annotations

Most existing Oite code has no annotations (5 of 121 test files, 1 of 16
benchmarks), and a program must not need them to be fast.

- **Local inference in the IR** (the existing pass), extended with the
  type table: results of typed calls, typed properties, and numbers read
  from the heap after their check.
- **Specialisation by argument types.** Oite compiles a whole program at
  once. When a known function with unannotated parameters is called
  directly with arguments whose types are known, the compiler makes a
  copy of the function for those types (`fib` for a `number`), with a
  typed entry, and calls the copy. The original stays for dynamic calls.
  The IR already has an unused operation for this (`CallMono`). Copies
  are limited per function, so code size is bounded.
- **Return types are inferred** for a specialised copy from its body, so
  `fib(n - 1) + fib(n - 2)` is an `f64` addition.

So `function fib(n) { ... }` called as `fib(32)` compiles to the same code
as `function fib(n: number): number`. The annotation's value is the
contract and the error message, not the speed.

### 7. Objects (after the first version)

- **Declared class fields have fixed slots.** A field declaration
  (`x: number;` in a class body) already defines the property on every
  instance before the constructor runs, in declaration order, parent
  class first. So field `i` is in slot `i` of every instance. Typed code
  reads `p.x` on a `p: Point` with one load: no cache, no shape check.
- **This needs one rule:** a declared field cannot be deleted. `delete
  p.x` on a declared field is a compile error where the type is known,
  and a `TypeError` otherwise. (JavaScript allows it.)
- **Typed fields checked on write.** When every store to a declared field
  is checked (typed code statically, dynamic code in the runtime), a read
  needs no check at all.
- **Methods called directly** when the receiver's class is known and the
  method is not overridden in the program.
- **Inlining** of small known functions, which the typed entry makes
  possible.

## Effect on the rest of the system

- **Language:** three changes.
  1. A program with a wrong annotation no longer compiles (or stops with
     a `TypeError` where the value came from dynamic code).
  2. `null` and `undefined` are not members of other types.
  3. `as` is a checked conversion.
  Code with no annotations behaves exactly as before.
- **Existing code:**
  - `tests/lang`: almost no annotations; expected to pass unchanged.
  - **The rolls** are heavily annotated (all 13 source files; 64 unions,
    52 `any`). They must pass the checker before it can reject programs.
  - **The self-hosted compiler** (`compiler/`, about 1,000 annotations,
    many nullable values typed as non-null) will need fixes or `any`.
- **Rollout:** the checker first reports **warnings** on `build` and
  `run`, and errors on `oitec check`. When the rolls, the self-hosted
  compiler and the tests are clean, warnings become errors. Typed code
  generation only relies on annotations once they are errors.
- **The VM:** runs the same checks (the checker is in the front end; the
  boundary checks become bytecode). It stays a boxed interpreter.
- **Rolls:** no API change. They get faster where they are annotated,
  which is everywhere.
- **Phase 4 (memory):** typed locals and known object types are what
  escape analysis and ownership need; this phase gives it that
  information.

## Code organisation

Under the file-size rule (about 800 lines), the files this phase touches
are split first, each in its own pull request with no behaviour change:
`src/compiler/mod.rs`, `src/ir/lower.rs`, and the two backends
(`cranelift/` and `llvm/codegen/`): all are split.

New code:

- `src/types/check/`: `mod.rs`, `ty.rs`, `resolve.rs`, `assign.rs`
  (assignability), `classes.rs`, `functions.rs`, `stmt.rs`, `expr.rs`,
  `narrow.rs`, `flow.rs`, `builtins.rs`; later `table.rs` (the type
  table).
- `src/ir/spec.rs`: specialisation.
- `src/backend/*/typed.rs`: typed entries and unboxed values.

## Order of work

Each step is one pull request, with tests on vm, jit and aot.

1. **Split the large files** (several pull requests, no behaviour change).
2. **Done: the checker**, reporting warnings: types of section 2, local
   inference and narrowing. `oitec check` reports errors. Tests for
   accepted and rejected programs are in `tests/types/`
   (`tests/type_check.rs`). What it leaves for later is listed in
   section 4.
3. **Make the rolls and the self-hosted compiler pass**, then turn
   warnings into errors. Closes #124. Done so far: the checker finds
   nothing in the repository's Oite code or in the core rolls, and a test
   keeps it so (`existing_code_has_no_findings`). Still to do: turn the
   warnings into errors, after a release has shipped them as warnings.
4. **Done, behind `--strict-types`: boundary checks** (section 3) on
   every backend. They become the default together with errors (step 3),
   once the code listed in section 3 passes with them on.
5. **Done, behind `--strict-types`: types in the IR.** A check is
   compiled inline, and what the checker proved reaches the IR, so
   arithmetic and comparisons on typed numbers have no type tests (see
   "What compiled code relies on today" in section 5). A number is
   already stored as its own bits, so there was no box to remove: the
   cost was the tests.
6. **Typed entries and direct typed calls.** Done: a fast entry with
   the arguments in registers, for every direct call, typed or not (see
   "Direct calls today" in section 5). Still to do: an entry that skips
   the parameter checks when the caller has proved the argument types.
7. **Specialisation** of unannotated functions.
8. **Class field layout and typed field access** (section 7), then
   direct method calls and inlining.

Expected effect, as estimates to be checked by each step's benchmarks:
steps 5 to 7 are for `fib`, `calls`, `nbody` and the loops; step 8 is for
`props` and `methods`. Steps 2 to 4 change no timings.

## Risks

- **It is a language change.** Existing annotated code may stop compiling.
  The warning period and `any` are the mitigation; the rule that
  unannotated code is untouched limits it to code that made a promise.
- **Checked `as` and heap-read checks cost time** in code that crosses
  between typed and dynamic often. The alternative, trusting annotations,
  makes a wrong annotation a crash instead of an error.
- **Two representations of a value** (boxed and unboxed) in the IR, in two
  backends, with exceptions and resumable `async` functions. This is the
  largest piece of engineering in the phase; step 5 does it inside one
  function before step 6 crosses calls.
- **Code size** from specialisation, bounded by the per-function limit.
- **Two checkers**: this one in Rust and `compiler/passes/types.ot` in the
  self-hosted compiler. They will disagree until the self-hosted compiler
  is brought in line; `oitec build` uses only the Rust one.
