# Script: Development Progress

High-performance systems language with **TypeScript syntax** compiling to **native code** via **Cranelift JIT** and **LLVM AOT**.

**Goals:** Faster than Bun, Actix-level server performance, familiar JS/TS syntax, standalone native binaries.

---

## Quick Status

| Phase | Status | Description |
|-------|--------|-------------|
| Phase 0 | ✅ Complete | Runtime Kernel Foundation |
| Phase 1 | ✅ Complete | SSA IR System |
| Phase 2 | ✅ Complete | Native Backend (Cranelift + LLVM) |
| Phase 3 | ✅ Complete | Language Completion |
| Phase 4 | ✅ Complete | Self-Hosting Compiler |

**Current Focus:** Language core is complete. Library functionality (HTTP, TLS, fs, etc.) will be developed in the **Rolls** ecosystem (separate repository).

---

## Architecture

Script is the **language core** — compiler, type system, and minimal runtime. Library functionality is separated:

```
┌─────────────────────────────────────────┐
│            User App Code                │
└──────────────────┬──────────────────────┘
                   │
┌──────────────────▼──────────────────────┐
│   Rolls (official system libs)          │  ← FUTURE: separate repo
│   @rolls/http, @rolls/tls, @rolls/fs    │
└──────────────────┬──────────────────────┘
                   │
┌──────────────────▼──────────────────────┐
│   Unroll (runtime + tooling)            │  ← FUTURE: separate repo
│   pkg manager, lockfiles, bundler, LSP  │
└──────────────────┬──────────────────────┘
                   │
┌──────────────────▼──────────────────────┐
│   Script (language core)                │  ← THIS REPO
│   compiler, type system, ABI, bootstrap │
└─────────────────────────────────────────┘
```

### Compilation Pipeline

```
tscl source → Compiler → SSA IR → Native Backend → CPU
                 ↓
          Borrow Checker
          Type Inference
          Optimizations
```

### Execution Modes

| Mode | Command | Use Case |
|------|---------|----------|
| JIT | `script jit app.tscl` | Fast development, benchmarking |
| AOT Release | `script build app.tscl --release -o app` | Production (ThinLTO) |
| AOT Dist | `script build app.tscl --dist -o app` | Maximum optimization (Full LTO) |
| VM | `script run app.tscl` | Debugging, REPL, compatibility |

---

## Phase Details

### Phase 0: Runtime Kernel ✅

Unified runtime primitives shared across VM/JIT/AOT backends.

**Key Components:**
- `src/runtime/abi.rs` - NaN-boxed 64-bit `TsclValue` ABI
- `src/runtime/heap.rs` - Bump allocator, object layouts
- `src/runtime/stubs.rs` - 20+ `extern "C"` stubs for native backends

**Runtime Stubs:** `tscl_alloc_object`, `tscl_add_any`, `tscl_get_prop`, `tscl_set_prop`, `tscl_call`, `tscl_console_log`, etc.

---

### Phase 1: SSA IR System ✅

Register-based SSA IR with type tracking and optimizations.

**Key Files:**
- `src/ir/lower.rs` - Bytecode → SSA lowering
- `src/ir/typecheck.rs` - Flow-sensitive type inference
- `src/ir/opt.rs` - DCE, constant folding, CSE, copy propagation
- `src/ir/verify.rs` - IR validation + borrow checking

**Optimization Passes:** Dead code elimination, constant folding, common subexpression elimination, copy propagation, branch simplification.

**CLI:** `script ir app.tscl` - Inspect IR before/after optimization

---

### Phase 2: Native Backend ✅

#### 2A: Cranelift JIT

Fast compilation for development. Each `IrOp` becomes Cranelift instructions or runtime stub calls.

```bash
script jit app.tscl
```

#### 2B: Multi-Function JIT + Tiered Compilation

- Function extraction from bytecode
- Direct call resolution through constant propagation
- Phi node → Cranelift block parameter translation
- `TierManager` for baseline/optimizing thresholds

