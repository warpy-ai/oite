# Lifetime Parameters

Lifetimes are Oite's way of tracking how long references are valid. They enable you to write functions that return borrowed references safely, without copying data.

## Why Lifetimes Matter

Oite uses **ownership-based memory management** (like Rust), not garbage collection. When you borrow data with `&`, the compiler needs to ensure you don't use that reference after the original data is freed.

```javascript
// Problem: Which input does the return value borrow from?
function pick(a: &string, b: &string): &string {
    return a;  // Returns reference to 'a'
}

let result;
{
    let s1 = "hello";
    let s2 = "world";
    result = pick(&s1, &s2);
}  // s1 and s2 are freed here!

console.log(result);  // ERROR: dangling reference!
```

Lifetimes solve this by explicitly tracking which input the output borrows from.

## Basic Syntax

### Lifetime Parameters

Lifetime parameters are declared with a leading apostrophe:

```javascript
// 'a is a lifetime parameter
function find<'a>(haystack: &'a number[], needle: number): &'a number | null {
    for (let i = 0; i < haystack.length; i++) {
        if (haystack[i] === needle) {
            return &haystack[i];
        }
    }
    return null;
}
```

The `'a` connects the input and output: the returned reference lives as long as `haystack`.

### Reference Types with Lifetimes

| Syntax | Meaning |
|--------|---------|
| `&T` | Immutable borrow (lifetime inferred) |
| `&mut T` | Mutable borrow (lifetime inferred) |
| `&'a T` | Immutable borrow with lifetime `'a` |
| `&'a mut T` | Mutable borrow with lifetime `'a` |

### The `'static` Lifetime

The special `'static` lifetime means "lives for the entire program":

```javascript
// String literals are 'static
let s: &'static string = "hello world";

// Constants are 'static
const CONFIG: &'static Config = { debug: true };
```

## Lifetime Elision (When You Don't Need Lifetimes)

Most of the time, you don't need to write lifetimes explicitly. The compiler infers them using these rules:

### Rule 1: Single Input Reference

If there's exactly one input reference, the output gets the same lifetime:

```javascript
// You write:
function first(arr: &number[]): &number {
    return arr[0];
}

// Compiler sees:
function first<'a>(arr: &'a number[]): &'a number {
    return arr[0];
}
```

### Rule 2: Method Receiver

For methods, the output borrows from `self`:

```javascript
class Vec<T> {
    // You write:
    get(index: number): &T | null {
        return this.data[index];
    }

    // Compiler sees:
    get<'self>(index: number): &'self T | null {
        return this.data[index];
    }
}
```

### Rule 3: Multiple Inputs (Explicit Required)

When there are multiple input references and no `self`, you must be explicit:

```javascript
// ERROR: Ambiguous - which input does output borrow from?
function longest(a: &string, b: &string): &string {
    return a.length >= b.length ? a : b;
}

// CORRECT: Explicit lifetime shows both inputs must live as long as output
function longest<'a>(a: &'a string, b: &'a string): &'a string {
    return a.length >= b.length ? a : b;
}
```

## Common Patterns

### Finding Elements

```javascript
// Return reference to element in collection
function find<'a, T>(arr: &'a T[], predicate: (x: &T) => boolean): &'a T | null {
    for (let i = 0; i < arr.length; i++) {
        if (predicate(&arr[i])) {
            return &arr[i];
        }
    }
    return null;
}

// Usage
let numbers = [1, 2, 3, 4, 5];
let found = find(&numbers, x => x > 3);
console.log(found);  // 4
```

### String Views (Zero-Copy Slicing)

```javascript
struct StringView<'a> {
    source: &'a string;
    start: number;
    len: number;
}

function slice<'a>(s: &'a string, start: number, len: number): StringView<'a> {
    return { source: s, start: start, len: len };
}

// No copying - just a view into the original string
let text = "hello world";
let view = slice(&text, 0, 5);  // view borrows from text
```

### Iterators

