---
sidebar_position: 2
title: Getting Started
description: Learn how to install and set up Oite programming language. Build your first Oite program with Cranelift JIT or LLVM AOT compilation.
keywords:
  [
    oite installation,
    oite setup,
    getting started,
    llvm,
    cranelift,
    jit compilation,
  ]
---

# Getting Started

This guide will help you install Oite and build your first program.

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

### Step 2: Clone and Build Oite

```bash
# Clone the repository
git clone https://github.com/warpy-ai/oite.git
cd oite

# Build in release mode
cargo build --release

# Verify installation
./target/release/oite --help
```

### Step 3: Add to PATH (Optional)

```bash
# Add Oite to your PATH
echo 'export PATH="$PATH:/path/to/oite/target/release"' >> ~/.zshrc
source ~/.zshrc

# Now you can run from anywhere
oite --help
```

## Your First Oite Program

### Hello World

Create a file called `hello.ot`:

```javascript
console.log("Hello, Oite!");
```

Run it:

```bash
./target/release/oite hello.ot
```

### A More Complete Example

Create `fibonacci.ot`:

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
./target/release/oite fibonacci.ot

# JIT (fast compilation, good for development)
./target/release/oite jit fibonacci.ot

# AOT (native binary, best performance)
./target/release/oite build fibonacci.ot --release -o fib
./fib
```

## Execution Modes

Oite provides multiple ways to run your code:

| Mode | Command | Use Case | Performance |
|------|---------|----------|-------------|
| **VM** | `oite app.ot` | Debugging, REPL | Slowest |
| **JIT** | `oite jit app.ot` | Development, testing | Fast |
| **AOT Release** | `oite build app.ot --release -o app` | Production (ThinLTO) | Faster |
| **AOT Dist** | `oite build app.ot --dist -o app` | Maximum optimization (Full LTO) | Fastest |

### JIT Compilation (Development)

```bash
./target/release/oite jit app.ot
```

Uses Cranelift for fast compilation. Perfect for rapid iteration.

### AOT Compilation (Production)

```bash
# Release build with ThinLTO
./target/release/oite build app.ot --release -o app

# Distribution build with Full LTO (slower compile, faster runtime)
./target/release/oite build app.ot --dist -o app

# Run the native binary
./app
```

Produces standalone executables with no runtime dependencies.

### VM Execution (Debugging)

```bash
./target/release/oite app.ot
```

Interpreted execution for debugging and testing.

## CLI Reference

```bash
# Run with VM
oite <file.ot>

# Run with JIT
oite jit <file.ot>

# Build native binary
oite build <file.ot> [--release|--dist] -o <output>

# Show SSA IR (debugging)
oite ir <file.ot>

# Show AST (debugging)
oite ast <file.ot>

# Type check only
oite check <file.ot>
```

## Project Structure

A typical Oite project looks like:

```
my-project/
├── main.ot             # Entry point
├── lib/
│   ├── utils.ot        # Utility functions
│   └── types.ot        # Type definitions
└── tests/
    └── test_utils.ot   # Tests
```

### Importing Modules

```javascript
// main.ot
import { helper } from "./lib/utils";

let result = helper(42);
console.log(result);
```

```javascript
// lib/utils.ot
export function helper(x: number): number {
  return x * 2;
}
```

## What's Next?

Now that you have Oite running, explore:

- [Language Features](/docs/language-features) — Variables, functions, classes, and more
- [Architecture](/docs/architecture) — How Oite works under the hood
- [Standard Library](/docs/standard-library) — Built-in functions and modules
- [Memory Model](/docs/memory-model) — Ownership and borrow checking
- [Development Status](/docs/development-status) — Current state and roadmap
