## tscl: Development Progress

High-performance systems language with **JavaScript syntax** that compiles to **native code** via **Cranelift JIT** and **LLVM AOT + LTO**.

- **Goal:** Faster than Bun, Actix-level server performance, JS syntax, native binaries.
- **Execution modes:** Native-first (JIT/AOT) with VM as a development / debugging tool.
- **Current phase:** **Phase 3 – Language Completion (JS compatibility) ~complete**, preparing for self‑hosting and server/runtime work.

---

## 1. Architecture Overview

### 1.1 High-Level Architecture

- **Original (VM-first):**

```text
tscl source → Rust compiler → Bytecode → Stack-based VM → CPU
```

- **Target (Native-first):**

```text
tscl source → Compiler → SSA IR → Native backend (Cranelift/LLVM) → CPU
                         ↓
                  Borrow checker
                  Type inference
                  Optimizations
```

- VM remains for:
  - Debugging and testing
  - Bootstrapping / experimentation
  - A compatibility fallback when native backend is unavailable

### 1.2 Backends

- **Cranelift JIT** – fast dev and benchmarking:
  - `./target/release/script jit <file.tscl>`
- **LLVM AOT + LTO** – optimized native binaries:
  - `./target/release/script build app.tscl --release -o app`
  - `./target/release/script build app.tscl --dist -o app  # Full LTO`

---

## 2. Phase Roadmap (High-Level)

- **Phase 0 – Runtime Kernel Foundation** ✅
- **Phase 1 – SSA IR System** ✅
- **Phase 2 – Native Backend (Cranelift JIT + LLVM AOT + LTO)** ✅
- **Phase 3 – Language Completion / JS Compatibility Layer** ✅ core, 🚧 modules/async
- **Phase 4 – Self-Hosting Compiler** 🚧 (design + migration)
- **Phase 5 – Runtime & Server (HTTP, async runtime)** 🚧
- **Phase 6 – Tooling (fmt, lint, LSP, profiler)** 🚧
- **Phase 7 – Distribution (packages, installers, binaries)** 🚧

The rest of this document walks through these phases **in order**, then summarizes **testing, performance, and current focus**.

---

## 3. Phase 0 – Runtime Kernel Foundation ✅

**Goal:** Separate runtime primitives from any single execution engine (VM/JIT/AOT).

### 3.1 Files

- `src/runtime/mod.rs` – runtime module root
- `src/runtime/abi.rs` – NaN-boxed `TsclValue` ABI
- `src/runtime/heap.rs` – bump allocator, object layouts
- `src/runtime/stubs.rs` – `extern "C"` stubs for JIT/AOT

### 3.2 Runtime ABI

- All values are represented as a **64‑bit NaN‑boxed** word:
  - Booleans, null, undefined, pointers encoded in NaN space.

Key idea: **uniform 64‑bit value** that both VM and native backends can understand.

### 3.3 Runtime Stubs (20+)

- **Allocation:** `tscl_alloc_object`, `tscl_alloc_array`, `tscl_alloc_string`
- **Property access:** `tscl_get_prop`, `tscl_set_prop`, `tscl_get_element`, `tscl_set_element`
- **Arithmetic:** `tscl_add_any`, `tscl_sub_any`, `tscl_mul_any`, `tscl_div_any`, `tscl_mod_any`
- **Comparisons / logic:** `tscl_eq_strict`, `tscl_lt`, `tscl_gt`, `tscl_not`, `tscl_neg`
- **Conversions:** `tscl_to_boolean`, `tscl_to_number`
- **I/O & calls:** `tscl_console_log`, `tscl_call`

These are the **stable ABI surface** that backends call into.

---

## 4. Phase 1 – SSA IR System ✅

**Goal:** Transform stack-based bytecode into a **register-based SSA IR** with type tracking and basic optimizations.

### 4.1 Files

- `src/ir/mod.rs` – IR data structures and ownership
- `src/ir/lower.rs` – bytecode → SSA lowering
- `src/ir/typecheck.rs` – flow-sensitive type inference
- `src/ir/opt.rs` – DCE, constant folding, CSE, copy propagation
- `src/ir/verify.rs` – IR validation + borrow checking
- `src/ir/stubs.rs` – mapping IR ops → runtime stubs / inline code

### 4.2 IR Design

- **Types** (high level):
  - `Number`, `String`, `Boolean`, `Object`, `Array`, `Function`, `Any`, `Never`, `Void`
