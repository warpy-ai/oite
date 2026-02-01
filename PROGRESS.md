# Oite: Development Progress

High-performance systems language with **TypeScript syntax** compiling to **native code** via **Cranelift JIT** and **LLVM AOT**.

**Goals:** Faster than Bun, Actix-level server performance, familiar JS/TS syntax, standalone native binaries.

---

## Quick Status

**Core language complete.** Library functionality (HTTP, TLS, fs, etc.) will be developed in the **Rolls** ecosystem (separate repository).

| Component | Status |
|-----------|--------|
| Runtime kernel (NaN-boxing, allocator) | ✅ Complete |
| SSA IR + optimizations | ✅ Complete |
| Native backends (Cranelift JIT, LLVM AOT) | ✅ Complete |
| Language features | ✅ Complete |
| Self-hosting compiler | ✅ Complete |

---

## Architecture

**Oite Core is like C without libc** — minimal, self-contained, runs without dependencies.

```
┌───────────────────────────────────┐  ┌──────────────────────────────────┐
│         OITE CORE               │  │            ROLLS                 │
│  ✅ Always available              │  │  ⚡ Optional system libraries    │
│                                   │  │                                  │
│  • Compiler (oitec)             │  │  • @rolls/http, @rolls/tls       │
│  • Runtime (NaN-boxing, heap)     │  │  • @rolls/fs, @rolls/db          │
│  • Primitives + console.log       │  │  • @rolls/async, @rolls/crypto   │
└───────────────────────────────────┘  └──────────────────────────────────┘
                    │                               │
                    └───────────────┬───────────────┘
                                    ▼
┌─────────────────────────────────────────────────────────────────────────┐
│  UNROLL: Build system → Single static binary (no runtime needed)        │
└─────────────────────────────────────────────────────────────────────────┘
```

See `docs/ARCHITECTURE.md` for detailed diagrams and philosophy.

### Compilation Pipeline

```
tscl source → Compiler → SSA IR → Native Backend → CPU
                 ↓
          Borrow Checker
          Type Inference
          Optimizations
```

### Execution Modes

| Mode        | Command                                  | Use Case                        |
| ----------- | ---------------------------------------- | ------------------------------- |
| JIT         | `oite jit app.ot`                    | Fast development, benchmarking  |
| AOT Release | `oite build app.ot --release -o app` | Production (ThinLTO)            |
| AOT Dist    | `oite build app.ot --dist -o app`    | Maximum optimization (Full LTO) |
| VM          | `oite run app.ot`                    | Debugging, REPL, compatibility  |

---

## Implementation Details

### Runtime Kernel ✅

Unified runtime primitives shared across VM/JIT/AOT backends.

**Key Components:**

- `src/runtime/abi.rs` - NaN-boxed 64-bit `TsclValue` ABI
- `src/runtime/heap.rs` - Bump allocator, object layouts
- `src/runtime/stubs.rs` - 20+ `extern "C"` stubs for native backends

**Runtime Stubs:** `tscl_alloc_object`, `tscl_add_any`, `tscl_get_prop`, `tscl_set_prop`, `tscl_call`, `tscl_console_log`, etc.

---

### SSA IR System ✅

Register-based SSA IR with type tracking and optimizations.

**Key Files:**

- `src/ir/lower.rs` - Bytecode → SSA lowering
- `src/ir/typecheck.rs` - Flow-sensitive type inference
- `src/ir/opt.rs` - DCE, constant folding, CSE, copy propagation
- `src/ir/verify.rs` - IR validation + borrow checking

**Optimization Passes:** Dead code elimination, constant folding, common subexpression elimination, copy propagation, branch simplification.

**CLI:** `oite ir app.ot` - Inspect IR before/after optimization

---

### Native Backend ✅

#### 2A: Cranelift JIT

Fast compilation for development. Each `IrOp` becomes Cranelift instructions or runtime stub calls.

```bash
oite jit app.ot
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
oite build app.ot --release -o app  # ThinLTO
oite build app.ot --dist -o app     # Full LTO
```

**Key Files:** `src/backend/llvm/codegen.rs`, `src/backend/llvm/optimizer.rs`, `src/backend/llvm/linker.rs`

