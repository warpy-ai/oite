---
sidebar_position: 8
title: Development Status and Roadmap
description: What Oite runs today on each backend, what is partial, and what is planned.
keywords: [development status, roadmap, features, progress, releases, changelog]
---

# Development Status

Oite is a compiled language: programs are meant to run as native code, built
with LLVM AOT, with the Cranelift JIT for development. The native backends are
not finished yet. Today most features run only in the VM, which is the
debugging interpreter, so the VM column below is often ahead. This page records
what runs on each backend today. "Covered"
means a golden test in `tests/lang/` checks it; see the
[contributing rules](https://github.com/warpy-ai/oite/blob/master/CONTRIBUTING.md).
Tracking issue for the full gap list: [#136](https://github.com/warpy-ai/oite/issues/136).

## Current Status

| Component                                 | Status                                                                                   |
| ----------------------------------------- | ---------------------------------------------------------------------------------------- |
| LLVM AOT (production target)              | Runs the "native" rows below; links the runtime into a standalone binary                 |
| Cranelift JIT (development)               | Same coverage as LLVM AOT, same runtime                                                  |
| SSA IR + optimizations                    | Working; refuses what the native backends cannot run                                     |
| VM interpreter (debugging)                | Runs most features; ahead of the native backends for now                                 |
| Static type checking                      | Not implemented: annotations are parsed and erased ([#124](https://github.com/warpy-ai/oite/issues/124))                                |
| Memory reclamation                        | Not implemented: nothing is ever freed ([#123](https://github.com/warpy-ai/oite/issues/123))                                            |
| Self-hosting compiler                     | Partial: cannot compile its own source yet ([#131](https://github.com/warpy-ai/oite/issues/131))                                        |
| Source positions in diagnostics           | Compile errors and syntax errors only; runtime and borrow errors still report `1:1` ([#125](https://github.com/warpy-ai/oite/issues/125)) |
| Rolls ecosystem (HTTP, TLS, fs, crypto)   | Planned; needs an FFI first ([#132](https://github.com/warpy-ai/oite/issues/132))                                                       |
| Tooling (fmt, lint, LSP)                  | Planned                                                                                  |
| Unroll package manager                    | Planned                                                                                  |

## Language Features

"Refused" means the backend reports an error and does not run the program.
That is the intended state for anything a native backend cannot run correctly
yet. Closing the gap between the two right-hand columns is the main work.

| Feature                                    | VM (debugging)                       | JIT / AOT (native)                |
| ------------------------------------------ | ------------------------------------ | --------------------------------- |
| Numbers, strings, arithmetic, comparison   | Covered                              | Covered                           |
| `==` / `!=`, `===` / `!==`, `typeof`       | Covered                              | Covered                           |
| Bitwise operators, `**`                    | Covered                              | Covered                           |
| Functions, recursion, functions as values  | Covered                              | Covered                           |
| Top-level variables used from functions    | Covered                              | Covered                           |
| Closures (shared captured variables, per-iteration loop bindings) | Covered      | Covered                           |
| Lexical scope (functions cannot see a caller's locals) | Covered                 | Covered                           |
| Default parameter values                   | Covered                              | Covered                           |
| Compound assignment (`+=`, `*=`, `<<=`, ...) | Covered                            | Covered                           |
| Objects, arrays, `obj[k]`, `.length`, `push`/`pop` | Covered                      | Covered                           |
| Methods on object literals (`this`)        | Covered                              | Covered                           |
| String methods (`slice`, `split`, `trim`, `indexOf`, `padStart`, ...) | Covered   | Covered (shared implementation)   |
| Array methods (`splice`, `join`, `indexOf`, `concat`, `shift`, ...)   | Covered   | Covered (shared implementation)   |
| Callback methods (`map`, `filter`, `forEach`, `reduce`) | Not implemented     | Not implemented                   |
| `if`, `while`, `for`, `do..while`, `?:`    | Covered                              | Covered                           |
| `break` / `continue` (unlabeled)           | Covered                              | Covered                           |
| `&&`, `\|\|`, `??` (short-circuit)         | Covered                              | Covered                           |
| Optional chaining `?.`                     | Covered                              | Covered                           |
| Sequence expressions `a, b`                | Covered                              | Covered                           |
| Classes, `new`, `instanceof`               | Covered                              | Refused                           |
| `extends`, `super`                         | Covered                              | Refused                           |
| `try` / `catch` / `finally`, `throw`       | Covered                              | Refused                           |
| `async` / `await`                          | Partial: `await` polls synchronously; `.then` callbacks never run; `new Promise` aborts ([#122](https://github.com/warpy-ai/oite/issues/122)) | Refused |
| ES modules (`import` / `export`)           | Works, not yet covered               | Refused                           |
| Decorators, private fields, getters/setters | Implemented, not yet covered        | Refused                           |
| Template literals                          | Covered                              | Covered                           |
| `console.log` formatting                   | Covered                              | Covered (same formatter)          |
| `switch`, labeled statements               | Compile error ([#118](https://github.com/warpy-ai/oite/issues/118))                 | Compile error                     |
| Generators, regex, tagged templates, class expressions | Compile error            | Compile error                     |
| Runtime errors as catchable exceptions     | No: most abort the process ([#121](https://github.com/warpy-ai/oite/issues/121))    | -                                 |
| String `length` / indexing                 | Counts UTF-8 bytes ([#133](https://github.com/warpy-ai/oite/issues/133))            | -                                 |
| Type annotations                           | Parsed, not checked ([#124](https://github.com/warpy-ai/oite/issues/124))           | -                                 |
| Ownership / borrow checking                | Compile-time checks run; moves are enforced | -                          |

### Compilers

| Component                         | Status                                         |
| --------------------------------- | ---------------------------------------------- |
| Rust compiler (`src/compiler/`)   | Production compiler for all three backends     |
| Bootstrap compiler (`bootstrap/`) | Runs on the VM; seed for self-hosting          |
| Modular compiler (`compiler/`)    | Runs on the VM; cannot yet compile itself ([#131](https://github.com/warpy-ai/oite/issues/131)) |

`scripts/bootstrap_verify.sh` checks determinism of one module
(`bootstrap/lexer.ot`), not a full self-compilation fixpoint ([#131](https://github.com/warpy-ai/oite/issues/131)).

## Test Coverage

```bash
cargo test --release                  # unit tests + golden tests
cargo test --release --test golden    # language behaviour only
```

- **Golden tests** (`tests/lang/`): each program runs through the real `oitec`
  binary and its output is compared with a hand-written expectation. Programs
  that must fail to compile are checked against their error messages.
- **Unit tests** (`src/`): IR passes, runtime layout, borrow checker, backend
  setup.
- **Bootstrap verification** (`scripts/bootstrap_verify.sh`).

## Performance

| Metric          | VM           | JIT             | Speedup |
| --------------- | ------------ | --------------- | ------- |
| Arithmetic      | 2.34 µs/iter | 0.39 µs/iter    | ~6x     |
| JIT compilation | -            | 980 µs          | -       |
| Break-even      | -            | ~500 iterations | -       |

### Performance Targets

| Benchmark | Node.js | Bun   | Target Oite |
| --------- | ------- | ----- | ----------- |
| `fib(35)` | 50 ms   | 30 ms | 20 ms       |
| Startup   | 30 ms   | 10 ms | 5 ms        |

## What's Intentionally Minimal

Oite Core is like "C without libc" — minimal and self-contained. These features are delegated to the **Rolls** ecosystem:

| Not in Core          | Why                   | Future Location             |
| -------------------- | --------------------- | --------------------------- |
| HTTP/TLS servers     | External dependencies | `@rolls/http`, `@rolls/tls` |
| Database drivers     | Database-specific     | `@rolls/db`                 |
| JSON parsing         | Can be pure Oite      | `@rolls/json`               |
| Math functions       | Standard library      | `@rolls/math`               |
| Advanced file system | POSIX-specific        | `@rolls/fs`                 |
| Crypto operations    | External libraries    | `@rolls/crypto`             |

## Next Steps

Correctness comes before new surface area (see [#136](https://github.com/warpy-ai/oite/issues/136)):

1. **Language correctness**: `switch` and labels ([#118](https://github.com/warpy-ai/oite/issues/118)), runtime errors as exceptions ([#121](https://github.com/warpy-ai/oite/issues/121)), async ([#122](https://github.com/warpy-ai/oite/issues/122)), the string model ([#133](https://github.com/warpy-ai/oite/issues/133))
2. **Native backends**: a real native runtime and the missing IR ops ([#127](https://github.com/warpy-ai/oite/issues/127), [#129](https://github.com/warpy-ai/oite/issues/129)), then exceptions ([#120](https://github.com/warpy-ai/oite/issues/120))
3. **Compiler infrastructure**: source positions everywhere ([#125](https://github.com/warpy-ai/oite/issues/125)), type checking ([#124](https://github.com/warpy-ai/oite/issues/124))
4. **Foundations**: language specification ([#135](https://github.com/warpy-ai/oite/issues/135)), memory reclamation ([#123](https://github.com/warpy-ai/oite/issues/123)), FFI ([#132](https://github.com/warpy-ai/oite/issues/132))
5. **Rolls, Unroll, tooling**: after the above

## Contributing

Contributions are welcome! See the [Contributing Guide](/contributing) for details.
