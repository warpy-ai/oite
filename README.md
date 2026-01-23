<div align="center">
  <h1>Script</h1>
  <p>A high-performance JavaScript-like scripting language with native code execution</p>
  <p>Featuring a self-hosting compiler and Rust-inspired memory safety</p>

  <br/>

  <img src="https://img.shields.io/badge/rust-1.70+-orange.svg" alt="Rust 1.70+"/>
  <img src="https://img.shields.io/badge/tests-60%20passing-brightgreen.svg" alt="Tests"/>
  <img src="https://img.shields.io/badge/license-Apache%202.0-blue.svg" alt="License"/>
</div>

---

## Overview

**Script** is a scripting language that combines JavaScript-like syntax with Rust-inspired memory safety and native code performance.

```javascript
function fib(n) {
    if (n < 2) return n;
    return fib(n - 1) + fib(n - 2);
}

console.log(fib(35));  // Compiled to native code!
```

### Key Features

- **Native Execution** — SSA-based IR compiled to native code via Cranelift/LLVM
- **Link-Time Optimization** — ThinLTO and Full LTO for maximum performance
- **Standalone Binaries** — Self-contained executables with runtime stubs in LLVM IR
- **Memory Safety** — Ownership model with compile-time borrow checking
- **Self-Hosting** — Bootstrap compiler written in Script itself
- **Type Inference** — Flow-sensitive type analysis for optimization
- **JavaScript Syntax** — Familiar syntax with ES6+ features
- **Classes & Inheritance** — ES6 classes with extends, super(), and private fields
- **Error Handling** — try/catch/finally with exception propagation

## Architecture

Script is the **language core** — the compiler, type system, and minimal runtime primitives. Library functionality (HTTP, TLS, file system, etc.) will be provided by the **Rolls** ecosystem in a separate repository.

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
┌─────────────────────────────────────────────────────────────────┐
│                         Script Source                           │
└───────────────────────────┬─────────────────────────────────────┘
                            ▼
┌─────────────────────────────────────────────────────────────────┐
│                     Script Compiler                             │
│  ┌─────────────┐  ┌──────────────┐  ┌────────────────────────┐  │
│  │   Parser    │─▶│ Borrow Check │─▶│   SSA IR Generation    │  │
│  │  (SWC AST)  │  │  (Ownership) │  │ (Type Inference, Opts) │  │
│  └─────────────┘  └──────────────┘  └────────────────────────┘  │
└───────────────────────────┬─────────────────────────────────────┘
                            ▼
┌─────────────────────────────────────────────────────────────────┐
│                    Native Backend                               │
│  ┌─────────────────┐  ┌─────────────────┐  ┌─────────────────┐  │
│  │  Cranelift JIT  │  │   LLVM AOT      │  │   VM (Debug)    │  │
│  │   (Fast)        │  │  (LTO, Native)  │  │  (Interpreter)  │  │
│  └─────────────────┘  └─────────────────┘  └─────────────────┘  │
└───────────────────────────┬─────────────────────────────────────┘
                            ▼
                          CPU
```

## Quick Start

### Prerequisites

**Required for LLVM AOT backend:**

```bash
# Install LLVM 18 (required for AOT compilation)
brew install llvm@18

# Install zstd (required for linking)
brew install zstd

# Set LLVM environment variable (add to ~/.zshrc or ~/.bashrc for persistence)
export LLVM_SYS_180_PREFIX=$(brew --prefix llvm@18)
```

**Note:** The Cranelift JIT backend works without LLVM. LLVM is only required if you want to use the AOT compilation backend.

### Building

```bash
# Build
cargo build --release

# Run a script
./target/release/script myprogram.tscl

# Dump SSA IR (for debugging)
./target/release/script ir myprogram.tscl

# Run with VM (debug mode)
./target/release/script --run-binary output.tscl.bc

# Build to native binary (requires LLVM)
./target/release/script build myprogram.tscl --release -o myprogram