#### 2C: LLVM AOT + LTO

Production binaries with LLVM 18.

```bash
# Prerequisites (macOS)
brew install llvm@18 zstd
export LLVM_SYS_180_PREFIX=$(brew --prefix llvm@18)

# Build
script build app.tscl --release -o app  # ThinLTO
script build app.tscl --dist -o app     # Full LTO
```

**Key Files:** `src/backend/llvm/codegen.rs`, `src/backend/llvm/optimizer.rs`, `src/backend/llvm/linker.rs`

---

### Phase 3: Language Completion ✅

Full TypeScript-style language with ownership semantics.

#### Type System
- Type annotations: `let x: number`, `function add(a: number): number`
- Ownership: `Ref<T>`, `MutRef<T>`, move semantics
- Generics with monomorphization
- Hindley-Milner inference

#### Control Flow
- `if`/`else`, `while`, `for`, `do..while`
- `break`/`continue` with labels
- `try`/`catch`/`finally`, `throw`

#### Classes & OOP
- ES6 class syntax with constructors
- `extends`, `super()`, prototype chain
- Static/instance methods and properties
- Getters, setters, private fields (`#field`)
- Decorators (`@decorator`, `@decorator(args)`)

#### Modules
- ES module `import`/`export` syntax
- File-based resolution (`.tscl`, `.ts`, `.js`)
- Module caching with SHA256 verification

#### Async/Await
- `async function` syntax
- `Promise.resolve()`, `.then()`, `.catch()`
- `await` expression handling

#### Minimal Standard Library

Script core includes only essential primitives:

| Module | Methods |
|--------|---------|
| `console` | `log`, `error` |
| `String` | `fromCharCode` |
| `ByteStream` | Binary data manipulation for bootstrap compiler |
| `fs` | `readFileSync`, `writeFileSync`, `writeBinaryFile` (minimal) |
| `require` | Module loading |

> **Note:** Full standard library (Math, Date, JSON, comprehensive fs/path, HTTP, TLS, etc.) will be provided by the **Rolls** ecosystem. See `docs/future/rolls-design.md`.

---

### Phase 4: Self-Hosting Compiler ✅

Compiler written in tscl, producing deterministic native binaries.

#### Bootstrap Chain
```
tscl₀ (Rust) ──compile──> tscl₁ (native)
                            │
                            └──compile──> tscl₂ (self-compiled)
                                              │
                                              └──verify: hash(tscl₁) == hash(tscl₂)
```

#### Key Achievements
- **ABI Frozen:** `ABI_VERSION = 1`, stable runtime interface
- **IR Frozen:** Deterministic serialization with `--emit-ir`
- **Deterministic Builds:** Bit-for-bit reproducible with `--dist`
- **Self-Hosted Compiler:** Two implementations in .tscl

#### Compiler Structure (Modular - `compiler/`)
```
compiler/
├── main.tscl           # CLI entry point
├── lexer/              # Tokenization module
│   ├── mod.tscl
│   ├── token.tscl
│   └── error.tscl
├── parser/             # AST generation module
│   ├── mod.tscl
│   ├── expr.tscl
│   ├── stmt.tscl
│   └── error.tscl
├── ast/                # AST type definitions
│   ├── mod.tscl
│   └── types.tscl
├── ir/                 # IR system
│   ├── mod.tscl
│   └── builder.tscl
├── codegen/            # Code generation
│   └── mod.tscl
└── stdlib/             # Runtime declarations
    └── builtins.tscl
```

#### Bootstrap Compiler (Flat - `bootstrap/`)
```
bootstrap/
├── main.tscl           # CLI entry point
├── types.tscl          # Type definitions
├── lexer.tscl          # Tokenization
├── parser.tscl         # AST generation
├── ir.tscl             # IR types
├── ir_builder.tscl     # AST → IR
├── codegen.tscl        # IR → Bytecode
├── emitter.tscl        # Bytecode serialization
├── pipeline.tscl       # Compilation orchestration
├── stdlib.tscl         # Runtime declarations
└── utils.tscl          # Helpers
```

