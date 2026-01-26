---
sidebar_position: 8
title: Development Status and Roadmap
description: Track Script's development progress, feature status, and roadmap. See what's implemented, in progress, and planned for future releases.
keywords: [development status, roadmap, features, progress, releases, changelog]
---

# Development Status

Script's core language is complete. Library functionality (HTTP, TLS, fs, etc.) will be developed in the **Rolls** ecosystem.

## Phase Roadmap

| Phase   | Status      | Description                                       |
| ------- | ----------- | ------------------------------------------------- |
| Phase 0 | ✅ Complete | Runtime kernel (NaN-boxing, allocator, stubs)     |
| Phase 1 | ✅ Complete | SSA IR (lowering, type inference, optimizations)  |
| Phase 2 | ✅ Complete | Native Backend (Cranelift JIT + LLVM AOT)         |
| Phase 3 | ✅ Complete | Language Completion (full TypeScript syntax)      |
| Phase 4 | 🚧 Working  | Self-Hosting Compiler (compiles itself to native) |
| Phase 5 | 📋 Planned  | Rolls Ecosystem (HTTP, TLS, fs, crypto libraries) |
| Phase 6 | 📋 Planned  | Tooling (fmt, lint, LSP, profiler)                |
| Phase 7 | 📋 Planned  | Distribution (Unroll package manager)             |

## Core Language: Complete

### ✅ All Language Features Implemented

- **Control Flow**: `if`/`else`, `while`, `for`, `do..while`, `break`/`continue` with labels
- **Error Handling**: `try`/`catch`/`finally`, `throw` with exception propagation
- **Classes & OOP**: ES6 classes, inheritance, `super()`, getters/setters, private fields (`#field`)
- **Decorators**: TypeScript-style decorators on classes, methods, and fields
- **Template Literals**: Backtick strings with interpolation
- **Type System**: Type annotations, Hindley-Milner inference, generics, ownership types
- **Modules**: ES module `import`/`export` syntax with file-based resolution
- **Async/Await**: `async function`, `await`, Promise.resolve/then/catch

### ✅ Self-Hosting Compiler Complete

The compiler written in Script can now compile itself to native binaries:

| Component                         | Status            | Output           |
| --------------------------------- | ----------------- | ---------------- |
| Rust Compiler (`src/compiler/`)   | ✅ Production     | Native binaries  |
| Bootstrap Compiler (`bootstrap/`) | ✅ Reference      | Bytecode         |
| Modular Compiler (`compiler/`)    | ✅ Self-Compiling | LLVM IR → Native |

**Build Pipeline:**

```bash
./target/release/script compiler/main.tscl llvm input.tscl  # → input.tscl.ll
clang input.tscl.ll -c -o input.o                          # → input.o
clang input.o -o output                                     # → native binary
```

**Performance (Native vs VM):**
| Test | Native | VM | Speedup |
|------|--------|-----|---------|
| Fibonacci(25) | 75025 | 75025 | ~30x faster |
| Loops | ✅ | ✅ | ~30x faster |
| Recursion | ✅ | ✅ | ~30x faster |
| Objects/Functions | ✅ | ✅ | ~4x faster |

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

| Benchmark | Node.js | Bun   | Target Script |
| --------- | ------- | ----- | ------------- |
| `fib(35)` | 50 ms   | 30 ms | 20 ms         |
| Startup   | 30 ms   | 10 ms | 5 ms          |

## What's Intentionally Minimal

Script Core is like "C without libc" — minimal and self-contained. These features are delegated to the **Rolls** ecosystem:

| Not in Core          | Why                   | Future Location             |
| -------------------- | --------------------- | --------------------------- |
| HTTP/TLS servers     | External dependencies | `@rolls/http`, `@rolls/tls` |
| Database drivers     | Database-specific     | `@rolls/db`                 |
| JSON parsing         | Can be pure Script    | `@rolls/json`               |
| Math functions       | Standard library      | `@rolls/math`               |
| Advanced file system | POSIX-specific        | `@rolls/fs`                 |
| Crypto operations    | External libraries    | `@rolls/crypto`             |

## Next Steps

1. **Rolls Ecosystem**: Build official system libraries (`@rolls/http`, `@rolls/fs`, etc.)
2. **Unroll Package Manager**: Create build system and package management
3. **Tooling**: Formatter, linter, LSP for IDE integration
4. **Bootstrap Verification**: Verify `hash(tscl₁) == hash(tscl₂)` for deterministic self-hosting

## Contributing

Contributions are welcome! See the [Contributing Guide](/docs/contributing) for details.