---

### Language Features ✅

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
- File-based resolution (`.ot`, `.ts`, `.js`)
- Module caching with SHA256 verification

#### Async/Await

- `async function` syntax
- `Promise.resolve()`, `.then()`, `.catch()`
- `await` expression handling

#### Minimal Standard Library

Oite core includes only essential primitives:

| Module       | Methods                                                      |
| ------------ | ------------------------------------------------------------ |
| `console`    | `log`, `error`                                               |
| `String`     | `fromCharCode`                                               |
| `ByteStream` | Binary data manipulation for bootstrap compiler              |
| `fs`         | `readFileSync`, `writeFileSync`, `writeBinaryFile` (minimal) |
| `require`    | Module loading                                               |

> **Note:** Full standard library (Math, Date, JSON, comprehensive fs/path, HTTP, TLS, etc.) will be provided by the **Rolls** ecosystem. See `docs/future/rolls-design.md`.

---

### Self-Hosting Compiler ✅

Fully self-hosted compiler (`oitec`) written in Oite with TypeScript support.

#### Current State

| Compiler          | Location           | Status            | Output                   |
| ----------------- | ------------------ | ----------------- | ------------------------ |
| **Rust Compiler** | `src/compiler/`    | ✅ Production     | Native binaries          |
| **Bootstrap**     | `bootstrap/*.ot` | ✅ Self-Compiling | Bytecode                 |
| **Modular**       | `compiler/*.ot`  | ✅ Working        | Bytecode (VM executable) |

#### Self-Compilation Verified ✅

The bootstrap compiler can now compile itself! All 8 modules successfully self-compile:

| Module          | Compiled Size | Purpose                   |
| --------------- | ------------- | ------------------------- |
| types.ot      | 37 bytes      | Type definitions          |
| lexer.ot      | 1,325 bytes   | Tokenization              |
| parser.ot     | 7,947 bytes   | AST generation            |
| emitter.ot    | 4,547 bytes   | Bytecode serialization    |
| ir.ot         | 2,766 bytes   | IR types                  |
| ir_builder.ot | 1,363 bytes   | AST → IR                  |
| codegen.ot    | 1,580 bytes   | IR → Bytecode             |
| pipeline.ot   | 969 bytes     | Compilation orchestration |

**Total:** ~20KB bytecode from ~5,000 lines of self-hosted compiler code.

#### TypeScript Support in Bootstrap

The bootstrap compiler now supports TypeScript syntax:

| Feature           | Example             | Status |
| ----------------- | ------------------- | ------ |
| Type annotations  | `let x: number`     | ✅     |
| Function types    | `(a: T) => R`       | ✅     |
| Union types       | `A \| B \| C`       | ✅     |
| Generic types     | `Array<T>`          | ✅     |
| Array shorthand   | `T[]`               | ✅     |
| Object types      | `{ x: number }`     | ✅     |
| Type aliases      | `type Foo = ...`    | ✅     |
| Interfaces        | `interface Foo { }` | ✅     |
| Enums             | `enum Color { }`    | ✅     |
| Type assertions   | `x as Type`         | ✅     |
| typeof operator   | `typeof x`          | ✅     |
| Hex literals      | `0xFF`              | ✅     |
| Bitwise operators | `<<`, `>>`, `&`     | ✅     |

#### Self-Hosting Roadmap

See `docs/SELF_HOSTING.md` for detailed plan.

**Foundation** ✅

```
Source → bootstrap/*.ot → Bytecode → Rust VM
Source → src/compiler/ (Rust) → Native Binary ← Production builds
```

**Feature Parity** ✅

```
Source → compiler/*.ot → Bytecode → Rust VM
         + Type inference, optimizations, borrow checking
         + All CLI commands working: ast, ir, check, build, run
```

**Native Code Generation** ✅

```
Source → compiler/*.ot → LLVM IR (.ll) → clang → Native Binary
         No Rust compiler needed for builds!
```

The self-hosted compiler now generates LLVM IR that compiles to native binaries:

| Test          | Native Output | VM Output | Performance |
| ------------- | ------------- | --------- | ----------- |
| Objects       | ✅ Match      | ✅ Match  | ~4x faster  |
| Functions     | ✅ Match      | ✅ Match  | ~4x faster  |
| Recursion     | ✅ Match      | ✅ Match  | ~30x faster |
| Loops         | ✅ Match      | ✅ Match  | ~30x faster |
| Fibonacci(25) | 75025         | 75025     | ~30x faster |

**Build Pipeline:**

```bash
./target/release/oite compiler/main.ot llvm input.ot  # → input.ot.ll
clang input.ot.ll -c -o input.o                          # → input.o
clang input.o -o output                                     # → native binary
```

**Key Features:**

- Complete LLVM IR generation with inlined runtime
- NaN-boxing for all values (numbers, strings, objects, arrays)
- Object/array allocation and property access
- Function calls and recursion
- Control flow (if/else, while, for)
- No external runtime library needed

**Bootstrap Verification** ✅

```
tscl₀ (Rust) ──► tscl₁ (native oitec)
                      │
                      └──► tscl₂ (self-compiled)
                                 │
                                 └──► verify: hash(tscl₁) == hash(tscl₂) ✅
```

**Verification Completed:**
- Bytecode generation is deterministic (same source → same bytecode)
- All 8 bootstrap modules compile successfully via self-hosted compiler
- Self-compilation produces identical output across generations
- hash(gen₀) == hash(gen₁) == hash(gen₂) verified

**Verification Tools:**
- `tests/compiler/bootstrap_verify.ot` - Comprehensive verification test suite
- `scripts/bootstrap_verify.sh` - Shell script for end-to-end verification

#### Bootstrap Compiler (Working - `bootstrap/`)

Reference implementation, flat file structure (~5,000 lines):

```
bootstrap/
├── main.ot           # CLI entry point (273 lines)
├── types.ot          # Type definitions (357 lines)
├── lexer.ot          # Tokenization (335 lines)
├── parser.ot         # AST generation (1,432 lines)
├── ir.ot             # IR types (619 lines)
├── ir_builder.ot     # AST → IR (270 lines)
├── codegen.ot        # IR → Bytecode (315 lines)
├── emitter.ot        # Bytecode serialization (846 lines)
├── pipeline.ot       # Compilation orchestration (228 lines)
├── stdlib.ot         # Runtime declarations (248 lines)
└── utils.ot          # Helpers (22 lines)
```

#### Modular Compiler (Target - `compiler/`)

Production compiler in modular structure (~3,500 lines, growing).

**CLI Commands:** All working on Rust VM:

| Command        | Status | Description                 |
| -------------- | ------ | --------------------------- |
| `ast <file>`   | ✅     | Output JSON AST             |
| `ir <file>`    | ✅     | Output SSA IR               |
| `check <file>` | ✅     | Type check + borrow check   |
| `build <file>` | ✅     | Compile to bytecode         |
| `run <file>`   | ✅     | Generate bytecode for VM    |
| `llvm <file>`  | ✅     | Generate LLVM IR (.ll file) |

**Recent Fixes:**

- IR opcode serialization (ADD/SUB/MUL/DIV display correctly)
- Function name collision (`getOpCodeForBinaryOp` renamed to `getIrOpCodeForBinaryOp` in IR builder)
- VM fall-through bug workaround (explicit `return` statements in emitter functions)
- Bytecode string encoding (varint-prefixed strings for decoder compatibility)
- Array element storage order (correct stack order for StoreElement)
- Variable declaration format handling (parser format compatibility)
- Number lexing bug fix (digits were being duplicated due to missing advance)
- Unique block labels in IR builder (fixed duplicate labels causing infinite loops in LLVM IR)

**IR Builder Features Implemented:**

- Break/continue with loop context tracking
- Member expressions (property and element access)
- Property/element assignment
- Array initialization with element storage
- Object initialization with property storage
- Conditional expression (ternary) with value merging
- Basic function expressions (closures foundation)
- Try/catch/finally block lowering

**LLVM IR Backend Features:**

- Complete LLVM IR text generation from SSA IR
- Inlined runtime (no external library needed)
- NaN-boxing for all value types
- Object/array allocation with property access
- String concatenation and number-to-string conversion
- All comparison and arithmetic operators
- Function calls and recursion
- Control flow with unique block labels

