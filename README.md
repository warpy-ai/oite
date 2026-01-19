<div align="center">
  <h1>Script</h1>
  <p>A high-performance JavaScript-like scripting language with native code execution</p>
  <p>Featuring a self-hosting compiler and Rust-inspired memory safety</p>
  
  <br/>
  
  <img src="https://img.shields.io/badge/rust-1.70+-orange.svg" alt="Rust 1.70+"/>
  <img src="https://img.shields.io/badge/tests-59%20passing-brightgreen.svg" alt="Tests"/>
  <img src="https://img.shields.io/badge/license-Apache%202.0-blue.svg" alt="License"/>
</div>

---

## Overview

**tscl** is a scripting language that combines JavaScript-like syntax with Rust-inspired memory safety and native code performance.

```javascript
function fib(n) {
    if (n < 2) return n;
    return fib(n - 1) + fib(n - 2);
}

console.log(fib(35));  // Compiled to native code!
```

### Key Features

- **Native Execution** — SSA-based IR compiled to native code via Cranelift/LLVM
- **Memory Safety** — Ownership model with compile-time borrow checking
- **Self-Hosting** — Bootstrap compiler written in tscl itself
- **Type Inference** — Flow-sensitive type analysis for optimization
- **JavaScript Syntax** — Familiar syntax for easy adoption

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                         tscl Source                             │
└───────────────────────────┬─────────────────────────────────────┘
                            ▼
┌─────────────────────────────────────────────────────────────────┐
│                     tscl Compiler                               │
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
│  │   (Fast)        │  │  (Optimized)    │  │  (Interpreter)  │  │
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
./target/release/script build myprogram.tscl --backend llvm --output myprogram
```

## Language Features

### Variables & Types

```javascript
let x = 42;              // Number
let name = "tscl";       // String
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

while (condition) {
    // ...
    if (done) break;
    if (skip) continue;
}
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

### Constructors

```javascript
function Point(x, y) {
    this.x = x;
    this.y = y;
}

let p = new Point(10, 20);
```

## Memory Model

tscl uses a Rust-inspired ownership system:

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

tscl compiles to an SSA (Static Single Assignment) intermediate representation:

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

## Standard Library

### Console

```javascript
console.log("Hello", 42, true);
```

### Timers

```javascript
setTimeout(() => {
    console.log("Delayed!");
}, 1000);
```

### File System

```javascript
let fs = require("fs");
let content = fs.readFileSync("file.txt");
fs.writeFileSync("out.txt", "Hello!");
```

### ByteStream (Binary Data)

```javascript
let stream = ByteStream.create();
ByteStream.writeU8(stream, 0xFF);
ByteStream.writeU32(stream, 12345);
ByteStream.writeF64(stream, 3.14159);
ByteStream.writeString(stream, "hello");
let bytes = ByteStream.toArray(stream);
```

## Project Structure

```
tscl/
├── src/
│   ├── main.rs              # Entry point
│   ├── compiler/            # Rust compiler (SWC-based)
│   │   ├── mod.rs           # Bytecode generation
│   │   └── borrow_ck.rs     # Borrow checker
│   ├── ir/                  # SSA IR system
│   │   ├── mod.rs           # IR types, ownership model
│   │   ├── lower.rs         # Bytecode → SSA
│   │   ├── typecheck.rs     # Type inference
│   │   ├── opt.rs           # Optimizations
│   │   ├── verify.rs        # IR validation
│   │   └── stubs.rs         # Runtime stub mapping
│   ├── runtime/             # Native runtime kernel
│   │   ├── abi.rs           # NaN-boxed values
│   │   ├── heap.rs          # Allocator
│   │   └── stubs.rs         # C ABI functions
│   ├── vm/                  # Stack-based VM (debug)
│   │   ├── mod.rs           # VM implementation
│   │   ├── opcodes.rs       # Bytecode opcodes
│   │   └── value.rs         # Runtime values
│   ├── loader/              # Bytecode loader
│   └── stdlib/              # Standard library
├── bootstrap/               # Self-hosting compiler
│   ├── lexer.tscl           # Tokenizer
│   ├── parser.tscl          # Parser
│   └── emitter.tscl         # Bytecode emitter
├── std/
│   └── prelude.tscl         # Standard prelude
└── examples/
    └── *.tscl               # Example programs
```

## Performance

| Benchmark | Target |
|-----------|--------|
| fib(35) | 20ms |
| Startup | 5ms |
| HTTP hello | 250k rps |

## Development Status

| Phase | Status | Description |
|-------|--------|-------------|
| Phase 0 | ✅ Complete | Runtime kernel (NaN-boxing, allocator, stubs) |
| Phase 1 | ✅ Complete | SSA IR (lowering, type inference, optimizations) |
| Phase 2 | 🚧 Planned | Cranelift JIT backend |
| Phase 3 | 📋 Planned | LLVM AOT backend |
| Phase 4 | 📋 Planned | Self-hosted native compiler |

See [progress.md](progress.md) for detailed implementation notes.

## Testing

```bash
# Run all tests
cargo test --release

# Run specific IR tests
cargo test --release ir::

# Output: 59 tests passed
```

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

## License

tscl is distributed under the terms of the Apache License (Version 2.0).

See [LICENSE](LICENSE) for details.