```javascript
struct ArrayIter<'a, T> {
    data: &'a T[];
    index: number;
}

function iter<'a, T>(arr: &'a T[]): ArrayIter<'a, T> {
    return { data: arr, index: 0 };
}

function next<'a, T>(it: &mut ArrayIter<'a, T>): &'a T | null {
    if (it.index >= it.data.length) {
        return null;
    }
    let result = &it.data[it.index];
    it.index = it.index + 1;
    return result;
}

// Usage
let items = [1, 2, 3];
let it = iter(&items);
while (let item = next(&mut it)) {
    console.log(item);
}
```

## Lifetime Variance

References have different "variance" rules:

| Type | Variance | Meaning |
|------|----------|---------|
| `&'a T` | Covariant | Can shorten lifetime (longer → shorter OK) |
| `&'a mut T` | Invariant | Must be exact match |

```javascript
// Covariant: longer lifetime can be used where shorter expected
let long_ref: &'long T = ...;
let short_ref: &'short T = long_ref;  // OK if 'long outlives 'short

// Invariant: mutable refs must match exactly
let mut_ref: &'a mut T = ...;
let other: &'b mut T = mut_ref;  // ERROR unless 'a == 'b
```

## Error Messages

### E0501: Cannot Return Reference to Local Variable

```
error[E0501]: cannot return reference to local variable
  --> src/example.ot:5:12
   |
 4 |     let local = [1, 2, 3];
   |         ----- local variable declared here
 5 |     return &local[0];
   |            ^^^^^^^^^ returns reference to data owned by current function
```

**Fix:** Return by value, or take the data as a parameter with a lifetime.

### E0502: Lifetime Too Short

```
error[E0502]: lifetime 'a does not live long enough
  --> src/example.ot:10:5
   |
   = help: The reference must be valid for 'b but it only lives for 'a
```

**Fix:** Ensure the borrowed data lives long enough, or restructure to avoid the borrow.

### E0505: Ambiguous Lifetime

```
error[E0505]: missing lifetime specifier
  --> src/example.ot:2:40
   |
 2 | function pick(a: &T, b: &T): &T {
   |                                 ^ expected named lifetime parameter
   |
help: consider introducing a named lifetime parameter: <'a>
```

**Fix:** Add explicit lifetime parameter to clarify which input the output borrows from.

## Interop with JavaScript/TypeScript

### .ot Files (Full Lifetime Support)

```javascript
// Full lifetime tracking, zero-copy APIs
export function first<'a, T>(arr: &'a T[]): &'a T | null {
    return arr.length > 0 ? &arr[0] : null;
}
```

### .ts Files (Scope-Based Only)

TypeScript files use scope-based borrow checking. References are valid within the function but cannot be returned:

```typescript
// utils.ts - TypeScript
export function process(arr: number[]): number {
    return arr[0];  // Returns by value (copy)
}
```

### .js Files (No Static Checking)

JavaScript files run with ownership semantics but without compile-time checking:

```javascript
// utils.js - dynamic mode
export function process(data) {
    return data[0];  // No static checking, runtime ownership
}
```

### npm Packages (.nroll)

npm packages use copy-at-boundary semantics for safety:

```javascript
import _ from "lodash";

let items = [1, 2, 3];
let sorted = _.sortBy(items);  // items is COPIED to lodash
// items still usable (wasn't moved)
```

## Summary

| Concept | Syntax | When to Use |
|---------|--------|-------------|
| Lifetime param | `<'a>` | Function/struct returning borrowed data |
| Immutable ref | `&'a T` | Read-only access with explicit lifetime |
| Mutable ref | `&'a mut T` | Write access with explicit lifetime |
| Static lifetime | `'static` | Data that lives forever (literals, constants) |
| Elision | (no annotation) | Single input or method receiver |

**Remember:** If the compiler asks for a lifetime, it's because it can't figure out how long your reference should live. The lifetime annotation tells it which input the output is borrowing from.
