---
sidebar_position: 1
title: Oite Compiler
description: Documentation for the Oite compiler including language syntax, type system, SSA IR, and native code generation.
keywords:
  [oite compiler, language, syntax, type system, ssa ir, code generation]
---

# Oite Compiler

The **Oite Compiler** is responsible for transforming Oite source code into native executables. This section covers the language definition layer - everything from syntax and semantics to the type system and compilation pipeline.

## What's in This Section

- **[Language Features](./language-features)** - Syntax, control flow, classes, and language constructs
- **[Type System](./type-system)** - Structural typing, type inference, and generics
- **[SSA IR](./ssa-ir)** - The intermediate representation used for optimization
- **[Standard Library](./standard-library)** - Core language APIs and primitives
- **[Architecture](./architecture)** - Compiler pipeline and ecosystem overview
- **[ABI Specification](./abi)** - Binary interface and runtime contracts
- **[Self-Hosting](./self-hosting)** - The self-hosted compiler implementation

## Overview

The Oite compiler features:

- **TypeScript-like Syntax** - Familiar JavaScript/TypeScript syntax with type annotations
- **Hindley-Milner Type Inference** - Automatic type deduction with optional annotations
- **SSA-based IR** - Static Single Assignment form for powerful optimizations
- **Multiple Backends** - Cranelift JIT and LLVM AOT compilation
- **Self-Hosting** - The compiler is written in Oite itself

## Quick Example

```javascript
// Oite source code
function fib(n: number): number {
    if (n < 2) return n;
    return fib(n - 1) + fib(n - 2);
}

console.log(fib(35)); // Compiled to native code!
```

## Compilation Pipeline

```
Source (.ot) --> Lexer --> Parser --> Type Check --> SSA IR --> Native Code
                                                        |
                                                        +--> Optimizations
                                                             - Dead code elimination
                                                             - Constant folding
                                                             - Copy propagation
```

## Getting Started

To compile an Oite program:

```bash
# Build the compiler
cargo build --release

# Compile to native binary
./target/release/oitec build myprogram.ot --release -o myprogram

# Run the compiled binary
./myprogram
```
