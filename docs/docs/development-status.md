---
sidebar_position: 8
---

# Development Status

Script is actively under development. Here's the current status of major features and phases.

## Phase Roadmap

| Phase | Status | Description |
|-------|--------|-------------|
| Phase 0 | ✅ Complete | Runtime kernel (NaN-boxing, allocator, stubs) |
| Phase 1 | ✅ Complete | SSA IR (lowering, type inference, optimizations) |
| Phase 2 | ✅ Complete | Cranelift JIT backend |
| Phase 3 | ✅ Complete | LLVM AOT backend with LTO |
| Phase 4 | 🚧 In Progress | Language Completion (JS compatibility) |
| Phase 5 | 🚧 Planned | Self-Hosting Compiler |
| Phase 6 | 🚧 Planned | Runtime & Server (HTTP, async runtime) |
| Phase 7 | 🚧 Planned | Tooling (fmt, lint, LSP, profiler) |
| Phase 8 | 🚧 Planned | Distribution (packages, installers, binaries) |

## Current Phase: Language Completion

### ✅ Completed Features

- **Control Flow**: `if`/`else`, `while`, `for`, `do..while`, `break`, `continue`
- **Error Handling**: `try`/`catch`/`finally`, `throw` with exception propagation
- **Classes & OOP**: ES6 classes with inheritance, `super()`, getters/setters, private fields
- **Decorators**: TypeScript-style decorators on classes, methods, and fields
- **Template Literals**: Backtick strings with interpolation
- **Type System**: Type annotations, type inference, generics, ownership types

### 🚧 In Progress

- **Modules**: `import`/`export` syntax (ES modules)
- **Async/Await**: `async`/`await`, Promise type, event loop integration
- **Standard Library**: Rich `fs`, `net`, `http`, `crypto`, `process`, `os` modules

## Test Coverage

Current test status:

```
94 tests passed, 0 failed
```

Coverage includes:
- IR lowering and optimization
- Type inference and specialization
- Runtime stubs and heap allocation
- VM functionality
- Borrow checker and closures
- Backend compilation (Cranelift, LLVM)
- Language features (loops, exceptions, classes, decorators)

## Performance Targets

| Benchmark | Node.js | Bun | Target Script |
|-----------|--------|-----|---------------|
| HTTP hello world | 100k rps | 200k rps | 250k rps |
| JSON parse | 1x | 1.5x | 2x |
| `fib(35)` | 50 ms | 30 ms | 20 ms |
| Startup | 30 ms | 10 ms | 5 ms |

Current JIT performance: ~6x faster than VM on arithmetic microbenchmarks.

## Next Steps

1. **Strengthen class semantics**:
   - Private field enforcement
   - Getter/setter auto-calling
   - Consistent `instanceof` across backends

2. **ES Modules**:
   - `import`/`export`, module graph, resolution, tree-shaking

3. **Async/await**:
   - `async`/`await`, Promise, event loop integration

4. **Self-Hosting**:
   - Emit SSA IR from Script compiler
   - Move toward self-hosted native compiler

## Contributing

Contributions are welcome! See the [Contributing Guide](/docs/contributing) for details.
