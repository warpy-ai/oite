---
sidebar_position: 6
title: ECMAScript Conformance
description: Understanding Oite's relationship with ECMAScript standards and Test262. Learn which JavaScript behaviors are supported, which differ, and why.
keywords:
  [
    ecmascript,
    test262,
    conformance,
    javascript,
    compatibility,
    standards,
    specification,
  ]
---

# ECMAScript Conformance

Oite uses **JavaScript syntax** but implements **ownership semantics** rather than standard ECMAScript reference semantics. This document clarifies what this means for compatibility.

## Oite is Not a JavaScript Runtime

Oite is a new language that:

- Uses JavaScript/TypeScript **syntax** (familiar to JS developers)
- Implements **ownership-based memory management** (no garbage collector)
- Compiles to **native code** (not interpreted)

```
┌─────────────────────────────────────────────────────────────────┐
│                    LANGUAGE COMPARISON                           │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│   JavaScript          Oite                                       │
│   ───────────         ────                                       │
│   Reference semantics → Ownership semantics                      │
│   Garbage collected   → Deterministic memory                     │
│   Interpreted/JIT     → Native compiled                          │
│   Dynamic types       → Optional static types                    │
│   Single-threaded*    → Memory-safe concurrency (planned)        │
│                                                                  │
│   * with event loop                                              │
└─────────────────────────────────────────────────────────────────┘
```

## Test262 and Oite

