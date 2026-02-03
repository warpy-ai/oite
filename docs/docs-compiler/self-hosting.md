---
sidebar_position: 8
title: Self-Hosting
description: Oite's self-hosting compiler is complete. The Oite compiler is written entirely in Oite itself and generates native binaries via LLVM IR.
keywords: [self-hosting, bootstrap, compiler, oitec, self-compiling, llvm]
---

# Oite Self-Hosting Compiler

The Oite compiler (`oitec`) is now **fully self-hosting** — written entirely in Oite and capable of compiling itself to native binaries.

---

## Current State: Self-Hosting Complete

```
┌─────────────────────────────────────────────────────────────┐
│                    Source Code (.ot)                      │
└───────────────────────────┬─────────────────────────────────┘
                            │
            ┌───────────────┼───────────────┐
            ▼               ▼               ▼
┌───────────────┐  ┌───────────────┐  ┌───────────────┐
│ src/compiler/ │  │  bootstrap/   │  │  compiler/    │
│    (Rust)     │  │  (reference)  │  │  (COMPLETE)   │
│               │  │               │  │               │
│ SWC Parser    │  │ Custom Lexer  │  │ Full Pipeline │
│ → Bytecode    │  │ Custom Parser │  │ → Bytecode    │
│ → IR → Native │  │ → Bytecode    │  │ → LLVM IR     │
└───────┬───────┘  └───────┬───────┘  └───────┬───────┘
        │                  │                  │
        ▼                  ▼                  ▼
   ┌─────────┐        ┌─────────┐        ┌─────────┐
   │ Native  │        │   VM    │        │ Native  │
   │ Binary  │        │ (Rust)  │        │ Binary  │
   └─────────┘        └─────────┘        └─────────┘
```

### All Components Working

| Component          | Location                   | Status                                               |
| ------------------ | -------------------------- | ---------------------------------------------------- |
| Rust Compiler      | `src/compiler/`            | Production: parse → bytecode → IR → native           |
| Bootstrap Compiler | `bootstrap/*.ot`           | Reference: parse → bytecode                          |
| Modular Compiler   | `compiler/*.ot`            | **Complete**: parse → IR → bytecode/LLVM IR → native |
| VM                 | `src/vm/`                  | Full bytecode execution                              |
| JIT                | `src/backend/cranelift.rs` | Cranelift codegen                                    |
| AOT                | `src/backend/llvm/`        | LLVM with LTO                                        |

---

## Foundation Complete

**Goal**: Consolidate and stabilize the .ot compiler infrastructure.

### Architecture

```
Source (.ot) ──► bootstrap/*.ot ──► Bytecode ──► Rust VM
                        │
                        └──► (reference implementation)

Source (.ot) ──► src/compiler/ (Rust) ──► Native Binary
                        │
                        └──► (production builds)
```

### Tasks (All Complete)

- [x] Working lexer in `bootstrap/lexer.ot`
- [x] Working parser in `bootstrap/parser.ot`
- [x] IR generation in `bootstrap/ir.ot`, `bootstrap/ir_builder.ot`
- [x] Bytecode emission in `bootstrap/codegen.ot`, `bootstrap/emitter.ot`
- [x] Port `bootstrap/` features to `compiler/` (modular structure)
- [x] Ensure `compiler/` can parse the same syntax as `bootstrap/`
- [x] Add all expression/statement handling in `compiler/parser/`

### File Structure

