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

## Supported Platforms

Oite builds on **macOS** and **Linux**. These are the platforms covered by CI.

Native Windows (`x86_64-pc-windows-msvc`) is **not supported yet** — build Oite under
[WSL2](#windows-wsl2) instead.

:::info LLVM 18 is required to build Oite at all
Oite depends on `llvm-sys`, which is a mandatory dependency — not an optional one. Even
though the Cranelift JIT backend does not *use* LLVM at runtime, LLVM 18 and its development
libraries must be present to **compile** Oite. `cargo build` will fail without them.
:::

## Installation

### Step 1: Install Prerequisites

**macOS:**

```bash
# Install Rust (if not already installed)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Install LLVM 18 and zstd (required for linking)
brew install llvm@18 zstd

# Set LLVM environment variable (add to ~/.zshrc or ~/.bashrc)
echo 'export LLVM_SYS_180_PREFIX=$(brew --prefix llvm@18)' >> ~/.zshrc
source ~/.zshrc
```

**Linux (Ubuntu 24.04 / Debian 13+):**

Ubuntu 24.04 (noble) and Debian 13 (trixie) ship LLVM 18 in their own repositories, so the
development packages can be installed directly. This is the path CI uses.

```bash
# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Install LLVM 18 development headers, Polly, and zstd
sudo apt update
sudo apt install -y llvm-18-dev libpolly-18-dev libzstd-dev

# Set LLVM path
echo 'export LLVM_SYS_180_PREFIX=/usr/lib/llvm-18' >> ~/.bashrc
source ~/.bashrc
```

:::caution Older distributions
If your distribution has no `llvm-18-dev` package (for example Debian 12 "bookworm", which
ships LLVM 14), use the official packages from [apt.llvm.org](https://apt.llvm.org/) to
install LLVM 18.

Prefer installing the `llvm-18-dev` package from that repository over running the
`llvm.sh` convenience script. On distributions that already ship LLVM 18, `llvm.sh` upgrades
`libllvm18` to a snapshot build that conflicts with the distro's `llvm-18-dev`, producing
unmet-dependency errors.
:::

### Windows (WSL2)

Oite cannot currently be built natively on Windows, for two independent reasons:

1. **The official LLVM installer is unusable with `llvm-sys`.** `llvm-sys` needs
   `llvm-config` to discover which libraries and compiler flags to link. As the
   [llvm-sys documentation](https://crates.io/crates/llvm-sys) states, binary distributions
   of LLVM — including the official release packages and the `winget` LLVM package — generally
   **do not ship `llvm-config`**, along with the static libraries and headers it reports. This
   is why installing LLVM from the official Windows installer and adding
   `C:\Program Files\LLVM\bin` to `PATH` does not work: the files `llvm-sys` needs were never
   installed, so no amount of `PATH` or `LLVM_SYS_180_PREFIX` configuration will find them.
   The official installers also track the latest LLVM release, whereas Oite requires 18.x.
2. **Oite's runtime is currently Unix-only.** The async reactor
   (`src/runtime/async/reactor.rs`) imports `std::os::unix::io::RawFd` unconditionally, which
   does not exist on Windows targets. So even with a hand-built LLVM 18, the build would still
   fail to compile.

**Use WSL2**, which runs a real Linux toolchain and needs no LLVM build from source:

```powershell
# In PowerShell (as Administrator), install WSL2 with Ubuntu 24.04
wsl --install -d Ubuntu-24.04
```

Then open the Ubuntu shell and follow the **Linux (Ubuntu 24.04 / Debian 13+)** instructions
above verbatim. Everything from Step 2 onward works unchanged.

:::tip
Keep the repository inside the Linux filesystem (e.g. `~/oite`) rather than under `/mnt/c/`.
Building on the Windows-mounted drive is dramatically slower.
:::

### Step 2: Clone and Build Oite

```bash
# Clone the repository
git clone https://github.com/warpy-ai/oite.git
cd oite

# Build in release mode
cargo build --release

# Verify installation
./target/release/oitec --help
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
./target/release/oitec hello.ot
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
./target/release/oitec fibonacci.ot

# JIT (fast compilation, good for development)
./target/release/oitec jit fibonacci.ot

# AOT (native binary, best performance)
./target/release/oitec build fibonacci.ot --release -o fib
./fib
```

## Execution Modes

Oite provides multiple ways to run your code:

| Mode            | Command                              | Use Case                        | Performance |
| --------------- | ------------------------------------ | ------------------------------- | ----------- |
| **VM**          | `oite app.ot`                        | Debugging, REPL                 | Slowest     |
| **JIT**         | `oite jit app.ot`                    | Development, testing            | Fast        |
| **AOT Release** | `oite build app.ot --release -o app` | Production (ThinLTO)            | Faster      |
| **AOT Dist**    | `oite build app.ot --dist -o app`    | Maximum optimization (Full LTO) | Fastest     |

### JIT Compilation (Development)

```bash
./target/release/oitec jit app.ot
```

Uses Cranelift for fast compilation. Perfect for rapid iteration.

### AOT Compilation (Production)

```bash
# Release build with ThinLTO
./target/release/oitec build app.ot --release -o app

# Distribution build with Full LTO (slower compile, faster runtime)
./target/release/oitec build app.ot --dist -o app

# Run the native binary
./app
```

Produces standalone executables with no runtime dependencies.

### VM Execution (Debugging)

```bash
./target/release/oitec app.ot
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

- [Language Features](/compiler/language-features) — Variables, functions, classes, and more
- [Architecture](/compiler/architecture) — How Oite works under the hood
- [Standard Library](/compiler/standard-library) — Built-in functions and modules
- [Memory Model](/rolls/memory-model) — Ownership and borrow checking
- [Development Status](/development-status) — Current state and roadmap
