---
sidebar_position: 8
title: Development Status and Roadmap
description: Track Oite's development progress, feature status, and roadmap. See what's implemented, in progress, and planned for future releases.
keywords: [development status, roadmap, features, progress, releases, changelog]
---

# Development Status

Oite's core language is complete. Library functionality (HTTP, TLS, fs, etc.) will be developed in the **Rolls** ecosystem.

## Current Status

| Component                                   | Status   |
| ------------------------------------------- | -------- |
| Runtime kernel (NaN-boxing, allocator)      | Complete |
| SSA IR + optimizations                      | Complete |
| Native backends (Cranelift JIT, LLVM AOT)   | Complete |
| Language features (classes, async, modules) | Complete |
| Self-hosting compiler                       | Complete |
| Rolls ecosystem (HTTP, TLS, fs, crypto)     | Planned  |
| Tooling (fmt, lint, LSP)                    | Planned  |
| Unroll package manager                      | Planned  |

## Core Language: Complete

###All Language Features Implemented

- **Control Flow**: `if`/`else`, `while`, `for`, `do..while`, `break`/`continue` with labels
- **Error Handling**: `try`/`catch`/`finally`, `throw` with exception propagation
- **Classes & OOP**: ES6 classes, inheritance, `super()`, getters/setters, private fields (`#field`)
- **Decorators**: TypeScript-style decorators on classes, methods, and fields
- **Template Literals**: Backtick strings with interpolation
- **Type System**: Type annotations, Hindley-Milner inference, generics, ownership types
- **Modules**: ES module `import`/`export` syntax with file-based resolution
- **Async/Await**: `async function`, `await`, Promise.resolve/then/catch

###Self-Hosting Compiler Complete

The compiler written in Oite can now compile itself to native binaries:

| Component                         | Status         | Output           |
| --------------------------------- | -------------- | ---------------- |
| Rust Compiler (`src/compiler/`)   | Production     | Native binaries  |
| Bootstrap Compiler (`bootstrap/`) | Reference      | Bytecode         |
| Modular Compiler (`compiler/`)    | Self-Compiling | LLVM IR → Native |

**Build Pipeline:**

```bash
./target/release/oitec compiler/main.ot llvm input.ot  # → input.ot.ll
clang input.ot.ll -c -o input.o                          # → input.o
clang input.o -o output                                     # → native binary
```

**Performance (Native vs VM):**
| Test | Native | VM | Speedup |
|------|--------|-----|---------|
| Fibonacci(25) | 75025 | 75025 | ~30x faster |
| Loops | Pass | ✅ | ~30x faster |
| Recursion | Pass | ✅ | ~30x faster |
| Objects/Functions | Pass | ✅ | ~4x faster |

## Test Coverage

Current test status:

```
113 tests passed, 0 failed
```

Coverage includes:

- IR lowering and optimization (DCE, CSE, constant folding, copy propagation)
- Type inference and specialization
- Runtime stubs and heap allocation
- VM functionality
- Borrow checker and closures
- Backend compilation (Cranelift JIT, LLVM AOT)
- Language features (loops, exceptions, classes, decorators, modules)
- Self-compilation verification
- Deterministic build verification

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

1. **Rolls Ecosystem**: Build official system libraries (`@rolls/http`, `@rolls/fs`, etc.)
2. **Unroll Package Manager**: Create build system and package management
3. **Tooling**: Formatter, linter, LSP for IDE integration
4. **Bootstrap Verification**: Verify `hash(oite₁) == hash(oite₂)` for deterministic self-hosting

## Contributing

Contributions are welcome! See the [Contributing Guide](/contributing) for details.
