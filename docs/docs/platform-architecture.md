---
sidebar_position: 7
title: Platform Architecture
description: How Oite is delivered - the core compiler and runtime, the Rolls system libraries, and Unroll - and the contracts between them.
keywords: [architecture, rolls, unroll, ffi, intrinsics, prelude, distribution]
---

# Platform Architecture: Core, Rolls and Unroll

> **Status:** agreed design. The decisions are recorded where they apply
> and summarised under "Decisions". Everything under "Today" was checked
> against `oite`,
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
│   ├── json/
│   └── async/    roll.toml, src/mod.ot, native library
└── cache/rolls/<name>/<version>/   other rolls, fetched by unroll
```

**Core rolls** are bundled and versioned together with `oitec`, so the
intrinsics they call always match the compiler. Every other roll comes from
the registry (`registry.oite.org`) through `unroll`.

The core rolls are `@rolls/std`, what it depends on (`array`, `string`,
`math`, `json`), and `@rolls/async`:

- `@rolls/async` is bundled because its native part works inside the
  runtime: it drains core's microtask queue and calls Oite callbacks, so it
  must match the runtime exactly. fs, http, tls and websocket depend on it,
  so nearly every real application needs it.
- **Bundled is not the prelude.** A bundled roll is in the install and needs
  no download. Only `@rolls/std` is imported implicitly. A program imports
  `@rolls/async` explicitly, and a binary that does not import it does not
  contain it.

### How a release is assembled

- **Rolls** are published to the registry by `warpy-ai/rolls` CI: merging
  a version bump to its `main` publishes that version.
- **`oite` releases** (`.github/workflows/release.yml`, on a `v*` tag) ship
  what `scripts/release-pins.env` pins:
  - `UNROLL_REF`: the unroll commit, as `unroll-src.tar.gz`;
  - `ROLLS_REF` and `CORE_ROLLS`: the rolls commit and which rolls are core,
    as `core-rolls.tar.gz`.
- **The core rolls must match the registry.** `scripts/bundle-core-rolls.sh`
  refuses to bundle a roll unless the registry has that exact version, and
  unless every roll it depends on is also core. So what ships with `oitec`
  is exactly what the registry serves.
- **`install.sh`** unpacks the bundle into `$OITE_HOME/rolls`. The bundle
  includes a `MANIFEST` that lists each roll's version.

## The prelude

`@rolls/std` is imported into every program unless the program opts out:

- `unroll`: `no-std = true` under `[package]` in `unroll.toml` (unroll's
  resolver already injects `@rolls/std`).
- `oitec`: `--no-std`.

The prelude re-exports a curated set as **free functions**:
`map(numbers, n => n * 2)`, `trim(name)`, `sin(PI / 4)`, `jsonParse(text)`,
plus `Result`, `Option`, `println`, `assert`, and so on.

### Method calls

Both styles work, with one implementation, in the rolls:

```javascript
trim(name)            // the @rolls/string function
name.trim()           // the same function, as a method
map(numbers, f)       // the @rolls/array function
numbers.map(f)        // the same function, as a method
```

Existing Oite code is method-style: the self-hosted compiler, the bootstrap
compiler and Unroll have about 920 method calls between them, and
JavaScript and TypeScript familiarity is part of the language. So:

- The rolls keep their free functions.
- The prelude also attaches them as methods of strings and arrays:
  `s.trim()` calls `@rolls/string`'s `trim(s)`.
- Core keeps only the hook: a method call on a string or an array that has
  no own property by that name looks the method up in a table the prelude
  fills (as `Array.prototype` and `String.prototype`).
- Core's built-in string, array and number methods move into the rolls.
- With `--no-std` there are no such methods, as there are no such
  functions.

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

The compiler turns a call to an `__intrinsic_*` name into one `Intrinsic`
instruction, and every backend runs the same Rust function for it
(`src/runtime/intrinsics.rs`, written once over the value representation),
so an intrinsic gives the same result on the VM, the JIT and native binaries.
Native code calls it through the runtime today; emitting a machine
instruction for the math ones is a later optimisation. An unknown
`__intrinsic_` name is a compile error, and so is using an intrinsic as a
value rather than calling it.

The contract is 29 intrinsics:

| Group | Intrinsics |
| ----- | ---------- |
| Math | `floor` `ceil` `round` `sqrt` `pow` `exp` `log` `sin` `cos` `tan` `asin` `acos` `atan` `atan2` `fround` `imul` `random` |
| Numbers | `parse_int` `parse_float` `to_fixed` `to_precision` `to_exponential` |
| Strings | `string_length` `char_at` `char_code_at` `from_char_code` |
| Values | `is_array` `object_keys` |
| Target | `platform` |

Each behaves as the JavaScript built-in it stands for (`Math.floor`,
`parseInt`, `Number.prototype.toPrecision`, `String.fromCharCode`,
`Object.keys`, ...), and `tests/lang/intrinsics/` holds them to Node's
output. Strings count characters (Unicode scalar values), as the string
methods do (#133). The list is versioned with the release: adding an
intrinsic is a minor change; changing or removing one is a breaking change.

### What an intrinsic may be

An intrinsic is a **pure computation**: it never asks the operating system.
That keeps core portable and deterministic, and leaves every access to the
system to a roll's native part.

- `platform` stays an intrinsic: it is a constant of the target the
  program was built for (`"linux"`, `"macos"`, `"windows"`).
- `cwd` reads process state, so it becomes `__ffi_cwd` in `@rolls/path`'s
  native part.

### What a roll may use

A roll uses only:

1. the language: operators, literals, objects, functions, classes, and on
   arrays `.length`, indexing, `.push` and `.pop` (native code compiles
   these inline);
2. intrinsics;
3. its own `__ffi_*` functions;
4. other rolls, through `import`.

No other JavaScript built-in. The ones the rolls use today change as
follows:

| Today | Instead |
| ----- | ------- |
| `Array.isArray`, `parseInt`, `parseFloat` | The `is_array`, `parse_int`, `parse_float` intrinsics |
| `String.fromCharCode`, `Object.keys` | New intrinsics `from_char_code` and `object_keys`: they need the runtime's representation |
| `isNaN(x)` | Oite: `x !== x` |
| `.substring`, `.split`, `.join` in `@rolls/json` | Imports from `@rolls/string` and `@rolls/array` |

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
| `declare function` | Ambient: no code and no binding. A call to an `__intrinsic_*` compiles to the intrinsic; an `__ffi_*` nothing provides is "not defined" when called | Bind `__ffi_*` (FFI) |
| `import` / `export` | Linked at compile time on every backend (`modules.rs`), with `@rolls/*` found through `--roll`, the importing roll's siblings, `OITE_ROLLS_PATH` and `$OITE_HOME/rolls`; circular imports are refused | The implicit `@rolls/std` prelude and `--no-std`, with the intrinsics |
| Intrinsics | All 29, on every backend (`runtime/intrinsics.rs`) | Machine instructions for the math ones |
| Spread (`[...a]`, `{...o}`) | On every backend | — |
| FFI | Only `__ffi_fetch` (and `__ffi_getenv`, which no roll uses), as VM globals | Calling convention, `oite-runtime` crate, symbol binding on every backend |
| `Math`, `Date.now` | Globals in both runtimes | Removed; `@rolls/math`, `@rolls/date` |
| `setTimeout` / `clearTimeout` | Globals in both runtimes, with their own timer loop | Removed; `@rolls/async` (`__ffi_set_timeout`) |
| Number methods (`toFixed`, `toString(radix)`) | Built in | Become the `to_*` intrinsics; API in `@rolls/math` |
| String and array methods (`trim`, `slice`, `join`, ...) | Built in, shared by VM and native code | Move into `@rolls/string` and `@rolls/array`; core keeps the method lookup hook ("Method calls") |
| VM `fs`, `fetch`, `process`, `JSON` globals | Built into the VM only | Removed; `@rolls/fs`, `@rolls/http`, `@rolls/json`, ... |
| Error classes (`std/core/error.ot`) | Core | Stay: the runtime throws `TypeError` and the others itself |
| `docs/docs-rolls/design.md` | Lists rolls that do not exist (`@rolls/promise`) and a layout that does not match `warpy-ai/rolls` | Replaced by this page |

### In Rolls (`warpy-ai/rolls`)

- **Native parts:** no roll has its Rust code yet. Every `__ffi_*`
  function is declared, and none is implemented.
- **Remove direct use of JavaScript built-ins**, as listed under "What a
  roll may use". The globals (`Array.isArray`, `Object.keys`,
  `String.fromCharCode`, `parseInt`, `parseFloat`, `isNaN`) are replaced by
  intrinsics; method calls such as `.substring` and `n.toString(16)` remain
  until the prelude provides them.
- **Methods:** `@rolls/std` attaches the string and array functions as
  methods.
- **`@rolls/path`:** `cwd` through its own `__ffi_cwd` (declared; its
  native part comes with FFI).
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

## Decisions

| Question | Decision |
| -------- | -------- |
| Which rolls are bundled | `std`, `array`, `string`, `math`, `json` and `async`; only `std` is the implicit prelude |
| Method calls or free functions | Both, with one implementation in the rolls; the prelude attaches the functions as methods |
| What an intrinsic may be | A pure computation; no operating system access (`platform` is a constant of the target, `cwd` becomes FFI) |
| What a roll may use | The language, intrinsics, its own FFI, and other rolls; no other JavaScript built-in |

## Order of work

1. **Modules in core** (done): `import`/`export` linked at compile time on
   every backend, and `@rolls/*` resolution.
2. **Intrinsics:** the 29 above (done, with spread on every backend), the
   implicit `@rolls/std` prelude and `--no-std`, then move `Math`,
   `Date.now` and the number methods out of core. CI runs the core rolls
   from `warpy-ai/rolls` (at `ROLLS_REF`) on all three backends.
3. **FFI:** the calling convention, `oite-runtime`, and symbol binding;
   `@rolls/async` first, with timers moving out of core.
4. **The remaining native rolls:** date, fs, crypto, tls, http, websocket.
5. **Unroll:** the `oitec` interface above, then `$OITE_HOME` and the
   bundled rolls in releases.
