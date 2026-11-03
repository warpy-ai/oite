---
sidebar_position: 8
title: Development Status and Roadmap
description: What Oite runs today on each backend, what is partial, and what is planned.
keywords: [development status, roadmap, features, progress, releases, changelog]
---

# Development Status

Oite runs real programs on the VM. Its native backends and several advertised
features are not finished yet. This page records what runs today. "Covered"
means a golden test in `tests/lang/` checks it; see the
[contributing rules](https://github.com/warpy-ai/oite/blob/master/CONTRIBUTING.md).
Tracking issue for the full gap list: [#136](https://github.com/warpy-ai/oite/issues/136).

## Current Status

| Component                                 | Status                                                                                   |
| ----------------------------------------- | ---------------------------------------------------------------------------------------- |
| VM interpreter                            | Working; reference implementation                                                        |
| SSA IR + optimizations                    | Working for numeric code                                                                  |
| Cranelift JIT                             | Numeric code only; object property access crashes                                        |
| LLVM AOT                                  | Links on Linux and macOS; native runtime is stubbed, so output is wrong ([#127](https://github.com/warpy-ai/oite/issues/127), [#129](https://github.com/warpy-ai/oite/issues/129))     |
| Static type checking                      | Not implemented: annotations are parsed and erased ([#124](https://github.com/warpy-ai/oite/issues/124))                                |
| Memory reclamation                        | Not implemented: nothing is ever freed ([#123](https://github.com/warpy-ai/oite/issues/123))                                            |
| Self-hosting compiler                     | Partial: cannot compile its own source yet ([#131](https://github.com/warpy-ai/oite/issues/131))                                        |
| Source positions in diagnostics           | Compile errors and syntax errors only; runtime and borrow errors still report `1:1` ([#125](https://github.com/warpy-ai/oite/issues/125)) |
| Rolls ecosystem (HTTP, TLS, fs, crypto)   | Planned; needs an FFI first ([#132](https://github.com/warpy-ai/oite/issues/132))                                                       |
| Tooling (fmt, lint, LSP)                  | Planned                                                                                  |
| Unroll package manager                    | Planned                                                                                  |

## Language Features

"Refused" means the backend reports an error and does not run the program.
That is the intended state for anything a backend cannot yet do exactly as
the VM does.

| Feature                                    | VM                                   | JIT / AOT                         |
| ------------------------------------------ | ------------------------------------ | --------------------------------- |
| Numbers, strings, arithmetic, comparison   | Covered                              | JIT works for numbers             |
| Functions, closures, recursion             | Covered                              | JIT works for numeric functions   |
| `if`, `while`, `for`, `do..while`, `?:`    | Covered                              | JIT works                         |
| `break` / `continue` (unlabeled)           | Covered                              | JIT works                         |
| `&&`, `\|\|`, `??` (short-circuit)         | Covered                              | JIT works                         |
| Optional chaining `?.`                     | Covered                              | Needs objects (crashes)           |
| Sequence expressions `a, b`                | Covered                              | -                                 |
| Classes, constructors, methods             | Covered                              | Crashes (objects)                 |
| `extends`, `super`                         | Covered                              | Refused                           |
| `try` / `catch` / `finally`, `throw`       | Covered                              | Refused                           |
| `async` / `await`                          | Partial: `await` polls synchronously; `.then` callbacks never run; `new Promise` aborts ([#122](https://github.com/warpy-ai/oite/issues/122)) | Refused |
| ES modules (`import` / `export`)           | Works, not yet covered               | Refused                           |
| Decorators, private fields, getters/setters | Implemented, not yet covered        | -                                 |
| Template literals                          | Implemented, not yet covered         | -                                 |
| `console.log` formatting                   | Covered                              | Stub prints numbers only          |
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
| Bootstrap compiler (`bootstrap/`) | Runs on the VM; reference for self-hosting     |
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
