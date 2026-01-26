---
sidebar_position: 2
title: Getting Started
description: Learn how to install and set up Script programming language. Build your first Script program with Cranelift JIT or LLVM AOT compilation.
keywords:
  [
    script installation,
    script setup,
    getting started,
    llvm,
    cranelift,
    jit compilation,
  ]
---

# Getting Started

This guide will help you install Script and build your first program.

## Installation

### Step 1: Install Prerequisites

**macOS:**

```bash
# Install Rust (if not already installed)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Install LLVM 18 (required for AOT compilation)
brew install llvm@18

# Install zstd (required for linking)
brew install zstd

# Set LLVM environment variable (add to ~/.zshrc or ~/.bashrc)
echo 'export LLVM_SYS_180_PREFIX=$(brew --prefix llvm@18)' >> ~/.zshrc
source ~/.zshrc
```

**Linux (Ubuntu/Debian):**

```bash
# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Install LLVM 18
wget https://apt.llvm.org/llvm.sh
chmod +x llvm.sh
sudo ./llvm.sh 18

# Install zstd
sudo apt install libzstd-dev

# Set LLVM path
echo 'export LLVM_SYS_180_PREFIX=/usr/lib/llvm-18' >> ~/.bashrc
source ~/.bashrc
```

:::note
The Cranelift JIT backend works without LLVM. LLVM is only required for AOT (ahead-of-time) compilation to native binaries.
:::

### Step 2: Clone and Build Script

```bash
# Clone the repository
git clone https://github.com/aspect/script.git
cd script

# Build in release mode
cargo build --release

# Verify installation
./target/release/script --help
```

### Step 3: Add to PATH (Optional)

```bash
# Add Script to your PATH
echo 'export PATH="$PATH:/path/to/script/target/release"' >> ~/.zshrc
source ~/.zshrc

# Now you can run from anywhere
script --help
```

## Your First Script Program

### Hello World

Create a file called `hello.tscl`:

```javascript
console.log("Hello, Script!");
```

Run it:

```bash
./target/release/script run hello.tscl
```

### A More Complete Example

Create `fibonacci.tscl`:

```javascript
function fib(n: number): number {
  if (n < 2) return n;
  return fib(n - 1) + fib(n - 2);
}

let result = fib(25);
console.log("Fibonacci(25) =", result);
```

Run with different backends:

```bash
# VM (interpreted, good for debugging)
./target/release/script run fibonacci.tscl

# JIT (fast compilation, good for development)
./target/release/script jit fibonacci.tscl

# AOT (native binary, best performance)
./target/release/script build fibonacci.tscl --release -o fib
./fib
```

## Execution Modes

Script provides multiple ways to run your code:

| Mode | Command | Use Case | Performance |
|------|---------|----------|-------------|
| **VM** | `script run app.tscl` | Debugging, REPL | Slowest |
| **JIT** | `script jit app.tscl` | Development, testing | Fast |
| **AOT Release** | `script build app.tscl --release -o app` | Production (ThinLTO) | Faster |
| **AOT Dist** | `script build app.tscl --dist -o app` | Maximum optimization (Full LTO) | Fastest |

### JIT Compilation (Development)

```bash
./target/release/script jit app.tscl
```

Uses Cranelift for fast compilation. Perfect for rapid iteration.

### AOT Compilation (Production)

```bash
# Release build with ThinLTO
./target/release/script build app.tscl --release -o app

# Distribution build with Full LTO (slower compile, faster runtime)
./target/release/script build app.tscl --dist -o app

# Run the native binary
./app
```

Produces standalone executables with no runtime dependencies.

### VM Execution (Debugging)

```bash
./target/release/script run app.tscl
```

Interpreted execution for debugging and testing.

## CLI Reference

```bash
# Run with VM
script run <file.tscl>

# Run with JIT
script jit <file.tscl>

# Build native binary
script build <file.tscl> [--release|--dist] -o <output>

# Show SSA IR (debugging)
script ir <file.tscl>

# Show AST (debugging)
script ast <file.tscl>

# Type check only
script check <file.tscl>
```

## Project Structure

A typical Script project looks like:

```
my-project/
├── main.tscl           # Entry point
├── lib/
│   ├── utils.tscl      # Utility functions
│   └── types.tscl      # Type definitions
└── tests/
    └── test_utils.tscl # Tests
```

### Importing Modules

```javascript
// main.tscl
import { helper } from "./lib/utils";

let result = helper(42);
console.log(result);
```

```javascript
// lib/utils.tscl
export function helper(x: number): number {
  return x * 2;
}
```

## What's Next?

Now that you have Script running, explore:

- [Language Features](/docs/language-features) — Variables, functions, classes, and more
- [Architecture](/docs/architecture) — How Script works under the hood
- [Standard Library](/docs/standard-library) — Built-in functions and modules
- [Memory Model](/docs/memory-model) — Ownership and borrow checking
- [Development Status](/docs/development-status) — Current state and roadmap
