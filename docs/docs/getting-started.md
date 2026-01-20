---
sidebar_position: 2
---

# Getting Started

This guide will help you get Script up and running on your system.

## Prerequisites

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

## Building

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

## Execution Modes

Script supports multiple execution backends:

### Cranelift JIT (Fast Development)

```bash
./target/release/script jit <file.tscl>
```

Fast compilation and execution, perfect for development and benchmarking.

### LLVM AOT (Optimized Production)

```bash
# Dev build (no LTO)
./target/release/script build app.tscl --release -o app

# Dist build (full LTO)
./target/release/script build app.tscl --dist -o app
```

Produces standalone native binaries with link-time optimization for maximum performance.

### VM (Debug Mode)

```bash
./target/release/script --run-binary output.tscl.bc
```

Stack-based VM for debugging and testing.

## Your First Script

Create a file `hello.tscl`:

```javascript
console.log("Hello, Script!");
```

Run it:

```bash
./target/release/script hello.tscl
```

## Next Steps

- Learn about [Language Features](/docs/language-features)
- Explore the [Architecture](/docs/architecture)
- Review the [Standard Library](/docs/standard-library)
