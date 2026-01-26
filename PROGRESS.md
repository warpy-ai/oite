# Script: Development Progress

High-performance systems language with **TypeScript syntax** compiling to **native code** via **Cranelift JIT** and **LLVM AOT**.

**Goals:** Faster than Bun, Actix-level server performance, familiar JS/TS syntax, standalone native binaries.

---

## Quick Status

| Phase   | Status      | Description                       |
| ------- | ----------- | --------------------------------- |
| Phase 0 | ✅ Complete | Runtime Kernel Foundation         |
| Phase 1 | ✅ Complete | SSA IR System                     |
| Phase 2 | ✅ Complete | Native Backend (Cranelift + LLVM) |
| Phase 3 | ✅ Complete | Language Completion               |
| Phase 4 | ✅ Complete | Self-Hosting Compiler             |

**Current Focus:** Language core is complete. Library functionality (HTTP, TLS, fs, etc.) will be developed in the **Rolls** ecosystem (separate repository).

---

## Architecture

**Script Core is like C without libc** — minimal, self-contained, runs without dependencies.

```
┌───────────────────────────────────┐  ┌──────────────────────────────────┐
│         SCRIPT CORE               │  │            ROLLS                 │
│  ✅ Always available              │  │  ⚡ Optional system libraries    │
│                                   │  │                                  │
│  • Compiler (scriptc)             │  │  • @rolls/http, @rolls/tls       │
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
| JIT         | `script jit app.tscl`                    | Fast development, benchmarking  |
| AOT Release | `script build app.tscl --release -o app` | Production (ThinLTO)            |
| AOT Dist    | `script build app.tscl --dist -o app`    | Maximum optimization (Full LTO) |
| VM          | `script run app.tscl`                    | Debugging, REPL, compatibility  |

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

| Module       | Methods                                                      |
| ------------ | ------------------------------------------------------------ |
| `console`    | `log`, `error`                                               |
| `String`     | `fromCharCode`                                               |
| `ByteStream` | Binary data manipulation for bootstrap compiler              |
| `fs`         | `readFileSync`, `writeFileSync`, `writeBinaryFile` (minimal) |
| `require`    | Module loading                                               |

> **Note:** Full standard library (Math, Date, JSON, comprehensive fs/path, HTTP, TLS, etc.) will be provided by the **Rolls** ecosystem. See `docs/future/rolls-design.md`.

---

### Phase 4: Self-Hosting Compiler ✅

Fully self-hosted compiler (`scriptc`) written in Script with TypeScript support.

#### Current State

| Compiler          | Location           | Status            | Output                   |
| ----------------- | ------------------ | ----------------- | ------------------------ |
| **Rust Compiler** | `src/compiler/`    | ✅ Production     | Native binaries          |
| **Bootstrap**     | `bootstrap/*.tscl` | ✅ Self-Compiling | Bytecode                 |
| **Modular**       | `compiler/*.tscl`  | ✅ Working        | Bytecode (VM executable) |

#### Self-Compilation Verified ✅

The bootstrap compiler can now compile itself! All 8 modules successfully self-compile:

| Module          | Compiled Size | Purpose                   |
| --------------- | ------------- | ------------------------- |
| types.tscl      | 37 bytes      | Type definitions          |
| lexer.tscl      | 1,325 bytes   | Tokenization              |
| parser.tscl     | 7,947 bytes   | AST generation            |
| emitter.tscl    | 4,547 bytes   | Bytecode serialization    |
| ir.tscl         | 2,766 bytes   | IR types                  |
| ir_builder.tscl | 1,363 bytes   | AST → IR                  |
| codegen.tscl    | 1,580 bytes   | IR → Bytecode             |
| pipeline.tscl   | 969 bytes     | Compilation orchestration |

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

**Phase 1:** Foundation ✅

```
Source → bootstrap/*.tscl → Bytecode → Rust VM
Source → src/compiler/ (Rust) → Native Binary ← Production builds
```

**Phase 2 (Complete):** Feature Parity ✅

```
Source → compiler/*.tscl → Bytecode → Rust VM
         + Type inference, optimizations, borrow checking
         + All CLI commands working: ast, ir, check, build, run
```

**Phase 3 (Complete):** Native Code Generation ✅

```
Source → compiler/*.tscl → LLVM IR (.ll) → clang → Native Binary
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
./target/release/script compiler/main.tscl llvm input.tscl  # → input.tscl.ll
clang input.tscl.ll -c -o input.o                          # → input.o
clang input.o -o output                                     # → native binary
```

**Key Features:**

- Complete LLVM IR generation with inlined runtime
- NaN-boxing for all values (numbers, strings, objects, arrays)
- Object/array allocation and property access
- Function calls and recursion
- Control flow (if/else, while, for)
- No external runtime library needed

**Phase 4:** Bootstrap Verification ✅

```
tscl₀ (Rust) ──► tscl₁ (native scriptc)
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
- `tests/compiler/bootstrap_verify.tscl` - Comprehensive verification test suite
- `scripts/bootstrap_verify.sh` - Shell script for end-to-end verification

#### Bootstrap Compiler (Working - `bootstrap/`)

Reference implementation, flat file structure (~5,000 lines):

```
bootstrap/
├── main.tscl           # CLI entry point (273 lines)
├── types.tscl          # Type definitions (357 lines)
├── lexer.tscl          # Tokenization (335 lines)
├── parser.tscl         # AST generation (1,432 lines)
├── ir.tscl             # IR types (619 lines)
├── ir_builder.tscl     # AST → IR (270 lines)
├── codegen.tscl        # IR → Bytecode (315 lines)
├── emitter.tscl        # Bytecode serialization (846 lines)
├── pipeline.tscl       # Compilation orchestration (228 lines)
├── stdlib.tscl         # Runtime declarations (248 lines)
└── utils.tscl          # Helpers (22 lines)
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
├── passes/             # Compiler passes (working)
│   ├── typecheck.tscl
│   ├── opt.tscl
│   └── borrow_ck.tscl
├── backend/            # Native codegen (LLVM IR)
│   └── llvm/
│       ├── mod.tscl    # LLVM IR emitter (~1,350 lines)
│       ├── runtime.tscl # Runtime function stubs
│       └── types.tscl  # Type mappings
└── stdlib/
    └── builtins.tscl
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