```
bootstrap/                    # Reference implementation (~5,400 lines)
├── main.ot                # CLI (273 lines)
├── types.ot               # Type definitions (357 lines)
├── lexer.ot               # Tokenization (335 lines)
├── parser.ot              # AST generation (1,432 lines)
├── ir.ot                  # IR types (619 lines)
├── ir_builder.ot          # AST → IR (270 lines)
├── codegen.ot             # IR → Bytecode (315 lines)
├── emitter.ot             # Binary serialization (846 lines)
├── pipeline.ot            # Orchestration (228 lines)
├── stdlib.ot              # Runtime decls (248 lines)
└── utils.ot               # Helpers (22 lines)

compiler/                     # Production compiler (~10,500 lines)
├── main.ot                # CLI entry (344 lines)
├── lexer/
│   ├── mod.ot             # Tokenization
│   ├── token.ot           # Token types
│   └── error.ot           # Lexer errors
├── parser/
│   ├── mod.ot             # Parser orchestration
│   ├── expr.ot            # Expressions
│   ├── stmt.ot            # Statements
│   └── error.ot           # Parse errors
├── ast/
│   ├── mod.ot             # AST definitions
│   └── types.ot           # Type annotations
├── ir/
│   ├── mod.ot             # IR types
│   └── builder.ot         # AST → IR (1,500+ lines)
├── codegen/
│   ├── mod.ot             # Codegen orchestration
│   └── emitter.ot         # IR → Bytecode
├── passes/
│   ├── mod.ot             # Pass orchestration
│   ├── typecheck.ot       # Type checking
│   ├── opt.ot             # Optimization passes
│   └── borrow_ck.ot       # Borrow checking
├── backend/
│   └── llvm/
│       ├── mod.ot         # LLVM IR emitter (1,348 lines)
│       ├── runtime.ot     # Runtime stubs
│       └── types.ot       # Type mappings
├── stdlib/
│   └── builtins.ot        # Built-in declarations
└── pipeline.ot            # Compilation orchestration
```

---

## Feature Parity Complete

**Goal**: Make `compiler/*.ot` feature-complete with `bootstrap/` and add optimization passes.

### Architecture

```
Source (.ot) ──► compiler/*.ot ──► Bytecode ──► Rust VM
                        │
                        ├──► IR Verification
                        ├──► Type Inference  [done]
                        └──► Optimizations   [done]
```

### Tasks (All Complete)

- [x] Complete parser in `compiler/parser/` to handle all syntax
- [x] Add type inference pass (`compiler/passes/typecheck.ot`)
- [x] Add optimization passes (`compiler/passes/opt.ot`)
  - [x] Dead code elimination
  - [x] Constant folding
  - [x] Copy propagation
- [x] Add borrow checker (`compiler/passes/borrow_ck.ot`)
- [x] Improve IR verification
- [x] Match bytecode output with `bootstrap/`

### CLI Commands (All Working)

```bash
oite ast <file>               # Output JSON AST
oite ir <file>                # Output SSA IR
oite check <file>             # Type + borrow check
oite build <file>             # Compile to bytecode
oite run <file>               # Generate bytecode for VM
oite llvm <file>              # Generate LLVM IR (.ll)
```

---

## Native Code Generation Complete

**Goal**: Add native code generation to `compiler/*.ot`, making it a full `oitec`.

### Architecture (Implemented)

```
Source (.ot) ──► compiler/*.ot (oitec) ──┬──► Bytecode ──► VM
                                               │
                                               └──► LLVM IR ──► clang ──► Native
```

### Chosen Approach: LLVM IR Text (Option C)

We implemented Option C — generating LLVM IR text and using `clang` to compile. This provides:

- Full LLVM optimizations
- Cross-platform support (x86-64, ARM64, etc.)
- Faster implementation than direct assembly
- Production-quality native binaries

```
compiler/
├── backend/
│   └── llvm/
│       ├── mod.ot         # LLVM IR emitter (1,348 lines)
│       ├── runtime.ot     # Inlined runtime functions
│       └── types.ot       # Type mappings
```

### Tasks (All Complete)

- [x] Design backend interface (`compiler/backend/llvm/mod.ot`)
- [x] Implement NaN-boxing for all value types
- [x] Implement all IR operations → LLVM IR
- [x] Implement string constants and escaping
- [x] Implement object/array allocation
- [x] Implement function calls and recursion
- [x] Inline runtime (no external library needed)
- [x] Self-compile test: all modules compile themselves

### Build Pipeline

```bash
# Generate LLVM IR
./target/release/oitec compiler/main.ot llvm input.ot

# Compile to native
clang input.ot.ll -c -o input.o
clang input.o -o output

# Run native binary
./output
```

### Performance Results

| Test          | Native Output | VM Output | Speedup     |
| ------------- | ------------- | --------- | ----------- |
| Objects       | Match         | Match     | ~4x faster  |
| Functions     | Match         | Match     | ~4x faster  |
| Recursion     | Match         | Match     | ~30x faster |
| Loops         | Match         | Match     | ~30x faster |
| Fibonacci(25) | 75025         | 75025     | ~30x faster |