- **Ownership:**
  - `Owned`, `Moved`, `BorrowedImm`, `BorrowedMut`, `Captured`
- **Storage:**
  - `Stack`, `Heap`, `Register`
- **Operations** (subset):
  - Constants: `Const`
  - Arithmetic: `AddNum`, `SubNum`, `MulNum` and dynamic `AddAny`, `SubAny`, ...
  - Control flow: `Jump`, `Branch`, `Return`, `Phi`

### 4.3 Lowering: Bytecode → SSA

- Bytecode stack ops become **explicit SSA values**:
  - `Push(v)` → `Const(r, v)`
  - `Add` → `AddAny(dst, a, b)` (specialized later)
  - `Load(name)` → `LoadLocal(dst, slot)`
  - `Jump(addr)` → `Jump(block)`
  - `JumpIfFalse(addr)` → `Branch(cond, true_block, false_block)`
  - `Call(n)` → `Call(dst, func, args...)`

CLI to inspect IR:

```bash
./target/release/script ir <filename>
```

Prints:
- Bytecode
- SSA before optimization
- SSA after type inference
- SSA after optimizations

### 4.4 Type Inference & Specialization

- Forward dataflow propagates static types; dynamic ops specialize when possible:

```text
// Before:
v2 = add.any v0, v1   // v0: number, v1: number

// After:
v2 = add.num v0, v1   // specialized to numeric add
```

### 4.5 Optimization Passes

- Dead Code Elimination (DCE)
- Constant folding
- Common Subexpression Elimination (CSE)
- Copy propagation
- Branch simplification
- Unreachable block elimination

### 4.6 IR Verification & Borrow Rules

- SSA validation: exactly‑once definitions
- Control flow validation for jumps and blocks
- Use‑after‑move detection
- Borrow checker rules:
  - No overlapping mutable borrows
  - Ownership and lifetime sanity

---

## 5. Phase 2 – Native Backend ✅

Phase 2 is implemented in **three sub-steps**, all complete:

1. **2A – Cranelift JIT backend**
2. **2B – Multi-function JIT + tiered compilation**
3. **2C – LLVM AOT backend + LTO (called “2B‑Gamma” in earlier notes)**

### 5.1 2A – Cranelift JIT Backend

**Goal:** Execute SSA IR as native machine code at runtime.

**Key files:**
- `src/backend/mod.rs` – backend manager, target selection
- `src/backend/layout.rs` – memory layout for structs/arrays/frames
- `src/backend/cranelift.rs` – IR → Cranelift IR
- `src/backend/jit.rs` – JIT runtime
- `src/backend/aot.rs` – AOT scaffold (superseded by LLVM path)
- `src/backend/tier.rs` – tiered compilation

**Backend configuration:**

- `BackendKind::CraneliftJit | CraneliftAot | Interpreter`
- `OptLevel::None | Speed | SpeedAndSize`

**Cranelift integration:**

- Each `IrOp` becomes Cranelift instructions or stub calls
- Specialized numeric ops (`AddNum`, `SubNum`, etc.) compile to FP instructions
- Dynamic ops (`AddAny`, etc.) call `tscl_*` runtime stubs
- NaN-boxed 64-bit values respected end-to-end

**JIT runtime API:**

- `JitRuntime::compile(&IrModule)`
- `JitRuntime::call_main()`
- `JitRuntime::call_func(name, args)`

**CLI:**

```bash
./target/release/script jit <filename>
```

### 5.2 2B – Multi-Function JIT + Tiered Compilation

**Goals:**
- Support multiple functions, recursion, closures, and phi nodes
- Enable tiered compilation based on hotness

#### 5.2.1 Function Extraction

- Inline function bodies in bytecode are extracted as separate IR functions.

```text
Bytecode:
  [0] Push(Function { address: 3, env: None })
  [1] Let("fib")
  [2] Jump(23)
  [3] Let("n")          // function body
  ...
  [22] Return
  [23] ...              // main

IR:
  fn func_3(n: any) { ... }  // extracted function
  fn main() { ... }          // main calls func_3
```

#### 5.2.2 Call Resolution & Recursion

- All functions declared/numbered before compilation.
- Constant propagation tracks function addresses through local slots, enabling **direct calls**:

```text
v0 = const 3        // function address
store.local $0, v0
v2 = load.local $0  // still known to be func_3
v3 = call v2(v1)    // direct call to compiled func_3
```