# Run the compiled binary
./myprogram
```

## Language Features

### Variables & Types

```javascript
let x = 42;              // Number
let name = "script";     // String
let active = true;       // Boolean
let data = { key: 1 };   // Object
let items = [1, 2, 3];   // Array
```

### Functions & Closures

```javascript
// Function declaration
function greet(name) {
    return "Hello, " + name + "!";
}

// Arrow functions
let double = x => x * 2;
let add = (a, b) => a + b;

// Closures
function counter() {
    let count = 0;
    return () => {
        count = count + 1;
        return count;
    };
}
```

### Control Flow

```javascript
if (condition) {
    // ...
} else {
    // ...
}

for (let i = 0; i < 10; i++) {
    // ...
    if (done) break;
    if (skip) continue;
}

while (condition) {
    // ...
}

do {
    // ...
} while (condition);
```

### Objects & Arrays

```javascript
let obj = { x: 10, y: 20 };
obj.z = 30;
console.log(obj["x"]);

let arr = [1, 2, 3];
arr.push(4);
let first = arr[0];
```

### Classes & Inheritance

```javascript
class Animal {
    name: string;

    constructor(name: string) {
        this.name = name;
    }

    speak() {
        console.log(this.name + " makes a sound");
    }
}

class Dog extends Animal {
    breed: string;

    constructor(name: string, breed: string) {
        super(name);
        this.breed = breed;
    }

    speak() {
        console.log(this.name + " barks!");
    }
}

let dog = new Dog("Buddy", "Golden Retriever");
dog.speak();  // "Buddy barks!"
```

### Private Fields

Script supports JavaScript-style private fields using the `#` prefix:

```javascript
class Counter {
    #count = 0;           // Private field (only accessible within class)

    increment() {
        this.#count++;
    }

    getCount() {
        return this.#count;  // Can access private field from methods
    }
}

let c = new Counter();
c.increment();
console.log(c.getCount());  // 1

// c.#count;       // ERROR: Private field not accessible outside class
// c["#count"];    // Returns undefined (encapsulation works)
```

### Error Handling

```javascript
try {
    riskyOperation();
} catch (e) {
    console.log("Error: " + e);
} finally {
    cleanup();
}
```

## Memory Model

Script uses a Rust-inspired ownership system:

```javascript
let a = { value: 42 };
let b = a;                // 'a' is MOVED to 'b'
// console.log(a.value);  // ERROR: use after move!
console.log(b.value);     // OK: 42

// Primitives are copied
let x = 10;
let y = x;                // 'x' is COPIED
console.log(x);           // OK: 10
```

### Ownership Rules

1. Each value has exactly one owner
2. Assigning objects **moves** ownership
3. Primitives (numbers, booleans) are **copied**
4. Variables are freed when their scope ends

## SSA IR

Script compiles to an SSA (Static Single Assignment) intermediate representation:

```
// Source: let x = 1 + 2; let y = x * 3;

fn main() -> any {
bb0:
    v0 = const 1
    v1 = const 2
    v2 = add.num v0, v1      // Specialized to numeric add
    store.local $0, v2
    v3 = load.local $0
    v4 = const 3
    v5 = mul.any v3, v4
    return
}

// After optimization:
bb0:
    v2 = const 3             // 1+2 constant-folded!
    store.local $0, v2
    ...
```

### Type Specialization

The type inference pass specializes dynamic operations:

| Before | After | Speedup |
|--------|-------|---------|
| `add.any v0, v1` | `add.num v0, v1` | ~10x |
| `mul.any v0, v1` | `mul.num v0, v1` | ~10x |

## Minimal Standard Library

Script core provides only essential primitives:

### Console

```javascript
console.log("Hello", 42, true);
console.error("Error message");
```

### ByteStream (Binary Data)

Used by the bootstrap compiler for bytecode emission:

```javascript
let stream = ByteStream.create();
ByteStream.writeU8(stream, 0xFF);
ByteStream.writeU32(stream, 12345);
ByteStream.writeF64(stream, 3.14159);
ByteStream.writeString(stream, "hello");
let bytes = ByteStream.toArray(stream);
```

### File I/O (Minimal)

Basic file operations for the bootstrap compiler:

```javascript
let fs = require("fs");
let content = fs.readFileSync("file.txt");
fs.writeFileSync("out.txt", "Hello!");
```

> **Note:** Full standard library functionality (Math, Date, JSON, comprehensive fs/path, etc.) will be provided by the **Rolls** ecosystem in a separate repository. See `docs/future/rolls-design.md` for the planned architecture.

## Project Structure

```
script/
├── Cargo.toml                    # Minimal dependencies
├── README.md                     # This file
├── PROGRESS.md                   # Development status
├── bootstrap/                    # Self-hosted compiler
│   ├── main.tscl                 # CLI entry point
│   ├── lexer.tscl                # Tokenization
│   ├── parser.tscl               # AST generation
│   ├── ir.tscl                   # IR types
│   ├── ir_builder.tscl           # AST → IR
│   ├── codegen.tscl              # IR → Bytecode
│   ├── emitter.tscl              # Bytecode serialization
│   ├── pipeline.tscl             # Compilation orchestration
│   ├── stdlib.tscl               # Runtime declarations
│   ├── types.tscl                # Type definitions
│   └── utils.tscl                # Helpers
├── src/
│   ├── main.rs                   # Entry point
│   ├── lib.rs                    # Library target
│   ├── compiler/
│   │   ├── mod.rs                # Parser → Bytecode
│   │   └── borrow_ck.rs          # Borrow checker
│   ├── ir/
│   │   ├── mod.rs                # SSA IR types
│   │   ├── lower.rs              # Bytecode → IR
│   │   ├── typecheck.rs          # Type inference
│   │   ├── verify.rs             # Validation
│   │   ├── opt.rs                # Optimizations
│   │   └── format.rs             # IR serialization
│   ├── backend/
│   │   ├── mod.rs                # Backend trait
│   │   ├── cranelift.rs          # JIT backend
│   │   ├── jit.rs                # JIT runtime
│   │   ├── layout.rs             # Memory layout
│   │   └── llvm/                 # AOT backend
│   ├── runtime/
│   │   ├── mod.rs                # Runtime module
│   │   ├── abi.rs                # NaN-boxed values
│   │   ├── heap.rs               # Memory allocation
│   │   ├── stubs.rs              # FFI bridge
│   │   └── async/
│   │       ├── mod.rs            # Core async traits
│   │       ├── task.rs           # Task abstraction
│   │       ├── reactor.rs        # Basic epoll/kqueue
│   │       └── runtime_impl.rs   # Simple executor
│   ├── vm/                       # Debug interpreter
│   │   ├── mod.rs                # VM implementation
│   │   ├── value.rs              # Runtime values
│   │   ├── opcodes.rs            # Bytecode opcodes
│   │   └── stdlib_setup.rs       # Minimal setup
│   └── stdlib/
│       └── mod.rs                # console, ByteStream only
├── docs/
│   └── future/                   # Future architecture docs
│       ├── rolls-design.md       # Rolls (system libraries)
│       └── unroll-design.md      # Unroll (tooling)
└── tests/
```

## Development Status

| Phase | Status | Description |
|-------|--------|-------------|
| Phase 0 | Complete | Runtime kernel (NaN-boxing, allocator, stubs) |
| Phase 1 | Complete | SSA IR (lowering, type inference, optimizations) |
| Phase 2 | Complete | Cranelift JIT backend |
| Phase 3 | Complete | LLVM AOT backend with LTO |
| Phase 4 | Complete | Self-hosted bootstrap compiler |

See [PROGRESS.md](PROGRESS.md) for detailed implementation notes.

## Testing

```bash
# Run all tests
cargo test --release

# Run specific IR tests
cargo test --release ir::
```

## Future: Rolls & Unroll

The Script ecosystem will eventually include:

- **Rolls**: Official system libraries (`@rolls/http`, `@rolls/tls`, `@rolls/fs`, etc.)
- **Unroll**: Package manager, build system, and developer tooling

See `docs/future/` for detailed architecture designs.

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

## License

Script is distributed under the terms of the Apache License (Version 2.0).

See [LICENSE](LICENSE) for details.