---

## Bootstrap Verification (In Progress)

**Goal**: Prove deterministic self-hosting through the bootstrap chain.

### Bootstrap Chain

```
┌─────────────────────────────────────────────────────────────┐
│  oite₀ = src/compiler/ (Rust)                               │
│         Original compiler written in Rust                   │
└───────────────────────────┬─────────────────────────────────┘
                            │ compiles
                            ▼
┌─────────────────────────────────────────────────────────────┐
│  oite₁ = compiler/*.ot compiled by oite₀                  │
│         First native oitec binary                         │
└───────────────────────────┬─────────────────────────────────┘
                            │ compiles
                            ▼
┌─────────────────────────────────────────────────────────────┐
│  oite₂ = compiler/*.ot compiled by oite₁                  │
│         Self-compiled oitec binary                        │
└───────────────────────────┬─────────────────────────────────┘
                            │
                            ▼
┌─────────────────────────────────────────────────────────────┐
│  VERIFY: hash(oite₁) == hash(oite₂)                         │
│          Proves deterministic compilation                   │
└─────────────────────────────────────────────────────────────┘
```

### Verification Oite

```bash
#!/bin/bash
# bootstrap_verify.sh

# Stage 1: Build oitec with Rust compiler
./target/release/oitec build compiler/main.ot -o oitec1 --dist

# Stage 2: Build oitec with oitec1
./oitec1 build compiler/main.ot -o oitec2 --dist

# Stage 3: Build oitec with oitec2
./oitec2 build compiler/main.ot -o oitec3 --dist

# Verify: oitec2 == oitec3 (bit-for-bit)
if cmp -s oitec2 oitec3; then
    echo "Bootstrap verification PASSED"
    echo "   oitec2 and oitec3 are identical"
    sha256sum oitec2 oitec3
else
    echo "Bootstrap verification FAILED"
    echo "   oitec2 and oitec3 differ"
    exit 1
fi
```

### Tasks

- [ ] Ensure deterministic IR serialization
- [ ] Ensure deterministic bytecode generation
- [ ] Ensure deterministic native code generation
- [ ] Create bootstrap verification script
- [ ] CI integration for bootstrap verification

---

## Progress Summary

| Milestone              | Status      | Lines of Code         |
| ---------------------- | ----------- | --------------------- |
| Foundation             | Complete    | ~5,400 (bootstrap)    |
| Feature parity         | Complete    | ~10,500 (compiler)    |
| Native codegen         | Complete    | +1,348 (LLVM backend) |
| Bootstrap verification | In Progress | ~500 lines + testing  |

**Total self-hosted code**: ~10,500 lines of .ot

---

## Success Criteria

### Foundation (Complete)

- [x] `bootstrap/` compiles .ot to bytecode
- [x] `compiler/` can parse same syntax as `bootstrap/`

### Feature Parity (Complete)

- [x] `compiler/` produces working bytecode
- [x] Type inference implemented
- [x] Basic optimizations working

### Native Codegen (Complete)

- [x] `compiler/` can produce LLVM IR
- [x] LLVM IR compiles to native binaries via clang
- [x] All compiler modules self-compile

### Bootstrap Verification (In Progress)

- [x] `oitec` can compile itself (via LLVM IR)
- [ ] `hash(oite₁) == hash(oite₂)` verified
- [ ] CI integration for bootstrap verification

---

## Migration Strategy

Once bootstrap verification is complete:

1. **Keep Rust compiler** as reference/testing tool
2. **Primary compiler** becomes `oitec` (compiler/\*.ot)
3. **Remove `bootstrap/`** (superseded by `compiler/`)
4. **Optional**: Remove `src/compiler/` entirely

### Final Structure

```
script/
├── compiler/                 # THE compiler (oitec)
│   ├── main.ot
│   ├── lexer/
│   ├── parser/
│   ├── ast/
│   ├── ir/
│   ├── passes/
│   ├── backend/
│   └── stdlib/
├── src/
│   ├── runtime/             # Keep: ABI, heap, stubs
│   │   ├── abi.rs
│   │   ├── heap.rs
│   │   └── stubs.rs
│   └── vm/                  # Keep: for debugging/testing
└── tests/
```

The Rust code becomes minimal runtime support, with the compiler fully in Oite.