#### 5.2.3 Phi Nodes

- IR uses explicit `Phi`; Cranelift uses block parameters.
- Translation:

```text
IR:
  bb2: phi v5 = [(bb0, v1), (bb1, v3)]

Cranelift:
  bb2(v5: i64):
    ...
  bb0: jump bb2(v1)
  bb1: jump bb2(v3)
```

#### 5.2.4 Tiered Compilation

- `TierManager` tracks call counts and compiled functions:
  - Baseline threshold (e.g. 100 calls)
  - Optimizing threshold (e.g. 1000 calls)
- VM feeds `function_call_counts` into tier manager to identify hot functions.

**Benchmark command:**

```bash
./target/release/script bench examples/bench_arithmetic.tscl
```

Example result:

```text
=== Summary ===
VM:        2.34 µs/iter
JIT:       0.39 µs/iter
JIT compilation:  980 µs

JIT is 5.98x faster than VM
Break-even point: 503 iterations
```

### 5.3 2C – LLVM AOT Backend + LTO ✅

**Goal:** Produce standalone native binaries with LLVM 18 and LTO.

#### 5.3.1 Prerequisites

```bash
brew install llvm@18
brew install zstd
export LLVM_SYS_180_PREFIX=$(brew --prefix llvm@18)
```

#### 5.3.2 Files

- `src/backend/llvm/mod.rs` – orchestration
- `src/backend/llvm/types.rs` – IR types → LLVM types
- `src/backend/llvm/codegen.rs` – IR → LLVM IR
- `src/backend/llvm/abi.rs` – runtime stub declarations & IR implementations
- `src/backend/llvm/optimizer.rs` – LLVM optimization pipeline (new pass manager)
- `src/backend/llvm/object.rs` – object file emission
- `src/backend/llvm/linker.rs` – static linking with embedded runtime

#### 5.3.3 Architecture

- **Type lowering:** `Number` → `double`, `Boolean` → `i1`, heap pointers → `i64`/structs
- **Function translation:** SSA functions → LLVM functions with basic blocks
- **Ops translation:** arithmetic, comparisons, branches, loads/stores
- **Runtime integration:** stubs implemented directly in LLVM IR:
  - `tscl_console_log` uses libc `printf`
  - Arithmetic, negation, and function calls implemented without Rust runtime
- **Emission:**
  - `.o` object files per module
  - `.bc` bitcode emission for per-module LTO
  - ThinLTO for `--release`, full LTO for `--dist`

#### 5.3.4 Usage

```bash
# Dev build (no LTO)
./target/release/script build app.tscl --release -o app

# Dist build (full LTO)
./target/release/script build app.tscl --dist -o app

# Example (Fibonacci)
./target/release/script build ./examples/test_fib.tscl --release -o test_fib
./test_fib   # prints 55
```

#### 5.3.5 Notes / Limitations

- Pipeline uses simplified set of LLVM 18 passes (new pass manager)
- Some advanced runtime features (objects, strings, full stdlib) still rely on a fuller runtime library

---

## 6. Type System Implementation ✅

**Goal:** Static type system with **TypeScript-style syntax** and **Rust-style ownership**.

> Originally planned as a later phase; now **fully integrated** across compiler and IR.

### 6.1 Features

- **Type annotations:**
  - `let x: number = 42`
  - `function add(a: number, b: number): number`
  - `let arr: string[] = ["a", "b"]`
  - Optional annotations with **Hindley–Milner inference**
- **Ownership & borrowing:**
  - `Ref<T>` / `&T` (immutable ref)
  - `MutRef<T>` / `&mut T` (mutable ref)
  - Move semantics for heap values, copy for primitives
  - Integrated with borrow checker and IR
- **Generics:**
  - Generic functions and structs
  - Monomorphization / specialization at compile time
  - Type inference for generic arguments

### 6.2 Architecture

- `src/types/mod.rs` – core type representation
- `src/types/checker.rs` – type checking logic
- `src/types/inference.rs` – inference engine
- `src/types/registry.rs` – named types
- `src/types/convert.rs` – coercions / conversions
- `src/types/error.rs` – diagnostics
- `src/compiler/borrow_ck.rs` – borrow checker

---

## 7. Phase 3 – Language Completion / JS Compatibility ✅ (Core)

**Goal:** Make tscl a practical **JavaScript superset** (with types + ownership).

Status:
- Control flow, error handling, classes, decorators: ✅
- Modules (`import`/`export`), async/await, full stdlib: 🚧