#### CLI Flags
```bash
--emit-ir       # Output SSA IR to .ir file
--emit-llvm     # Output LLVM IR to .ll file
--emit-obj      # Output object file to .o file
--verify-ir     # Validate SSA IR
```

---

## Future: Rolls & Unroll

Library functionality has been extracted to future repositories:

### Rolls (System Libraries)

Official libraries built on Script core:

| Roll | Purpose |
|------|---------|
| `@rolls/async` | Work-stealing executor, io_uring |
| `@rolls/tls` | TLS encryption via rustls |
| `@rolls/http` | HTTP/1.1, HTTP/2 server |
| `@rolls/websocket` | WebSocket protocol |
| `@rolls/fs` | File system operations |
| `@rolls/path` | Path utilities |
| `@rolls/json` | JSON parse/stringify |
| `@rolls/math` | Math functions |
| `@rolls/date` | Date/time handling |
| `@rolls/string` | String methods |
| `@rolls/array` | Array methods |
| `@rolls/promise` | Promise implementation |

See `docs/future/rolls-design.md` for detailed architecture.

### Unroll (Tooling)

Package manager and developer tools:

| Component | Purpose |
|-----------|---------|
| `unroll new` | Create new project |
| `unroll add` | Add Roll dependency |
| `unroll build` | Build with static linking |
| `unroll run` | Build and run |
| `unroll fmt` | Code formatter |
| `unroll lint` | Linter |
| LSP | Language server |

See `docs/future/unroll-design.md` for detailed architecture.

---

## Testing & Performance

### Test Suite
```
60+ tests passed
```

Coverage includes: IR lowering, type inference, optimizations, borrow checker, JIT compilation, LLVM backend, language features.

### Performance Benchmarks

| Metric | VM | JIT | Speedup |
|--------|----|----|---------|
| Arithmetic | 2.34 µs/iter | 0.39 µs/iter | ~6x |
| JIT compilation | - | 980 µs | - |
| Break-even | - | ~500 iterations | - |

### Performance Targets

| Benchmark | Node.js | Bun | Target |
|-----------|---------|-----|--------|
| fib(35) | 50 ms | 30 ms | 20 ms |
| Startup | 30 ms | 10 ms | 5 ms |

---

## Building

### Prerequisites
```bash
# macOS
brew install llvm@18 zstd
export LLVM_SYS_180_PREFIX=$(brew --prefix llvm@18)

# Build
cargo build --release
```

### Running
```bash
# JIT execution
./target/release/script jit app.tscl

# VM execution
./target/release/script run app.tscl

# Build native binary
./target/release/script build app.tscl --release -o app

# Run tests
cargo test
```

---

## Key Design Decisions

| Area | Decision |
|------|----------|
| Value representation | 64-bit NaN-boxed words |
| Module system | Native ES Modules (no CommonJS) |
| Memory model | Rust-style ownership + borrow checking |
| Async runtime | Minimal core (epoll/kqueue reactor) |
| Standard library | Minimal core; extended via Rolls |

---

## Project Structure

```
script/
├── Cargo.toml                    # Minimal dependencies
├── compiler/                     # Self-hosted compiler (modular .tscl)
├── bootstrap/                    # Bootstrap compiler (flat .tscl files)
├── src/
│   ├── compiler/                 # Rust: Parser → Bytecode
│   ├── ir/                       # SSA IR system
│   ├── backend/                  # Cranelift JIT + LLVM AOT
│   ├── runtime/
│   │   ├── abi.rs                # NaN-boxed values
│   │   ├── heap.rs               # Memory allocation
│   │   ├── stubs.rs              # FFI bridge
│   │   └── async/                # Core async primitives
│   ├── vm/                       # Debug interpreter
│   └── stdlib/                   # Minimal: console, ByteStream, fs
├── docs/
│   └── future/                   # Rolls & Unroll designs
└── tests/
```
