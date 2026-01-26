---
sidebar_position: 5
title: Memory Model
description: Understand Script's Rust-inspired memory model with ownership rules, borrow checking, and memory safety without garbage collection.
keywords:
  [memory model, ownership, borrow checker, memory safety, no gc, rust-inspired]
---

# Memory Model

Script uses a Rust-inspired ownership system for memory safety without garbage collection.

## Ownership Rules

1. Each value has exactly one owner
2. Assigning objects **moves** ownership
3. Primitives (numbers, booleans) are **copied**
4. Variables are freed when their scope ends

## Examples

```javascript
let a = { value: 42 };
let b = a; // 'a' is MOVED to 'b'
// console.log(a.value);  // ERROR: use after move!
console.log(b.value); // OK: 42

// Primitives are copied
let x = 10;
let y = x; // 'x' is COPIED
console.log(x); // OK: 10
```

## Borrowing

Script supports immutable and mutable references:

```javascript
// Immutable reference
let obj = { x: 10 };
let ref = &obj;           // Immutable borrow
console.log(ref.x);       // OK: can read
// ref.x = 20;            // ERROR: cannot mutate through immutable ref

// Mutable reference
let mut_ref = &mut obj;   // Mutable borrow
mut_ref.x = 20;           // OK: can mutate
```

## Borrow Checker Rules

- No overlapping mutable borrows
- Ownership and lifetime sanity
- Use-after-move detection at compile time

## Runtime Representation

All values are represented as **64-bit NaN-boxed** words:

- Booleans, null, undefined encoded in NaN space
- Pointers to heap objects
- Uniform representation for VM and native backends

## Heap Allocation

Objects, arrays, and strings are allocated on the heap:

- Bump allocator for fast allocation
- Automatic deallocation when scope ends
- No garbage collection overhead