### 7.1 Control Flow ✅

Implemented:
- `if` / `else`
- `while` loops
- `for` loops (`for (init; test; update)`)
- `do..while` loops
- `break` / `continue`
- Basic label support

Implementation notes:
- `LoopContext` tracks `start_addr`, `continue_addr`, `break_jumps`, `continue_jumps`
- For loops use `usize::MAX` as sentinel for `continue_addr` (backpatch)
- `continue` jumps to **update expression**, not condition

### 7.2 Error Handling ✅

Implemented:
- `try` / `catch` / `finally`
- `throw`
- Exception propagation and stack unwinding

Key opcodes:
- `Throw`
- `SetupTry { catch_addr, finally_addr }`
- `PopTry`
- `EnterFinally(bool)`

VM maintains an `ExceptionHandler` stack with:
- Target addresses
- Stack depths to unwind to

### 7.3 Classes & OOP ✅ (Prototype Chain)

Implemented:
- ES6 class syntax
- Constructors
- Instance + static methods/properties
- `extends` inheritance
- `super()` constructor calls
- `super.method()` calls (prototype chain lookup)
- Property initializers
- Getters/setters (syntax)
- Private field/method syntax (`#field`, `#method`)
- `new.target`, `extends` with expressions, decorators on classes

Prototype chain layout (example):

```typescript
class Animal {
    constructor(name) { this.name = name; }
    speak() { return this.name + " makes a sound"; }
}

class Dog extends Animal {
    constructor(name, breed) {
        super(name);
        this.breed = breed;
    }
    speak() { return this.name + " barks!"; }
}

let dog = new Dog("Buddy", "Golden");
```

Structure:
- `Dog` wrapper:
  - `constructor` → Dog constructor
  - `prototype` → `Dog.prototype`
  - `__super__` → Animal wrapper
- `Dog.prototype`:
  - `constructor` → Dog
  - `__proto__` → `Animal.prototype`
  - `speak` → Dog’s speak
- `Animal.prototype`:
  - `constructor` → Animal
  - `speak` → base method
- `dog` instance:
  - own fields (`name`, `breed`)
  - `__proto__` → `Dog.prototype` → `Animal.prototype`

VM/compiler changes:
- `Construct` opcode:
  - Extracts `__super__` from wrapper and stores in frame
- `CallSuper`:
  - Uses frame’s `__super__` for constructor chaining
- `GetSuperProp`:
  - Supports `super.method()` lookups via prototype chain
- Compiler:
  - Compiles `super()` to `LoadSuper` + `CallSuper`
  - Handles `Expr::SuperProp` and `Expr::Cond` (for `extends (cond ? A : B)`)

Remaining class gaps (mostly polish):
- Abstract classes (not implemented)
- Full private-field enforcement (currently syntax-level, not fully hidden)
- Auto-calling getters/setters in **all** access paths
- `instanceof` is implemented for VM; AOT path is limited by borrow checker today

### 7.4 Decorators (Story 5)

**Goal:** TypeScript/JS decorators on classes, methods, and fields.

Test files:
- `tests/decorator-simple.tscl` – ✅ simple decorator works
- `tests/decorator-class-params.tscl` – 🚧 parameterized decorator bug (mostly fixed; see below)

#### 7.4.1 Implemented

- Support for:
  - `@decorator`
  - `@decorator(arg1, arg2)`
- Two-stage decorator pattern:
  - Call factory with args → returns decorator
  - Apply decorator to class / method / field target
- Works for:
  - Class decorators
  - Method decorators
  - Field decorators

Compiler / borrow checker fixes:
- Functions are treated as **primitive** for move semantics (`VarKind::Primitive`), so loading/storing function references no longer causes ownership bugs.
- Return statement duplication bug for arrow functions fixed:
  - If an arrow with block body already emits a `Return`, surrounding code does not emit another.

#### 7.4.2 Known Bug (Fixed Direction)

Problem (original state):
- When a decorator factory returned an **arrow function with a block body**, the arrow’s body was skipped:
  - `Jump` after the `Function` pointed to the wrong instruction (to the caller’s `Return`, not after the arrow body).

Root cause:
- `gen_expr` for `Expr::Arrow(BlockStmt)` computed the **jump target** incorrectly.

Resolution direction:
- Jump target must be **after** the arrow’s `Return` (`after_body + 1`), not at the `Return` itself.
- Once fixed, parameterized decorator bodies run correctly:

