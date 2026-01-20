---
sidebar_position: 1
---

# Welcome to Script

**Script** is a high-performance JavaScript-like scripting language with native code execution, featuring a self-hosting compiler and Rust-inspired memory safety.

```javascript
function fib(n) {
    if (n < 2) return n;
    return fib(n - 1) + fib(n - 2);
}

console.log(fib(35));  // Compiled to native code!
```

## Key Features

- **Native Execution** — SSA-based IR compiled to native code via Cranelift/LLVM
- **Link-Time Optimization** — ThinLTO and Full LTO for maximum performance
- **Standalone Binaries** — Self-contained executables with runtime stubs in LLVM IR
- **Memory Safety** — Ownership model with compile-time borrow checking
- **Self-Hosting** — Bootstrap compiler written in Script itself
- **Type Inference** — Flow-sensitive type analysis for optimization
- **JavaScript Syntax** — Familiar syntax with ES6+ features
- **Classes & Inheritance** — ES6 classes with extends, super(), and private fields
- **Error Handling** — try/catch/finally with exception propagation

## Quick Start

```bash
# Build
cargo build --release

# Run a script
./target/release/script myprogram.tscl

# Build to native binary (requires LLVM)
./target/release/script build myprogram.tscl --release -o myprogram

# Run the compiled binary
./myprogram
```

## What's Next?

- Read the [Getting Started](/docs/getting-started) guide
- Explore [Language Features](/docs/language-features)
- Learn about the [Architecture](/docs/architecture)
- Check the [Development Status](/docs/development-status)
