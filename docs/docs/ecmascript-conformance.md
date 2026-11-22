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

Oite uses **JavaScript syntax**. At run time, objects are shared by reference, as in JavaScript. A compile-time move check rejects some programs that JavaScript accepts. At the top level of a file, a program cannot use a variable after it assigned the variable's value to another variable. This page lists how Oite differs from ECMAScript.

## Oite is Not a JavaScript Runtime

Oite is a new language that:

- Uses JavaScript/TypeScript **syntax** (familiar to JS developers)
- Has **no garbage collector**, and does not free memory yet ([#123](https://github.com/warpy-ai/oite/issues/123))
- Compiles to **native code** (not interpreted)

```
┌─────────────────────────────────────────────────────────────────┐
│                    LANGUAGE COMPARISON                           │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│   JavaScript          Oite                                       │
│   ───────────         ────                                       │
│   Reference semantics → Reference semantics + move check         │
│   Garbage collected   → Nothing freed yet (#123)                 │
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

- **Move check** — at the top level of a file, `let b = a` moves the value out of `a`
- **No GC** — memory is not freed yet
- **Explicit behavior** — minimal implicit conversions
- **Class-based** — ES6 classes

### Test Categories

| Category               | Expected Compatibility | Notes                           |
| ---------------------- | ---------------------- | ------------------------------- |
| **Syntax**             | High                   | Same parser (SWC)               |
| **Operators**          | High                   | Arithmetic, logical, comparison |
| **Primitives**         | High                   | number, string, boolean         |
| **Control flow**       | High                   | if, for, while, switch          |
| **Functions**          | High                   | Declarations, arrows, closures  |
| **Classes**            | High                   | ES6 class syntax                |
| **Object identity**    | High                   | Objects are shared by reference |
| **Reference mutation** | Medium                 | Shared; see the move check      |
| **Built-in objects**   | Medium                 | Implemented incrementally       |
| **Prototype chain**    | Medium                 | Supported but less central      |

## Semantic differences

### 1. Assignment moves at the top level

```javascript
// ECMAScript:
let a = { value: 1 };
let b = a;
b.value = 2;
console.log(a.value); // 2 (same object)

// Oite, at the top level of a file:
let a = { value: 1 };
let b = a; // the value moves from 'a' to 'b'
b.value = 2;
console.log(a.value);
// borrow error: Cannot borrow moved variable 'a'
```

The program does not compile, on any backend and for `.ot`, `.ts` and `.js` files alike. Read the object through `b` instead.

The move check does not read function bodies. Inside a function, the same code prints `2`, as in ECMAScript, and `a === b` is `true`.

A variable that was initialized with a number or boolean literal is copied, not moved. [Memory model](/rolls/memory-model) lists the full rules.

### 2. Function parameters

Passing a value to a function borrows it for the statement and does not move it. The function receives the same object:

```javascript
function modify(obj) {
  obj.value = 99;
}
let x = { value: 1 };
modify(x);
console.log(x.value); // 99, as in ECMAScript
```

### 3. Closures

A closure that is not `async` behaves as in ECMAScript. At the top level, an `async` arrow function or `async` function expression that names a variable moves its value into the closure, and a later use of the variable is a compile error:

```javascript
let a = { v: 1 };
let f = async () => { console.log(a.v); };
console.log(a.v);
// borrow error: Cannot borrow 'a' - it was captured by an async closure!
```

### 4. Strings Count Characters, Not UTF-16 Code Units

An Oite string is a sequence of Unicode scalar values (code points other
than surrogates), stored as UTF-8. `length`, `s[i]`, and every index a
string method takes or returns count those characters, on every backend.

```javascript
"héllo".length;       // 5, as in JavaScript
"日本語"[1];           // "本", as in JavaScript
"😀".length;          // 1 (JavaScript: 2)
"a😀b"[2];            // "b" (JavaScript: "\ude00", half a surrogate pair)
"a😀b".charCodeAt(1); // 128512 (JavaScript: 55357)
```

JavaScript counts UTF-16 code units. For text in the Basic Multilingual
Plane (Latin, Greek, Cyrillic, CJK, and most other scripts) a character is
one code unit, so the results match JavaScript. They differ only for astral
characters, such as emoji, which JavaScript counts as two code units (a
surrogate pair) and Oite counts as one character. `for...of` and spread
iterate code points in JavaScript too, so they agree in every case.

`<` and `>` order strings by code point. JavaScript orders them by UTF-16
code unit, which differs only when an astral character is compared with
one in U+E000 to U+FFFF.

**Rationale:** Counting UTF-16 code units over UTF-8 storage would need a
second encoding or an index for every string, and would expose surrogate
halves that are not characters. Native code reads an ASCII string's length
and characters directly. Indexing a non-ASCII string walks it from the
start.

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

## Tests

Oite does not run Test262. Its language tests are golden tests in `tests/lang/`: each program has the output it must print, and `cargo test --test golden` runs it on the backends it lists.

## Migration Recommendations

### Patterns that work unchanged

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

### Patterns that need adjustment

```javascript
// Two names for one object at the top level ✗
let config = { debug: false };
let current = config;
current.debug = true;
console.log(config.debug); // borrow error: Cannot borrow moved variable 'config'

// Fix: use one name, or do the work inside a function
let config = { debug: false };
config.debug = true;
console.log(config.debug);
```

## Summary

| Aspect            | Standard JavaScript   | Oite                     |
| ----------------- | --------------------- | ------------------------ |
| **Goal**          | ECMAScript compliance | Performance + Safety     |
| **Memory**        | Garbage collected     | Not freed yet (#123)     |
| **Assignment**    | Reference copy        | Reference copy; a top-level `let b = a` also moves |
| **Test suite**    | Test262               | Golden tests in `tests/lang/` |
| **Compatibility** | 100% spec             | Syntax + common patterns |

Most JavaScript code that Oite supports runs with the same results. Code that uses a variable at the top level after it assigned the variable's value to another variable does not compile.

## See Also

- [JavaScript Interoperability](/javascript-interop) — How .js files run in Oite
- [Language Features](/compiler/language-features) — Full feature reference
- [Memory model](/rolls/memory-model) — The move check rules
