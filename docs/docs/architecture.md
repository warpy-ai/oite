---
sidebar_position: 4
---

# Architecture

Script's architecture is designed for native-first execution with multiple backend options.

## High-Level Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                         Script Source                             │
└───────────────────────────┬─────────────────────────────────────┘
                            ▼
┌─────────────────────────────────────────────────────────────────┐
│                     Script Compiler                               │
│  ┌─────────────┐  ┌──────────────┐  ┌────────────────────────┐  │
│  │   Parser    │─▶│ Borrow Check │─▶│   SSA IR Generation    │  │
│  │  (SWC AST)  │  │  (Ownership) │  │ (Type Inference, Opts) │  │
│  └─────────────┘  └──────────────┘  └────────────────────────┘  │
└───────────────────────────┬─────────────────────────────────────┘
                            ▼
┌─────────────────────────────────────────────────────────────────┐
│                    Native Backend                                │
│  ┌─────────────────┐  ┌─────────────────┐  ┌─────────────────┐  │
│  │  Cranelift JIT  │  │   LLVM AOT      │  │   VM (Debug)    │  │
│  │   (Fast)        │  │  (LTO, Native)  │  │  (Interpreter)  │  │
│  └─────────────────┘  └─────────────────┘  └─────────────────┘  │
└───────────────────────────┬─────────────────────────────────────┘
                            ▼
                          CPU
```

## Compilation Pipeline

### 1. Parsing

Script uses SWC (Speedy Web Compiler) for parsing, producing an AST that's compatible with JavaScript/TypeScript syntax.

### 2. Borrow Checking

Rust-inspired ownership model ensures memory safety at compile time:
- Each value has exactly one owner
- Objects are moved, primitives are copied
- No use-after-move errors

### 3. SSA IR Generation

The compiler transforms the AST into Static Single Assignment (SSA) intermediate representation:

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

### 4. Type Inference & Specialization

Flow-sensitive type analysis specializes dynamic operations:

| Before | After | Speedup |
|--------|-------|---------|
| `add.any v0, v1` | `add.num v0, v1` | ~10x |
| `mul.any v0, v1` | `mul.num v0, v1` | ~10x |

### 5. Optimizations

- Dead Code Elimination (DCE)
- Constant folding
- Common Subexpression Elimination (CSE)
- Copy propagation
- Branch simplification
- Unreachable block elimination

## Backends

### Cranelift JIT

Fast development and benchmarking:
- Runtime compilation to native code
- Multi-function support with recursion
- Tiered compilation based on hotness

### LLVM AOT

Optimized production binaries:
- Link-Time Optimization (LTO)
- ThinLTO for `--release`
- Full LTO for `--dist`
- Standalone executables

### VM (Debug)

Stack-based interpreter for:
- Debugging and testing
- Bootstrapping
- Compatibility fallback

## Runtime Kernel

The runtime provides a stable ABI that all backends use:

- **NaN-boxed values**: 64-bit uniform representation
- **Heap allocator**: Bump allocator for objects/arrays
- **Runtime stubs**: 20+ C ABI functions for operations

Key stubs include:
- Allocation: `tscl_alloc_object`, `tscl_alloc_array`, `tscl_alloc_string`
- Property access: `tscl_get_prop`, `tscl_set_prop`
- Arithmetic: `tscl_add_any`, `tscl_sub_any`, `tscl_mul_any`
- I/O: `tscl_console_log`, `tscl_call`