```typescript
function classDecorator(value: string, num: number): ClassDecorator {
    console.log("Class decorator called with:", value, num);
    return (target: any) => {
        console.log("Class decorator applied to:", target);
        return target;
    };
}

@classDecorator("test_value", 42)
class TestClass {}
```

### 7.5 Modules 🚧

- Current state:
  - Only `require` style runtime module loading.
- Not yet implemented:
  - `import` / `export` syntax
  - ES module graph and resolution algorithm
  - Tree-shaking, side-effect analysis, circular deps handling

### 7.6 Async/Await 🚧

- Current state:
  - Borrow checker is aware of async closures
- Not yet implemented:
  - `async` function syntax
  - `await` expression
  - Promise type and event loop integration
  - Zero-cost futures and proper async runtime

### 7.7 Standard Library Surface

Implemented:
- `console.log`
- `setTimeout`
- `require` (basic)
- `fs.readFileSync`
- `fs.writeFileSync`
- `fs.writeBinaryFile`
- `ByteStream`

Planned:
- Rich `fs`, `net`, `http`, `crypto`, `process`, `os` modules

---

## 8. Original VM System (Complete)

Even though tscl is now **native-first**, the VM remains important and mature.

### 8.1 Bootstrap Compiler (Self-hosting VM path)

- `bootstrap/lexer.tscl` – lexer
- `bootstrap/parser.tscl` – recursive descent parser
- `bootstrap/emitter.tscl` – bytecode emitter
- Two-stage loading:
  - Prelude, then bootstrap modules, then main script
- Bytecode rebasing for appended modules

### 8.2 Memory & Ownership in VM

- Ownership model:
  - Primitives on stack (copy)
  - Objects/arrays on heap (move)
- `Let` vs `Store`:
  - `Let` introduces new bindings (shadowing)
  - `Store` updates existing bindings
- Scoped lifetimes:
  - Variables freed automatically at scope end
- Variable lifting:
  - Captured variables moved from stack to heap for closures

### 8.3 VM Features

- Stack-based architecture with call frames
- Heap allocation for objects, arrays, ByteStreams
- Native bridge: Rust functions injected into JS environment
- Event loop with `setTimeout`
- Stack overflow protection (max call depth ~1000)

### 8.4 Language Support (VM)

- Variables: `let`, `const`
- Objects and arrays with property/element access
- Control flow: `if`, `while`, `for`, `do..while`, `break`, `continue`
- Exceptions: `try` / `catch` / `finally`, `throw`
- Classes: ES6 syntax with inheritance, `super()`, getters/setters
- Operators: arithmetic, comparisons, logical, unary
- String and array methods (subset of JS)

### 8.5 Bytecode Instruction Set (Summary)

Examples:
- `Push(Value)`, `Let(Name)`, `Store(Name)`, `Load(Name)`
- `StoreLocal(idx)`, `LoadLocal(idx)`
- `NewObject`, `NewArray(Size)`
- `SetProp(Key)`, `GetProp(Key)`
- `StoreElement`, `LoadElement`
- `Call(ArgCount)`, `CallMethod(N,A)`
- `Return`, `Jump(Addr)`, `JumpIfFalse(Addr)`
- `MakeClosure(Addr)`, `Construct(Args)`
- `Drop(Name)`, `Dup`, `Pop`
- Arithmetic, equality, comparison, logical, `Neg`
- `Require`, `Halt`
- Exception opcodes: `Throw`, `SetupTry`, `PopTry`, `EnterFinally`
- Class inheritance opcodes: `SetProto`, `LoadSuper`, `CallSuper`, `GetSuperProp`

---

## 9. Testing & Performance

### 9.1 Test Suite

Current status:

```text
60+ tests passed, 0 failed
```

Coverage:
- IR lowering (control flow, loops, functions, variables)
- Type inference and specialization
- Constant folding, DCE, CSE
- IR verification and ownership rules
- Runtime stubs and heap allocation
- NaN-boxing behavior
- VM functionality
- Borrow checker and closures
- Backend:
  - Cranelift codegen creation
  - JIT runtime and function compilation
  - Memory layout
  - AOT target detection and LLVM backend
  - Function extraction, multi-function compilation
  - Call resolution and phi handling
  - Tiered compilation manager
- Language features:
  - For loops, do–while
  - Try/catch/finally and throw
  - Classes with inheritance, `super()`, getters/setters, private syntax
  - Decorators (simple + parameterized scenarios)

