---
sidebar_position: 7
title: Platform Architecture
description: How Oite is delivered - the core compiler and runtime, the Rolls system libraries, and Unroll - and the contracts between them.
keywords: [architecture, rolls, unroll, ffi, intrinsics, prelude, distribution]
---

# Platform Architecture: Core, Rolls and Unroll

> **Status:** design, agreed in outline. Sections marked **Open** need a
> decision. Everything under "Today" was checked against `oite`,
> [`warpy-ai/rolls`](https://github.com/warpy-ai/rolls) and
> [`warpy-ai/unroll`](https://github.com/warpy-ai/unroll) at the time of
> writing.

Oite ships as one toolchain: the `oitec` compiler, the `unroll` tool, and
the core Rolls. An application is built into a **single native binary**,
which contains the Oite runtime and only the Rolls it uses.

```
┌──────────────────────────────────────────────────────────────────────┐
│                         User application                             │
└──────────────────────────────────┬───────────────────────────────────┘
               ┌───────────────────┴────────────────────┐
               ▼                                        ▼
┌──────────────────────────────┐   ┌──────────────────────────────────┐
│          OITE CORE           │   │              ROLLS               │
│  Always there                │   │  System libraries, used as needed│
│                              │   │                                  │
│  • Compiler (oitec)          │   │  Oite code (the API)             │
│  • Runtime (NaN-boxing,      │   │   + native Rust (`__ffi_*`)      │
│    heap, exceptions)         │   │                                  │
│  • The language              │   │  @rolls/std (prelude), array,    │
│  • Intrinsics                │   │  string, math, json, async, fs,  │
│  • console.log               │   │  http, tls, crypto, ...          │
│                              │   │                                  │
│  Like C without libc         │   │  Batteries for real apps         │
└──────────────────────────────┘   └──────────────────────────────────┘
               └───────────────────┬────────────────────┘
                                   ▼
┌──────────────────────────────────────────────────────────────────────┐
│                   ./myapp — one binary, no runtime to install        │
└──────────────────────────────────────────────────────────────────────┘

Unroll: resolves and fetches Rolls, drives oitec, formats, lints, tests, LSP.
```

## Who owns what

| Layer | Owns | Does not own |
| ----- | ---- | ------------ |
| **Core** (`oitec`, this repo) | The compiler and its three backends (VM for debugging, Cranelift JIT, LLVM AOT). The runtime: values, heap, exceptions, stack traces. The language, including `async`/`await` and the promise machinery `await` runs on. `console.log`. The **intrinsics**. Module resolution and whole-program compilation. | Any library API: no `Math`, `Date`, `JSON`, timers, file system, string or array helpers. |
| **Roll, Oite part** (`rolls/<name>/src/mod.ot`) | The public API, written in Oite: `map`, `trim`, `PI`, `jsonParse`, `setTimeout`, `readFileText`, ... | Machine-level work: it calls intrinsics or its own FFI functions. |
| **Roll, native part** (`[native]` in `roll.toml`) | The roll's `__ffi_*` functions, in Rust, using the crates it declares (`rustls`, `httparse`, ...). | Anything another roll needs: each roll links only its own native code. |
| **Unroll** | Manifests and lockfiles, version resolution, the registry, the cache, building each used roll's native part, invoking `oitec`, `fmt`, `lint`, `test`, `doc`, the LSP. | Compiling Oite code: it hands the whole program to `oitec`. |

## What is delivered

One release contains:

```
$OITE_HOME/                      (default ~/.oite)
├── bin/
│   ├── oitec
│   └── unroll
├── rolls/                       core rolls, versioned with this release
│   ├── std/      roll.toml, src/mod.ot
│   ├── array/
│   ├── string/
│   ├── math/
│   └── json/
└── cache/rolls/<name>/<version>/   other rolls, fetched by unroll
```

**Core rolls** are bundled and versioned together with `oitec`, so the
intrinsics they call always match the compiler. Every other roll comes from
the registry (`registry.oite.org`) through `unroll`.

**Open: which rolls are core.** The proposal is `@rolls/std` plus what it
depends on: `array`, `string`, `math`, `json`. Should `@rolls/async` also
be bundled? It provides `setTimeout`, and most programs that use `await`
want it.

## The prelude

`@rolls/std` is imported into every program unless the program opts out:

- `unroll`: `no-std = true` under `[package]` in `unroll.toml` (unroll's
  resolver already injects `@rolls/std`).
- `oitec`: `--no-std`.

The prelude re-exports a curated set as **free functions**:
`map(numbers, n => n * 2)`, `trim(name)`, `sin(PI / 4)`, `jsonParse(text)`,
plus `Result`, `Option`, `println`, `assert`, and so on.

**Open: method calls.** Core today also has built-in methods on primitives
(`s.trim()`, `arr.slice()`, `n.toFixed(2)`), inherited from the VM. If free
functions are the intended style, those methods leave core with the rest of
the library code. If both styles are wanted, the methods should be thin
wrappers over the same roll functions, not a second implementation in core.

## Contract 1: intrinsics (core ↔ rolls)

An **intrinsic** is an operation the compiler and runtime provide because
it cannot be written in Oite, or not efficiently: a machine instruction
(`sqrt`), access to a value's representation (`string_length`), or a
conversion with exact rules (`to_fixed`). A roll declares it and calls it
like a function:

```javascript
declare function __intrinsic_sqrt(x: number): number;

export function sqrt(x: number): number {
    return __intrinsic_sqrt(x);
}
```

The compiler recognises `__intrinsic_*` names and emits the operation
inline (a native instruction where there is one, otherwise a runtime call),
on every backend. An unknown `__intrinsic_` name is a compile error.

The intrinsics the core rolls use today (28):

| Group | Intrinsics |
| ----- | ---------- |
| Math | `floor` `ceil` `round` `sqrt` `pow` `exp` `log` `sin` `cos` `tan` `asin` `acos` `atan` `atan2` `fround` `imul` `random` |
| Numbers | `parse_int` `parse_float` `to_fixed` `to_precision` `to_exponential` |
| Strings | `string_length` `char_at` `char_code_at` |
| Values | `is_array` |
| Host | `platform` `cwd` |

This list is the contract, so it is versioned with the release. Adding an
intrinsic is a minor change; changing or removing one is a breaking change.

**Open: host intrinsics.** `platform` and `cwd` (used by `@rolls/path`) ask
the operating system. They could be `__ffi_*` functions of a roll instead,
keeping core free of OS access beyond what `console.log` needs.

**Open: what else rolls may use from core.** Besides intrinsics, the rolls
currently use a few JavaScript built-ins directly:

- `.length` and `.push` on arrays;
- `String.fromCharCode`, `Object.keys`, `Array.isArray`;
- `parseInt`, `parseFloat`, `isNaN`;
- in `@rolls/json`, the string methods `.substring`, `.split` and `.join`.

`.length`, indexing and `.push` are language-level and stay in core. The
others should become intrinsics, or calls into sibling rolls
(`@rolls/json` using `@rolls/string`), so the set core must provide stays
exactly the intrinsics list.

## Contract 2: FFI (rolls ↔ their native code)

A roll's `__ffi_*` functions are implemented in Rust in that roll and
linked into the binary only when the roll is used:

```javascript
// @rolls/async, Oite part
declare function __ffi_set_timeout(callback: () => void, ms: number): number;

export function setTimeout(callback: () => void, ms: number): Timer {
    let id = __ffi_set_timeout(callback, ms);
    return { id, cancel: () => __ffi_clear_timeout(id) };
}
```

```rust
// @rolls/async, native part
#[no_mangle]
pub extern "C" fn __ffi_set_timeout(callback: OtValue, ms: OtValue) -> OtValue { ... }
```

What this needs (#132):

1. **A calling convention.** Arguments and the result are Oite values in
   the runtime's representation (NaN-boxed `u64`), the same as compiled Oite
   functions. A native function signals an exception the way the runtime
   does (`EXCEPTION_BITS`), so `try`/`catch` and stack traces work across the
   boundary.
2. **A runtime crate for native code.** Roll code in Rust needs the
   runtime's API: making strings and arrays, reading properties, calling an
   Oite function (a callback), throwing. That API is published from this
   repository as a crate (`oite-runtime`) that rolls depend on. It is the
   only part of core that roll native code may use.
3. **Compiling the declaration.** `declare function __ffi_x(...)` becomes a
   call to the external symbol `__ffi_x`:
   - AOT: a link-time symbol, resolved against the roll's static library;
   - JIT: a symbol looked up in the roll's library, loaded at start-up;
   - VM: a native function, from the same library loaded at run time.

   A program that calls an `__ffi_` function whose roll is not linked fails
   to build, naming the function and the roll.

The rolls declare 118 distinct `__ffi_*` functions, by roll: `async` (32),
`fs` (24), `crypto` (18), `tls` (17), `websocket` (13), `http` (8),
`date` (8). One, `__ffi_encode_utf8`, is declared by three rolls (`crypto`, `http`, `tls`):
a function needs a single owner, and the other rolls depend on that one.

## Contract 3: Unroll ↔ oitec

`unroll build` resolves the dependency graph (with `@rolls/std` unless
`no-std` is set), fetches what is not cached, builds each used roll's
native part, and calls `oitec` once for the whole program:

```bash
oitec build src/main.ot -o target/release/myapp --release \
    --roll @rolls/http=$OITE_HOME/cache/rolls/http/0.1.0 \
    --roll @rolls/async=$OITE_HOME/cache/rolls/async/0.1.0 \
    --native target/release/rolls/libhttp.a \
    --native target/release/rolls/libasync.a
```

- `oitec` resolves `import ... from "@rolls/<name>"` through `--roll`
  flags first, then `$OITE_HOME/rolls` (the bundled core rolls). Relative
  imports resolve against the importing file.
- `oitec` compiles the entry file and every module it reaches as **one
  program**, on every backend. This is how the native backends already
  work, and it gives whole-program optimisation without a separate link
  step. Separate compilation can come later, if build times need it.
- `oitec run file.ot` and `oitec build file.ot` work without `unroll` for
  programs that use only core rolls.

## Async: where core ends

- **Core:** `async` functions, `await`, promises (`new Promise`, `then`,
  `catch`, `finally`), the microtask queue, and draining it when `main`
  returns. A program that only awaits promises it creates runs with no rolls.
- **`@rolls/async`:** the event loop and IO reactor, timers (`setTimeout`,
  `setInterval`, `sleep`), tasks, channels, mutexes, the work-stealing
  executor. Its native part runs pending timers and IO, and drains core's
  microtask queue between them.

## Today, and what has to change

### In core (`oite`)

| Item | Today | Change |
| ---- | ----- | ------ |
| `declare function` | Compiles to an empty function that returns `undefined`, silently | Bind `__intrinsic_*` and `__ffi_*` as above; any other `declare` of a value with no definition is an error |
| `import` / `export` | VM only; refused on JIT/AOT; no `@rolls/*` resolution | Whole-program modules on every backend, `--roll`, `$OITE_HOME/rolls`, implicit `@rolls/std`, `--no-std` |
| Intrinsics | 0 of 28 implemented | Implement all 28, on every backend |
| FFI | Only `__ffi_fetch` (and `__ffi_getenv`, which no roll uses), as VM globals | Calling convention, `oite-runtime` crate, symbol binding on every backend |
| `Math`, `Date.now` | Globals in both runtimes | Removed; `@rolls/math`, `@rolls/date` |
| `setTimeout` / `clearTimeout` | Globals in both runtimes, with their own timer loop | Removed; `@rolls/async` (`__ffi_set_timeout`) |
| Number methods (`toFixed`, `toString(radix)`) | Built in | Become the `to_*` intrinsics; API in `@rolls/math` |
| String and array methods (`trim`, `slice`, `join`, ...) | Built in, shared by VM and native code | Depends on **Open: method calls** |
| VM `fs`, `fetch`, `process`, `JSON` globals | Built into the VM only | Removed; `@rolls/fs`, `@rolls/http`, `@rolls/json`, ... |
| Error classes (`std/core/error.ot`) | Core | Stay: the runtime throws `TypeError` and the others itself |
| `docs/docs-rolls/design.md` | Lists rolls that do not exist (`@rolls/promise`) and a layout that does not match `warpy-ai/rolls` | Replaced by this page |

### In Rolls (`warpy-ai/rolls`)

- **Native parts:** no roll has its Rust code yet. Every `__ffi_*`
  function is declared, and none is implemented.
- **Remove direct use of JavaScript built-ins:** `String.fromCharCode`,
  `Object.keys`, `parseInt`, and so on, as listed above.
- **Shared tests:** each roll gets golden tests, run by CI against `oitec`
  on the VM, JIT and AOT.

### In Unroll (`warpy-ai/unroll`)

- `oitec` flags: unroll passes `-O3`, `--lto=thin` and `--target=...`;
  `oitec` takes `--release`, `--dist` and `--debug`. One set should be
  agreed and documented in `oitec --help`.
- Per-file object files: unroll compiles each source and each roll to its
  own object and links them. Under contract 3 it calls `oitec` once
  instead.
- Finding `oitec`: `$OITE_HOME/bin/oitec` first, rather than a list of
  developer paths.

## Order of work

1. **Modules in core:** `import`/`export` on JIT and AOT, `@rolls/*`
   resolution, the implicit prelude, `--no-std`. Bundle the core rolls from
   `warpy-ai/rolls` into the test suite, so CI runs real roll code on all
   three backends.
2. **Intrinsics:** the 28 above, then move `Math`, `Date.now` and the
   number methods out of core.
3. **FFI:** the calling convention, `oite-runtime`, and symbol binding;
   `@rolls/async` first, with timers moving out of core.
4. **The remaining native rolls:** date, fs, crypto, tls, http, websocket.
5. **Unroll:** the `oitec` interface above, then `$OITE_HOME` and the
   bundled rolls in releases.
