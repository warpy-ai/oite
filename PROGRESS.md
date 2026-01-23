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
| Phase 5 | 🚧 In Progress | Runtime & Server |
| Phase 6 | 📋 Planned | Tooling (fmt, lint, LSP) |
| Phase 7 | 📋 Planned | Distribution & Packaging |

**Current Focus:** Phase 5 - Async runtime, HTTP server, work-stealing executor

---

## Architecture

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

#### Standard Library

| Module | Methods |
|--------|---------|
| `console` | `log`, `error` |
| `Math` | 35+ methods (abs, floor, sin, random, etc.) + constants |
| `String` | 20+ methods (trim, slice, indexOf, split, etc.) |
| `Array` | push, pop, map, filter, forEach, splice, etc. |
| `JSON` | parse, stringify |
| `Date` | Constructor, now, parse, UTC, 22 instance methods |
| `Promise` | Constructor, resolve, reject, then, catch, all |
| `fs` | 18 methods (sync + async file operations) |
| `path` | join, resolve, dirname, basename, extname, parse |
| `ByteStream` | Binary data manipulation |

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
- **Self-Hosted Compiler:** 3,100+ lines in `compiler/` directory

#### Compiler Structure
```
compiler/
├── main.tscl           # CLI, pipeline
├── lexer/              # Tokenization (520 lines)
├── parser/             # AST generation (1,163 lines)
├── ast/                # Type definitions (365 lines)
├── ir/                 # IR system (468 lines)
├── codegen/            # Code generation (321 lines)
└── stdlib/             # Built-in declarations
```

#### CLI Flags
```bash
--emit-ir       # Output SSA IR to .ir file
--emit-llvm     # Output LLVM IR to .ll file
--emit-obj      # Output object file to .o file
--verify-ir     # Validate SSA IR
```

---

### Phase 5: Runtime & Server 🚧

Building high-performance async runtime and HTTP stack.

#### Completed
| Component | Status | Description |
|-----------|--------|-------------|
| Async Runtime | ✅ | Task scheduler, timer support |
| I/O Reactor | ✅ | epoll (Linux), kqueue (macOS) |
| io_uring | ✅ | Linux feature-gated (`--features io-uring`) |
| TCP Primitives | ✅ | TcpListener, TcpStream, AsyncRead/AsyncWrite |
| HTTP/1 Parser | ✅ | Request/Response, headers, chunked encoding |
| HTTP Server | ✅ | Routing, method handlers, path parameters |
| Work-Stealing Executor | ✅ | Multi-threaded with `--features work-stealing` |
| TLS Integration | ✅ | rustls with aws-lc-rs (`--features tls`) |
| HTTPS Server | ✅ | HttpsServer with session resumption |

#### Key Files
| File | Lines | Purpose |
|------|-------|---------|
| `src/runtime/async/mod.rs` | 255 | Async traits, TCP primitives |
| `src/runtime/async/reactor.rs` | 282 | epoll/kqueue reactor |
| `src/runtime/async/task.rs` | 345 | Task scheduler, Timer, Executor |
| `src/runtime/async/work_stealing.rs` | 260 | Work-stealing executor |
| `src/runtime/async/worker.rs` | 120 | Worker thread implementation |
| `src/runtime/async/io_uring.rs` | 330 | io_uring backend (Linux) |
| `src/runtime/async/tls.rs` | 530 | TLS with rustls, session resumption |
| `src/runtime/http/mod.rs` | 650 | HTTP parser |
| `src/runtime/http/server.rs` | 535 | HTTP/HTTPS server with routing |

#### Planned
- HTTP/2 support
- WebSocket support
- Database drivers (PostgreSQL, Redis, SQLite)
- Connection pooling

---

### Phase 6: Tooling 📋

- `script repl` - Interactive REPL
- `script fmt` - Code formatter
- `script lint` - Linter
- Language Server (LSP)
- Debugger integration
- Profiler with flamegraphs

---

### Phase 7: Distribution 📋

- `script install` - Package manager
- Lockfiles and dependency resolution
- Cross-compilation support
- Official binaries (GitHub Releases, Homebrew, apt/rpm)
- Docker images

---

## Testing & Performance

### Test Suite
```
118 tests passed
```

Coverage includes: IR lowering, type inference, optimizations, borrow checker, JIT compilation, LLVM backend, language features, async runtime.

### Performance Benchmarks

| Metric | VM | JIT | Speedup |
|--------|----|----|---------|
| Arithmetic | 2.34 µs/iter | 0.39 µs/iter | ~6x |
| JIT compilation | - | 980 µs | - |
| Break-even | - | ~500 iterations | - |

### Performance Targets

| Benchmark | Node.js | Bun | Target |
|-----------|---------|-----|--------|
| HTTP hello world | 100k rps | 200k rps | 250k rps |
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

# With work-stealing executor
cargo build --release --features work-stealing

# With io_uring (Linux only)
cargo build --release --features io-uring

# With TLS/HTTPS support
cargo build --release --features tls
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
| Async runtime | Custom (not tokio) for minimal overhead |
| HTTP | Zero-copy parsing, io_uring on Linux |
| Work-stealing | crossbeam-deque for lock-free queues |