### 9.2 Performance Targets

Target benchmarks (vs Node/Bun):

| Benchmark          | Node.js | Bun  | Target tscl |
|--------------------|--------:|-----:|------------:|
| HTTP hello world   | 100k rps | 200k rps | 250k rps |
| JSON parse         | 1x      | 1.5x | 2x          |
| `fib(35)`          | 50 ms   | 30 ms | 20 ms      |
| Startup            | 30 ms   | 10 ms | 5 ms       |

JIT vs VM:
- JIT currently ~6x faster than VM on arithmetic microbenchmarks.

---

## 10. Future Phases

### 10.1 Phase 4 – Self-Hosting Compiler 🚧

**Goal:** `tscl` compiles `tscl` → native → `tscl`.

Current state:

```text
tscl(tscl) → bytecode → Rust VM
```

Target:

```text
tscl(tscl) → SSA → LLVM → native
```

Tasks:
- Stable IR format + deterministic lowering
- Emit SSA IR from bootstrap compiler instead of VM bytecode
- Replace VM backend with Cranelift/LLVM
- Compile compiler as a tscl program and link native binary
- Remove VM dependency from compiler path (or keep as dev-only tool)

Self-hosting loop:

```text
tscl₀ (Rust) compiles tscl₁
tscl₁ compiles tscl₂
tscl₂ must equal tscl₁ (bit-for-bit)
```

Requires:
- ABI freeze
- Reproducible builds + bit-for-bit output checks
- Bootstrap test suite

### 10.2 Phase 5 – Runtime & Server 🚧

**Goal:** Beat Bun and Actix performance on server workloads.

Planned:
- Async runtime:
  - `epoll` / `kqueue` integration
  - `io_uring` backend (Linux)
  - Work-stealing executor, timers, zero-copy buffers
- HTTP stack:
  - HTTP/1 parser (SIMD-optimized)
  - HTTP/2 support
  - Routing, middleware, streaming, TLS, WebSocket
- Database:
  - PostgreSQL, Redis, SQLite drivers
  - Connection pooling and query builder

### 10.3 Phase 6 – Tooling 🚧

- REPL (`tscl repl`)
- Formatter (`tscl fmt`)
- Linter (`tscl lint`)
- Language Server (LSP)
- Debugger integration
- Profiler + flamegraphs, tracing, CPU/memory profiler

### 10.4 Phase 7 – Distribution 🚧

- `tscl install` and package manager
- Lockfiles (`tscl.lock`), dependency resolution, build caching
- Cross-compilation support
- Official binaries (GitHub Releases), Docker images
- Homebrew formula, apt/rpm packages
- Install docs and onboarding experience

---

## 11. Current Snapshot

**You are here:**

```text
Phase 3: Language Completion – nearly complete
→ ✅ For/while/do..while loops
→ ✅ Try/catch/finally and throw
→ ✅ Classes with proper prototype chain, inheritance, super(), decorators
→ ✅ Type system + borrow checker + generics + NaN-boxed runtime
→ ✅ Cranelift JIT + LLVM AOT + LTO, standalone binaries
→ 🚧 Modules (`import`/`export`)
→ 🚧 Async/await + Promise runtime
→ 🚧 Rich stdlib and server/runtime stack
```

**Next concrete steps:**

1. Strengthen class semantics:
   - Private field enforcement
   - Getter/setter auto-calling in VM/JIT/AOT
   - Consistent `instanceof` across VM and native backends
2. JS modules:
   - `import`/`export`, module graph, resolution, tree-shaking
3. Async/await:
   - `async`/`await`, Promise, event loop integration
4. Start Phase 4:
   - Emit SSA IR from tscl compiler, move toward self-hosted native compiler


### Fix Applied: ApplyDecorator Stack Order

**Bug:** The `ApplyDecorator` implementation was pushing `target` twice instead of `decorator` then `target`, causing it to call `target()(target)` instead of `decorator(target)`.

**Fix in `src/vm/mod.rs:2201-2246`:**
```rust
// Before (WRONG):
self.stack.push(target.clone());
self.stack.push(target);

// After (CORRECT):
self.stack.push(decorator);
self.stack.push(target);
```

**Test Result:**
```
LOG: String("Creating instance...")
LOG: String("DECORATOR CALLED!")  ← Decorator works!
LOG: String("Instance name:") Undefined  ← Field init issue (separate bug)
```