**Bytecode Generation Verified:**

- Arrays, objects, functions compile and execute correctly
- Control flow (while, if, break) works properly
- Function calls with parameters verified
- Console output confirmed working

**Architecture:**

```
compiler/
├── main.ot           # CLI entry point
├── lexer/              # Tokenization module
│   ├── mod.ot
│   ├── token.ot
│   └── error.ot
├── parser/             # AST generation module
│   ├── mod.ot
│   ├── expr.ot
│   ├── stmt.ot
│   └── error.ot
├── ast/                # AST type definitions
│   ├── mod.ot
│   └── types.ot
├── ir/                 # IR system
│   ├── mod.ot
│   └── builder.ot
├── codegen/            # Code generation
│   └── mod.ot
├── passes/             # Compiler passes (working)
│   ├── typecheck.ot
│   ├── opt.ot
│   └── borrow_ck.ot
├── backend/            # Native codegen (LLVM IR)
│   └── llvm/
│       ├── mod.ot    # LLVM IR emitter (~1,350 lines)
│       ├── runtime.ot # Runtime function stubs
│       └── types.ot  # Type mappings
└── stdlib/
    └── builtins.ot
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

Official libraries built on Oite core:

| Roll               | Purpose                          |
| ------------------ | -------------------------------- |
| `@rolls/async`     | Work-stealing executor, io_uring |
| `@rolls/tls`       | TLS encryption via rustls        |
| `@rolls/http`      | HTTP/1.1, HTTP/2 server          |
| `@rolls/websocket` | WebSocket protocol               |
| `@rolls/fs`        | File system operations           |
| `@rolls/path`      | Path utilities                   |
| `@rolls/json`      | JSON parse/stringify             |
| `@rolls/math`      | Math functions                   |
| `@rolls/date`      | Date/time handling               |
| `@rolls/string`    | String methods                   |
| `@rolls/array`     | Array methods                    |
| `@rolls/promise`   | Promise implementation           |

See `docs/future/rolls-design.md` for detailed architecture.

### Unroll (Tooling)

Package manager and developer tools:

| Component      | Purpose                   |
| -------------- | ------------------------- |
| `unroll new`   | Create new project        |
| `unroll add`   | Add Roll dependency       |
| `unroll build` | Build with static linking |
| `unroll run`   | Build and run             |
| `unroll fmt`   | Code formatter            |
| `unroll lint`  | Linter                    |
| LSP            | Language server           |

See `docs/future/unroll-design.md` for detailed architecture.

---

## Testing & Performance

### Test Suite

```
60+ tests passed
```

Coverage includes: IR lowering, type inference, optimizations, borrow checker, JIT compilation, LLVM backend, language features.

### Performance Benchmarks

| Metric          | VM           | JIT             | Speedup |
| --------------- | ------------ | --------------- | ------- |
| Arithmetic      | 2.34 µs/iter | 0.39 µs/iter    | ~6x     |
| JIT compilation | -            | 980 µs          | -       |
| Break-even      | -            | ~500 iterations | -       |

### Performance Targets

| Benchmark | Node.js | Bun   | Target |
| --------- | ------- | ----- | ------ |
| fib(35)   | 50 ms   | 30 ms | 20 ms  |
| Startup   | 30 ms   | 10 ms | 5 ms   |

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
./target/release/oite jit app.ot

# VM execution
./target/release/oite run app.ot

# Build native binary
./target/release/oite build app.ot --release -o app

# Run tests
cargo test
```

---

## Key Design Decisions

| Area                 | Decision                               |
| -------------------- | -------------------------------------- |
| Value representation | 64-bit NaN-boxed words                 |
| Module system        | Native ES Modules (no CommonJS)        |
| Memory model         | Rust-style ownership + borrow checking |
| Async runtime        | Minimal core (epoll/kqueue reactor)    |
| Standard library     | Minimal core; extended via Rolls       |

---

## Project Structure

```
oite/
├── Cargo.toml                    # Minimal dependencies
├── compiler/                     # Self-hosted compiler (modular .ot)
├── bootstrap/                    # Bootstrap compiler (flat .ot files)
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