[Test262](https://github.com/tc39/test262) is the official ECMAScript conformance test suite. It tests JavaScript **semantics**, not just syntax.

### Why Full Test262 Compliance is Not a Goal

Test262 tests assume:

- **Reference semantics** — multiple variables can reference the same object
- **Garbage collection** — memory is automatically reclaimed
- **Specific coercion rules** — implicit type conversions
- **Prototype-based inheritance** — object prototype chains

Oite intentionally differs:

- **Ownership semantics** — objects have a single owner
- **No GC** — deterministic memory via ownership
- **Explicit behavior** — minimal implicit conversions
- **Class-based** — ES6 classes with ownership

### Test Categories

| Category               | Expected Compatibility | Notes                           |
| ---------------------- | ---------------------- | ------------------------------- |
| **Syntax**             | High                   | Same parser (SWC)               |
| **Operators**          | High                   | Arithmetic, logical, comparison |
| **Primitives**         | High                   | number, string, boolean         |
| **Control flow**       | High                   | if, for, while, switch          |
| **Functions**          | High                   | Declarations, arrows, closures  |
| **Classes**            | High                   | ES6 class syntax                |
| **Object identity**    | Low                    | Copy semantics differ           |
| **Reference mutation** | Low                    | No shared references            |
| **Built-in objects**   | Medium                 | Implemented incrementally       |
| **Prototype chain**    | Medium                 | Supported but less central      |

## Semantic Differences

### 1. Object Assignment (Copy vs Reference)

```javascript
// ECMAScript behavior:
let a = { value: 1 };
let b = a;
b.value = 2;
console.log(a.value); // 2 (same object)
console.log(a === b); // true

// Oite .js behavior:
let a = { value: 1 };
let b = a; // COPY
b.value = 2;
console.log(a.value); // 1 (different objects)
console.log(a === b); // false
```

**Rationale:** Ownership semantics require clear ownership. Copy-by-default in `.js` mode ensures predictable behavior without GC.

### 2. Function Parameter Passing

```javascript
// ECMAScript behavior:
function modify(obj) {
  obj.value = 99;
}
let x = { value: 1 };
modify(x);
console.log(x.value); // 99 (modified)

// Oite .js behavior:
function modify(obj) {
  obj.value = 99;
}
let x = { value: 1 };
modify(x); // COPY passed
console.log(x.value); // 1 (unchanged)
```

**Rationale:** Functions receive copies in `.js` mode. Use `.ot` with explicit borrows for in-place modification.

### 3. Array Methods

```javascript
// Both behave similarly for most methods:
let arr = [1, 2, 3];
let doubled = arr.map((x) => x * 2); // [2, 4, 6]

// But mutation methods differ:
// ECMAScript:
let items = [3, 1, 2];
items.sort();
console.log(items); // [1, 2, 3] (mutated in place)

// Oite .js: same behavior (mutation is on owned copy)
let items = [3, 1, 2];
items.sort(); // Mutates the copy we own
console.log(items); // [1, 2, 3]
```

### 4. Closures and Captured Variables

```javascript
// ECMAScript behavior:
function counter() {
  let count = 0;
  return () => ++count;
}
let c = counter();
console.log(c()); // 1
console.log(c()); // 2

// Oite behavior: SAME
// Closures capture ownership of their environment
function counter() {
  let count = 0;
  return () => ++count; // count is MOVED into closure
}
let c = counter();
console.log(c()); // 1
console.log(c()); // 2
```

Closures work as expected — captured variables are owned by the closure.

## Built-in Object Support

Oite implements JavaScript built-in objects incrementally:

### Fully Implemented

| Object    | Methods                                                                       |
| --------- | ----------------------------------------------------------------------------- |
| `console` | `log`, `error`, `warn`                                                        |
| `Array`   | `push`, `pop`, `map`, `filter`, `reduce`, `forEach`, `find`, `join`, `length` |
| `String`  | `length`, `charAt`, `fromCharCode`, concatenation                             |
| `Object`  | Literal syntax, property access, computed properties                          |
| `Promise` | `resolve`, `reject`, `then`, `catch`, `finally`                               |
| `JSON`    | `parse`, `stringify`                                                          |

### Planned (via Rolls ecosystem)

| Object       | Location             | Status  |
| ------------ | -------------------- | ------- |
| `Math`       | `@rolls/math`        | Planned |
| `Date`       | `@rolls/date`        | Planned |
| `RegExp`     | `@rolls/regex`       | Planned |
| `Map`, `Set` | `@rolls/collections` | Planned |
| `Buffer`     | `@rolls/buffer`      | Planned |
| `crypto`     | `@rolls/crypto`      | Planned |

### Not Planned

| Object               | Reason                                   |
| -------------------- | ---------------------------------------- |
| `Proxy`              | Conflicts with static ownership analysis |
| `Reflect`            | Limited use case without Proxy           |
| `eval()`             | Security and optimization concerns       |
| `with`               | Deprecated, scope confusion              |
| `WeakMap`, `WeakSet` | Requires GC semantics                    |

## Oite262: Our Test Suite

Instead of Test262 compliance, Oite maintains **Oite262** — a test suite that:

1. **Validates syntax compatibility** — Can parse standard JS/TS
2. **Tests ownership semantics** — Correct move/copy/borrow behavior
3. **Documents differences** — Clear expectations for each test
4. **Covers built-ins** — Tests implemented standard library

### Running Tests

```bash
# Run Oite test suite
cargo test

# Run Oite262 JavaScript compatibility tests
./target/release/oitec test tests/oite262/
```

### Test Structure

```
tests/oite262/
├── syntax/           # Parsing tests (high compat)
├── operators/        # Arithmetic, logical (high compat)
├── control-flow/     # if, for, while (high compat)
├── functions/        # Declarations, closures (high compat)
├── classes/          # ES6 classes (high compat)
├── ownership/        # Oite-specific semantics
├── builtins/         # Built-in object tests
└── differences/      # Documented semantic differences
```

## Compatibility Checker

Check if your JavaScript code is compatible with Oite:

```bash
# Analyze a file for potential issues
oite check --compat myfile.js

# Output:
# myfile.js:15: warning: Object identity check (===) may behave differently
# myfile.js:28: warning: Shared reference pattern detected
# myfile.js:42: info: Consider using .ts for better performance
```

## Migration Recommendations

### Patterns That Work Unchanged

```javascript
// Pure functions ✓
function add(a, b) {
  return a + b;
}

// Array transformations ✓
let doubled = arr.map((x) => x * 2);

// Object creation ✓
let user = { name: "Alice", age: 30 };

// Closures ✓
let counter = (() => {
  let n = 0;
  return () => ++n;
})();

// Async/await ✓
async function fetchData() {
  return await api.get();
}
```

### Patterns That Need Adjustment

```javascript
// Shared mutable state ✗
let shared = {};
function a() { shared.x = 1; }
function b() { shared.y = 2; }

// Fix: Use .ot with explicit borrows
// file.ot
let shared = {};
function a(s: &mut Object) { s.x = 1; }
function b(s: &mut Object) { s.y = 2; }
a(&mut shared);
b(&mut shared);

// Object identity checks ✗
if (objA === objB) { ... }

// Fix: Use deep equality or unique IDs
if (deepEqual(objA, objB)) { ... }
if (objA.id === objB.id) { ... }

// Prototype manipulation ✗
obj.__proto__ = newProto;

// Fix: Use class inheritance
class NewClass extends OldClass { ... }
```

## Summary

| Aspect            | Standard JavaScript   | Oite                     |
| ----------------- | --------------------- | ------------------------ |
| **Goal**          | ECMAScript compliance | Performance + Safety     |
| **Memory**        | Garbage collected     | Ownership-based          |
| **Assignment**    | Reference copy        | Value copy or move       |
| **Test suite**    | Test262               | Oite262                  |
| **Compatibility** | 100% spec             | Syntax + common patterns |

Oite prioritizes **predictable performance** and **memory safety** over strict ECMAScript compatibility. Most JavaScript code works unchanged; patterns relying on shared mutable references need adjustment.

## See Also

- [JavaScript Interoperability](/javascript-interop) — How .js files run in Oite
- [Language Features](/compiler/language-features) — Full feature reference
- [Migration Guide](#migration-recommendations) — Adapting JS patterns for Oite
