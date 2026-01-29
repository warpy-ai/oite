---
sidebar_position: 9
title: Self-Hosting Compiler
description: Script's self-hosting compiler is complete. The Script compiler is written entirely in Script itself and generates native binaries via LLVM IR.
keywords: [self-hosting, bootstrap, compiler, scriptc, self-compiling, llvm]
---

# Script Self-Hosting Compiler

The Script compiler (`scriptc`) is now **fully self-hosting** — written entirely in Script and capable of compiling itself to native binaries.

---

## Current State: Self-Hosting Complete

```
┌─────────────────────────────────────────────────────────────┐
│                    Source Code (.tscl)                      │
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

| Component          | Location                   | Status                                                  |
| ------------------ | -------------------------- | ------------------------------------------------------- |
| Rust Compiler      | `src/compiler/`            | Production: parse → bytecode → IR → native           |
| Bootstrap Compiler | `bootstrap/*.tscl`         | Reference: parse → bytecode                          |
| Modular Compiler   | `compiler/*.tscl`          | **Complete**: parse → IR → bytecode/LLVM IR → native |
| VM                 | `src/vm/`                  | Full bytecode execution                              |
| JIT                | `src/backend/cranelift.rs` | Cranelift codegen                                    |
| AOT                | `src/backend/llvm/`        | LLVM with LTO                                        |

---

## Foundation Complete

**Goal**: Consolidate and stabilize the .tscl compiler infrastructure.

### Architecture

```
Source (.tscl) ──► bootstrap/*.tscl ──► Bytecode ──► Rust VM
                        │
                        └──► (reference implementation)

Source (.tscl) ──► src/compiler/ (Rust) ──► Native Binary
                        │
                        └──► (production builds)
```

### Tasks (All Complete)

- [x] Working lexer in `bootstrap/lexer.tscl`
- [x] Working parser in `bootstrap/parser.tscl`
- [x] IR generation in `bootstrap/ir.tscl`, `bootstrap/ir_builder.tscl`
- [x] Bytecode emission in `bootstrap/codegen.tscl`, `bootstrap/emitter.tscl`
- [x] Port `bootstrap/` features to `compiler/` (modular structure)
- [x] Ensure `compiler/` can parse the same syntax as `bootstrap/`
- [x] Add all expression/statement handling in `compiler/parser/`

### File Structure

```
bootstrap/                    # Reference implementation (~5,400 lines)
├── main.tscl                # CLI (273 lines)
├── types.tscl               # Type definitions (357 lines)
├── lexer.tscl               # Tokenization (335 lines)
├── parser.tscl              # AST generation (1,432 lines)
├── ir.tscl                  # IR types (619 lines)
├── ir_builder.tscl          # AST → IR (270 lines)
├── codegen.tscl             # IR → Bytecode (315 lines)
├── emitter.tscl             # Binary serialization (846 lines)
├── pipeline.tscl            # Orchestration (228 lines)
├── stdlib.tscl              # Runtime decls (248 lines)
└── utils.tscl               # Helpers (22 lines)

compiler/                     # Production compiler (~10,500 lines)
├── main.tscl                # CLI entry (344 lines)
├── lexer/
│   ├── mod.tscl             # Tokenization
│   ├── token.tscl           # Token types
│   └── error.tscl           # Lexer errors
├── parser/
│   ├── mod.tscl             # Parser orchestration
│   ├── expr.tscl            # Expressions
│   ├── stmt.tscl            # Statements
│   └── error.tscl           # Parse errors
├── ast/
│   ├── mod.tscl             # AST definitions
│   └── types.tscl           # Type annotations
├── ir/
│   ├── mod.tscl             # IR types
│   └── builder.tscl         # AST → IR (1,500+ lines)
├── codegen/
│   ├── mod.tscl             # Codegen orchestration
│   └── emitter.tscl         # IR → Bytecode
├── passes/
│   ├── mod.tscl             # Pass orchestration
│   ├── typecheck.tscl       # Type checking
│   ├── opt.tscl             # Optimization passes
│   └── borrow_ck.tscl       # Borrow checking
├── backend/
│   └── llvm/
│       ├── mod.tscl         # LLVM IR emitter (1,348 lines)
│       ├── runtime.tscl     # Runtime stubs
│       └── types.tscl       # Type mappings
├── stdlib/
│   └── builtins.tscl        # Built-in declarations
└── pipeline.tscl            # Compilation orchestration
```

---

## Feature Parity Complete

**Goal**: Make `compiler/*.tscl` feature-complete with `bootstrap/` and add optimization passes.

### Architecture

```
Source (.tscl) ──► compiler/*.tscl ──► Bytecode ──► Rust VM
                        │
                        ├──► IR Verification
                        ├──► Type Inference  [done]
                        └──► Optimizations   [done]
```

### Tasks (All Complete)

- [x] Complete parser in `compiler/parser/` to handle all syntax
- [x] Add type inference pass (`compiler/passes/typecheck.tscl`)
- [x] Add optimization passes (`compiler/passes/opt.tscl`)
  - [x] Dead code elimination
  - [x] Constant folding
  - [x] Copy propagation
- [x] Add borrow checker (`compiler/passes/borrow_ck.tscl`)
- [x] Improve IR verification
- [x] Match bytecode output with `bootstrap/`

### CLI Commands (All Working)

```bash
script ast <file>               # Output JSON AST
script ir <file>                # Output SSA IR
script check <file>             # Type + borrow check
script build <file>             # Compile to bytecode
script run <file>               # Generate bytecode for VM
script llvm <file>              # Generate LLVM IR (.ll)
```

---

## Native Code Generation Complete

**Goal**: Add native code generation to `compiler/*.tscl`, making it a full `scriptc`.

### Architecture (Implemented)

```
Source (.tscl) ──► compiler/*.tscl (scriptc) ──┬──► Bytecode ──► VM
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
│       ├── mod.tscl         # LLVM IR emitter (1,348 lines)
│       ├── runtime.tscl     # Inlined runtime functions
│       └── types.tscl       # Type mappings
```

### Tasks (All Complete)

- [x] Design backend interface (`compiler/backend/llvm/mod.tscl`)
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
./target/release/script compiler/main.tscl llvm input.tscl

# Compile to native
clang input.tscl.ll -c -o input.o
clang input.o -o output

# Run native binary
./output
```

### Performance Results

| Test          | Native Output | VM Output | Speedup     |
| ------------- | ------------- | --------- | ----------- |
| Objects       | Match      | Match  | ~4x faster  |
| Functions     | Match      | Match  | ~4x faster  |
| Recursion     | Match      | Match  | ~30x faster |
| Loops         | Match      | Match  | ~30x faster |
| Fibonacci(25) | 75025         | 75025     | ~30x faster |

---

## Bootstrap Verification (In Progress)

**Goal**: Prove deterministic self-hosting through the bootstrap chain.

### Bootstrap Chain

```
┌─────────────────────────────────────────────────────────────┐
│  tscl₀ = src/compiler/ (Rust)                               │
│         Original compiler written in Rust                   │
└───────────────────────────┬─────────────────────────────────┘
                            │ compiles
                            ▼
┌─────────────────────────────────────────────────────────────┐
│  tscl₁ = compiler/*.tscl compiled by tscl₀                  │
│         First native scriptc binary                         │
└───────────────────────────┬─────────────────────────────────┘
                            │ compiles
                            ▼
┌─────────────────────────────────────────────────────────────┐
│  tscl₂ = compiler/*.tscl compiled by tscl₁                  │
│         Self-compiled scriptc binary                        │
└───────────────────────────┬─────────────────────────────────┘
                            │
                            ▼
┌─────────────────────────────────────────────────────────────┐
│  VERIFY: hash(tscl₁) == hash(tscl₂)                         │
│          Proves deterministic compilation                   │
└─────────────────────────────────────────────────────────────┘
```

### Verification Script

```bash
#!/bin/bash
# bootstrap_verify.sh

# Stage 1: Build scriptc with Rust compiler
./target/release/script build compiler/main.tscl -o scriptc1 --dist

# Stage 2: Build scriptc with scriptc1
./scriptc1 build compiler/main.tscl -o scriptc2 --dist

# Stage 3: Build scriptc with scriptc2
./scriptc2 build compiler/main.tscl -o scriptc3 --dist

# Verify: scriptc2 == scriptc3 (bit-for-bit)
if cmp -s scriptc2 scriptc3; then
    echo "Bootstrap verification PASSED"
    echo "   scriptc2 and scriptc3 are identical"
    sha256sum scriptc2 scriptc3
else
    echo "Bootstrap verification FAILED"
    echo "   scriptc2 and scriptc3 differ"
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

| Milestone | Status | Lines of Code |
|-----------|--------|---------------|
| Foundation | Complete | ~5,400 (bootstrap) |
| Feature parity | Complete | ~10,500 (compiler) |
| Native codegen | Complete | +1,348 (LLVM backend) |
| Bootstrap verification | In Progress | ~500 lines + testing |

**Total self-hosted code**: ~10,500 lines of .tscl

---

## Success Criteria

### Foundation (Complete)

- [x] `bootstrap/` compiles .tscl to bytecode
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

- [x] `scriptc` can compile itself (via LLVM IR)
- [ ] `hash(tscl₁) == hash(tscl₂)` verified
- [ ] CI integration for bootstrap verification

---

## Migration Strategy

Once bootstrap verification is complete:

1. **Keep Rust compiler** as reference/testing tool
2. **Primary compiler** becomes `scriptc` (compiler/\*.tscl)
3. **Remove `bootstrap/`** (superseded by `compiler/`)
4. **Optional**: Remove `src/compiler/` entirely

### Final Structure

```
script/
├── compiler/                 # THE compiler (scriptc)
│   ├── main.tscl
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

The Rust code becomes minimal runtime support, with the compiler fully in Script.
