# Oite: Development Progress

High-performance systems language with **TypeScript syntax** compiling to **native code** via **Cranelift JIT** and **LLVM AOT**.

**Goals:** Faster than Bun, Actix-level server performance, familiar JS/TS syntax, standalone native binaries.

---

## Quick Status

**Oite compiles to native code. The JIT and AOT backends run the same language
features as the VM (the debugging interpreter); the `require`, `fs`, `JSON`,
`fetch`, `process` and `ByteStream` globals exist only in the VM.** The per-feature matrix is in
[docs/docs/development-status.md](docs/docs/development-status.md), and the
gap analysis is tracked in [#136](https://github.com/warpy-ai/oite/issues/136). Sections below describe design and intent;
where they say ✅ without a golden test behind them, trust the status page.

| Component                                 | Status                                              |
| ----------------------------------------- | --------------------------------------------------- |
| Runtime kernel (NaN-boxing, allocator)    | 🟡 Works; no memory is ever freed ([#123](https://github.com/warpy-ai/oite/issues/123))            |
| SSA IR + optimizations                    | 🟡 Works; refuses what the native backends cannot run (`require`) |
| Native backends (Cranelift JIT, LLVM AOT) | 🟡 Same language features as the VM; see the status page |
| Language features                         | 🟡 On every backend; no generators, regular expressions or tagged templates yet |
| Static type checking                      | 🟡 Annotations checked as warnings; `oitec check` errors; generic calls unchecked ([#190](https://github.com/warpy-ai/oite/issues/190)) |
| Self-hosting compiler                     | 🟡 Cannot compile its own source yet ([#131](https://github.com/warpy-ai/oite/issues/131))         |

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

See `docs/docs/platform-architecture.md` for detailed diagrams and philosophy.

### Compilation Pipeline

```
tscl source → Compiler → SSA IR → Native Backend → CPU
                 ↓
          Borrow Checker
          Type Inference
          Optimizations
```

### Execution Modes

| Mode        | Command                               | Use Case                        |
| ----------- | ------------------------------------- | ------------------------------- |
| JIT         | `oitec jit app.ot`                    | Fast development, benchmarking  |
| AOT Release | `oitec build app.ot --release -o app` | Production (ThinLTO)            |
| AOT Dist    | `oitec build app.ot --dist -o app`    | Maximum optimization (Full LTO) |
| VM          | `oitec app.ot`                        | Debugging                       |

---

## Implementation Details

### Runtime Kernel ✅

Unified runtime primitives shared across VM/JIT/AOT backends.

**Key Components:**

- `src/runtime/abi.rs` - NaN-boxed 64-bit `OtValue` ABI
- `src/runtime/heap.rs` - Bump allocator, object layouts
- `src/runtime/stubs/` - 20+ `extern "C"` stubs for native backends

**Runtime Stubs:** `ot_alloc_object`, `ot_add_any`, `ot_get_prop`, `ot_set_prop`, `ot_call`, `ot_console_log`, etc.

---

### SSA IR System ✅

Register-based SSA IR with type tracking and optimizations.

**Key Files:**

- `src/ir/lower/` - Bytecode → SSA lowering
- `src/ir/typecheck.rs` - Flow-sensitive type inference
- `src/ir/opt.rs` - DCE, constant folding, CSE, copy propagation
- `src/ir/verify.rs` - IR validation + borrow checking

**Optimization Passes:** Dead code elimination, constant folding, common subexpression elimination, copy propagation, branch simplification.

**CLI:** `oitec ir app.ot` - Inspect IR before/after optimization

---

### Native Backend ✅

#### 2A: Cranelift JIT

Fast compilation for development. Each `IrOp` becomes Cranelift instructions or runtime stub calls.

```bash
oitec jit app.ot
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
oitec build app.ot --release -o app  # ThinLTO
oitec build app.ot --dist -o app     # Full LTO
```

**Key Files:** `src/backend/llvm/codegen.rs`, `src/backend/llvm/optimizer.rs`, `src/backend/llvm/linker.rs`

---

### Language Features ✅

Full TypeScript-style language with ownership semantics.

#### Type System

- Type annotations: `let x: number`, `function add(a: number): number`,
  checked as warnings (`oitec check` reports errors); see `docs/docs/types.md`
- Ownership: `Ref<T>`, `MutRef<T>`, move semantics
- Generics: parsed; a type parameter is `any` to the checker

#### Control Flow

- `if`/`else`, `while`, `for`, `do..while`
- `switch` (strict `===` matching, fall-through, `default` anywhere)
- `break`/`continue` with labels; labeled blocks
- `try`/`catch`/`finally`, `throw`; `return`/`break`/`continue` out of a `try` run its `finally`

#### Classes & OOP

- ES6 class syntax with constructors
- `extends`, `super()`, prototype chain
- Static/instance methods and properties
- Getters, setters, private fields, methods and accessors (`#field`, per class, checked on access; `#field in obj`)
- TypeScript parameter properties (`constructor(private x)`)
- Decorators on classes, methods, accessors and fields (TC39 semantics; not parameters)
- Class expressions; arrow functions use the enclosing `this`

#### Modules

- ES module `import`/`export` syntax
- File-based resolution (`.ot`, `.ts`, `.js`)
- Module caching with SHA256 verification

#### Async/Await

- `async` functions and arrows; `await` suspends and resumes as a microtask
- `new Promise`, `.then()` / `.catch()` / `.finally()`, `Promise.resolve` / `reject` / `all` / `race`
- `setTimeout` / `clearTimeout`; unhandled rejections are reported (exit 1)
- Native code compiles async functions as resumable functions with heap frames

#### Minimal Standard Library

Oite core includes only essential primitives:

| Module       | Methods                                                      |
| ------------ | ------------------------------------------------------------ |
| `console`    | `log`, `error`                                               |
| `String`     | `fromCharCode`                                               |
| `Math`       | `floor`, `round`, `min`, `max`, `pow`, `sqrt`, `random`, ... and constants |
| `Date`       | `now` (the bootstrap compiler times itself with it)          |
| Errors       | `Error`, `TypeError`, `ReferenceError`, `RangeError`, `SyntaxError` (written in Oite: `std/core/error.ot`) |
| `ByteStream` | Binary data manipulation for bootstrap compiler              |
| `fs`         | `readFileSync`, `writeFileSync`, `writeBinaryFile` (minimal) |
| `require`    | Module loading                                               |

> **Note:** The rest of the standard library (full `Date`, comprehensive fs/path, HTTP, TLS, etc.) will be provided by the **Rolls** ecosystem. See `docs/docs/platform-architecture.md`.

Runtime errors are exceptions: reading a property of `null`/`undefined` or calling a non-function throws a `TypeError`, an unbound name a `ReferenceError`, and so on. An uncaught exception prints `Uncaught TypeError: ...` and exits with status 1, on the VM and in native code alike.

---

### Self-Hosting Compiler 🟡

Two compilers written in Oite, with TypeScript support, run on the VM.
Self-hosting is partial: neither has compiled its full source into a working
compiler yet ([#131](https://github.com/warpy-ai/oite/issues/131)). `oitec`
itself is the Rust compiler.

#### Current State

| Compiler          | Location         | Status                                   | Output            |
| ----------------- | ---------------- | ---------------------------------------- | ----------------- |
| **Rust Compiler** | `src/compiler/`  | ✅ Production                            | Native binaries   |
| **Bootstrap**     | `bootstrap/*.ot` | 🟡 Runs on the VM                        | Bytecode          |
| **Modular**       | `compiler/*.ot`  | 🟡 Runs on the VM; cannot compile itself | Bytecode, LLVM IR |

#### Per-Module Compilation

Bytecode sizes recorded for each bootstrap module compiled on its own.
Compiling one module at a time is not a self-compilation fixpoint
([#131](https://github.com/warpy-ai/oite/issues/131)):

| Module        | Compiled Size | Purpose                   |
| ------------- | ------------- | ------------------------- |
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

See `docs/docs-compiler/self-hosting.md` for detailed plan.

**Foundation** ✅

```
Source → bootstrap/*.ot → Bytecode → Rust VM
Source → src/compiler/ (Rust) → Native Binary ← Production builds
```

**Feature Parity** 🟡

```
Source → compiler/*.ot → Bytecode → Rust VM
         + Type inference, optimizations, borrow checking
         + CLI commands: ast, ir, check, build, run
```

Not at parity yet: it skips `require`, `interface` and `typeof` with an
"Unknown node type" warning and still reports success
([#131](https://github.com/warpy-ai/oite/issues/131)).

**Native Code Generation** 🟡

```
Source → compiler/*.ot → LLVM IR (.ll) → clang → Native Binary
         The compiler itself still runs on the Rust VM
```

The self-hosted compiler generates LLVM IR for simple programs:

| Test          | Native Output | VM Output | Performance |
| ------------- | ------------- | --------- | ----------- |
| Objects       | ✅ Match      | ✅ Match  | ~4x faster  |
| Functions     | ✅ Match      | ✅ Match  | ~4x faster  |
| Recursion     | ✅ Match      | ✅ Match  | ~30x faster |
| Loops         | ✅ Match      | ✅ Match  | ~30x faster |
| Fibonacci(25) | 75025         | 75025     | ~30x faster |

**Build Pipeline:**

```bash
./target/release/oitec compiler/main.ot llvm input.ot  # → input.ll (run from the repo root)
clang input.ll -c -o input.o                           # → input.o
clang input.o -lm -o output                            # → native binary (-lm for fmod)
```

**Key Features:**

- Complete LLVM IR generation with inlined runtime
- NaN-boxing for all values (numbers, strings, objects, arrays)
- Object/array allocation and property access
- Function calls and recursion
- Control flow (if/else, while, for)
- No external runtime library needed

**Bootstrap Verification** 🟡

The goal is a three-generation fixpoint: compiler₁ compiles the compiler's
full source into compiler₂, and `hash(compiler₁) == hash(compiler₂)`. That
does not hold yet ([#131](https://github.com/warpy-ai/oite/issues/131)).

**What is verified today:**

- Bytecode generation is deterministic: `bootstrap/lexer.ot` compiled twice
  gives the same hash
- LLVM IR generation is deterministic for a small program
- The full compiler source is not compiled, so this is not a fixpoint

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

See `docs/docs/platform-architecture.md` for detailed architecture.

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

See `docs/docs-unroll/design.md` for detailed architecture.

---

## Testing & Performance

### Test Suite

`cargo test --release` runs the unit tests and the golden tests: each program
in `tests/lang/` runs through `oitec` on the backends it lists (VM, JIT, AOT)
and its output is compared with a hand-written expectation.

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
./target/release/oitec jit app.ot

# VM execution
./target/release/oitec run app.ot

# Build native binary
./target/release/oitec build app.ot --release -o app

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
│   │   ├── stubs/                # The native runtime
│   │   └── async/                # Core async primitives
│   ├── vm/                       # Debug interpreter
│   └── stdlib/                   # Minimal: console, ByteStream, fs
├── docs/
│   └── future/                   # Rolls & Unroll designs
└── tests/
```
