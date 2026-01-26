---
sidebar_position: 6
title: SSA IR
description: Learn about Script's Static Single Assignment (SSA) intermediate representation used for optimization and native code generation.
keywords:
  [ssa, intermediate representation, ir, compiler optimization, code generation]
---

# SSA IR

Script compiles to a Static Single Assignment (SSA) intermediate representation for optimization and code generation.

## What is SSA?

In SSA form, each variable is assigned exactly once. This enables powerful optimizations and makes the code easier to analyze.

## IR Structure

### Types

- `Number`, `String`, `Boolean`, `Object`, `Array`, `Function`, `Any`, `Never`, `Void`

### Ownership

- `Owned`, `Moved`, `BorrowedImm`, `BorrowedMut`, `Captured`

### Storage

- `Stack`, `Heap`, `Register`

### Operations

- **Constants**: `Const`
- **Arithmetic**: `AddNum`, `SubNum`, `MulNum` and dynamic `AddAny`, `SubAny`, ...
- **Control flow**: `Jump`, `Branch`, `Return`, `Phi`
- **Memory**: `LoadLocal`, `StoreLocal`, `LoadProp`, `StoreProp`

## Example Transformation

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

## Type Specialization

The type inference pass specializes dynamic operations:

| Before           | After            | Speedup |
| ---------------- | ---------------- | ------- |
| `add.any v0, v1` | `add.num v0, v1` | ~10x    |
| `mul.any v0, v1` | `mul.num v0, v1` | ~10x    |

## Optimization Passes

1. **Dead Code Elimination (DCE)** - Remove unused code
2. **Constant Folding** - Evaluate constant expressions at compile time
3. **Common Subexpression Elimination (CSE)** - Reuse computed values
4. **Copy Propagation** - Replace copies with original values
5. **Branch Simplification** - Simplify conditional branches
6. **Unreachable Block Elimination** - Remove unreachable code

## Inspecting IR

You can dump the IR for any Script program:

```bash
./target/release/script ir myprogram.tscl
```

This prints:

- Bytecode
- SSA before optimization
- SSA after type inference
- SSA after optimizations

## Phi Nodes

Phi nodes handle values that come from multiple control flow paths:

```
IR:
  bb2: phi v5 = [(bb0, v1), (bb1, v3)]

Cranelift:
  bb2(v5: i64):
    ...
  bb0: jump bb2(v1)
  bb1: jump bb2(v3)
```

## Verification

The IR is validated for:

- Exactly-once definitions (SSA property)
- Valid control flow (jumps and blocks)
- Use-after-move detection
- Borrow checker rules
