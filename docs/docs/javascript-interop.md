---
sidebar_position: 5
title: JavaScript & TypeScript Interoperability
description: How Oite runs JavaScript and TypeScript files with ownership semantics. Learn about copy-by-default behavior, performance tiers, and migration paths from JS to Oite.
keywords:
  [
    javascript,
    typescript,
    interop,
    compatibility,
    jsx,
    tsx,
    node,
    npm,
    ownership,
    memory model,
  ]
---

# JavaScript & TypeScript Interoperability

Oite can run `.js`, `.ts`, `.jsx`, and `.tsx` files alongside native `.ot` files. All file types use **ownership semantics** — there is no garbage collector. The difference is how ownership is managed.

## Core Principle: No GC, Ever

Unlike Node.js, Bun, or Deno, Oite never uses garbage collection. Instead, all file types use ownership-based memory management:

| File Type | Memory Model | Description |
|-----------|--------------|-------------|
| `.ot` | Move + Borrow | Full ownership control, zero-copy |
| `.ts` | Move + Auto-borrow | Scope-based borrows, some implicit copies |
| `.js` | Copy-by-default | Implicit cloning, maximum compatibility |
| `.nroll` | Copy at boundary | npm packages, isolated memory |

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                         ALL MODES: OWNERSHIP (NO GC)                         │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                              │
│  ┌────────────────┐  ┌────────────────┐  ┌────────────────┐  ┌───────────┐  │
│  │     .ot        │  │     .ts        │  │     .js        │  │  .nroll   │  │
│  │                │  │                │  │                │  │           │  │
│  │  Move default  │  │  Move default  │  │  Copy default  │  │  Copy at  │  │
│  │  Explicit &    │  │  Scope-based & │  │  Implicit      │  │  boundary │  │
│  │  Full borrow   │  │  Auto-borrow   │  │  cloning       │  │           │  │
│  │  Zero-copy     │  │  Some copies   │  │  Safe & slow   │  │           │  │
│  └────────────────┘  └────────────────┘  └────────────────┘  └───────────┘  │
│                                                                              │
│                      No GC — Deterministic memory                            │
└─────────────────────────────────────────────────────────────────────────────┘
```

## File Type Behaviors

### `.ot` Files — Full Ownership Control

Native Oite files have explicit ownership with move semantics and borrow checking:

```javascript
// file.ot
let a = { value: 1 };
let b = a;              // MOVE: 'a' is no longer valid
// console.log(a.value); // ERROR: use after move

let c = { value: 2 };
let d = &c;             // BORROW: 'd' references 'c'
console.log(d.value);   // OK: 2
console.log(c.value);   // OK: 'c' still valid (was borrowed, not moved)
```

**Characteristics:**
- Move by default on assignment
- Explicit borrows with `&` and `&mut`
- Compile-time borrow checking
- Zero-copy performance
- Full lifetime tracking

### `.ts` Files — Scope-Based Ownership

TypeScript files use move semantics with automatic scope-based borrowing:

```typescript
// file.ts
function process(data: number[]): number {
    // 'data' is automatically borrowed within this scope
    return data.reduce((a, b) => a + b, 0);
}

let items = [1, 2, 3];
let sum = process(items);  // 'items' borrowed, not moved
console.log(items.length); // OK: 'items' still valid
```

**Characteristics:**
- Move by default on assignment
- Automatic borrows within function scopes
- No explicit lifetime annotations
- Copies when borrows cannot be proven safe
- Type annotations help optimization

### `.js` Files — Copy-by-Default

JavaScript files use implicit copy semantics for maximum compatibility:

```javascript
// file.js
let a = { value: 1 };
let b = a;              // COPY: 'b' is a clone of 'a'
b.value = 2;
console.log(a.value);   // 1 — 'a' is unchanged
console.log(b.value);   // 2 — 'b' is independent

function update(obj) {
    obj.value = 99;
    return obj;
}

let c = { value: 0 };
let d = update(c);      // 'c' COPIED into function
console.log(c.value);   // 0 — 'c' unchanged
console.log(d.value);   // 99
```

**Characteristics:**
- Copy on assignment (implicit clone)
- Copy when passing to functions
- No static ownership checking
- Safe but slower than `.ot`
- Runs most JavaScript code correctly

### `.nroll` Files — npm Package Interop

npm packages are wrapped with copy-at-boundary semantics:

```javascript
import _ from "lodash";  // .nroll package

let items = [3, 1, 2];
let sorted = _.sortBy(items, x => x);  // 'items' COPIED in, result COPIED out
console.log(items);   // [3, 1, 2] — unchanged
console.log(sorted);  // [1, 2, 3] — new array
```

**Characteristics:**
- All data copied when crossing boundary
- npm code runs in isolated context
- No lifetime tracking across boundary
- Safe interop with any npm package

## How Copy-by-Default Works

In `.js` mode, Oite automatically clones objects on assignment and function calls:

```javascript
// What you write:
let a = { x: 1, y: { z: 2 } };
let b = a;

// What Oite does internally:
let a = { x: 1, y: { z: 2 } };
let b = deepClone(a);  // Implicit deep copy
```

### Primitives Are Unaffected

Primitive values (numbers, strings, booleans) are already value types:

```javascript
let x = 42;
let y = x;    // Copy (same as standard JS)
y = 100;
console.log(x);  // 42
```

### Arrays and Objects Are Copied

```javascript
let arr1 = [1, 2, 3];
let arr2 = arr1;      // Deep copy
arr2.push(4);
console.log(arr1);    // [1, 2, 3] — unchanged
console.log(arr2);    // [1, 2, 3, 4]

let obj1 = { nested: { value: 1 } };
let obj2 = obj1;      // Deep copy
obj2.nested.value = 2;
console.log(obj1.nested.value);  // 1 — unchanged
console.log(obj2.nested.value);  // 2
```

## Semantic Differences from Standard JavaScript

Oite's `.js` mode is **not** standard ECMAScript. Key differences:

### Object Identity

```javascript
// Standard JavaScript:
let a = {};
let b = a;
console.log(a === b);  // true (same reference)

// Oite .js mode:
let a = {};
let b = a;             // Copy
console.log(a === b);  // false (different objects)
```

### Mutation Through References

```javascript
// Standard JavaScript:
let a = { value: 1 };
let b = a;
b.value = 2;
console.log(a.value);  // 2 (same object)

// Oite .js mode:
let a = { value: 1 };
let b = a;             // Copy
b.value = 2;
console.log(a.value);  // 1 (different objects)
```

### Shared State Patterns

Some JavaScript patterns rely on shared references:

```javascript
// This pattern works differently in Oite:
let shared = { count: 0 };
let ref1 = shared;
let ref2 = shared;
ref1.count++;
ref2.count++;
console.log(shared.count);
// Standard JS: 2
// Oite .js: 0 (each is a separate copy)
```

**Solution:** Use `.ot` files with explicit borrows for shared state:

```javascript
// file.ot
let shared = { count: 0 };
let ref1 = &mut shared;
ref1.count++;
// ref1 goes out of scope, borrow released
let ref2 = &mut shared;
ref2.count++;
console.log(shared.count);  // 2
```

## Performance Tiers

```
┌─────────────────────────────────────────────────────────────────┐
│                      PERFORMANCE SPECTRUM                        │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  .ot (fastest)          .ts (fast)           .js (compatible)   │
│  ─────────────────────────────────────────────────────────────  │
│  Zero-copy              Some copies          Copy everything     │
│  Explicit control       Inferred borrows     Implicit clones     │
│  Maximum perf           Good perf            Maximum compat      │
│                                                                  │
│  "Rust mode"            "Swift mode"         "Value mode"        │
└─────────────────────────────────────────────────────────────────┘
```

| Operation | `.ot` | `.ts` | `.js` |
|-----------|-------|-------|-------|
| Assignment | Move (free) | Move (free) | Copy (O(n)) |
| Function call | Borrow (free) | Auto-borrow (free) | Copy (O(n)) |
| Return value | Move (free) | Move (free) | Move (free) |
| Array iteration | Borrow (free) | Auto-borrow (free) | Copy elements |

## Migration Path

Gradually migrate JavaScript code to Oite for better performance:

### Step 1: Start with `.js`

```javascript
// utils.js — works immediately, copies everything
function processData(data) {
    return data.filter(x => x > 0).map(x => x * 2);
}

let items = [1, -2, 3, -4, 5];
let result = processData(items);
```

### Step 2: Rename to `.ts` for Optimization

```typescript
// utils.ts — compiler infers borrows, fewer copies
function processData(data: number[]): number[] {
    return data.filter(x => x > 0).map(x => x * 2);
}

let items: number[] = [1, -2, 3, -4, 5];
let result = processData(items);  // 'items' borrowed, not copied
```

### Step 3: Convert to `.ot` for Full Control

```javascript
// utils.ot — explicit ownership, zero-copy
function processData(data: &number[]): number[] {
    return data.filter(x => x > 0).map(x => x * 2);
}

let items = [1, -2, 3, -4, 5];
let result = processData(&items);  // Explicit borrow
console.log(items);  // Still valid
```

## Mixing File Types

You can import any file type from any other:

```javascript
// main.ot
import { helper } from "./helper.js";    // .js module
import { utils } from "./utils.ts";      // .ts module
import { core } from "./core.ot";        // .ot module
import _ from "lodash";                  // .nroll package

// Each import uses appropriate semantics at boundaries
```

### Boundary Behavior

| From → To | Behavior |
|-----------|----------|
| `.ot` → `.ot` | Move/borrow (zero-copy) |
| `.ot` → `.ts` | Move/auto-borrow |
| `.ot` → `.js` | Copy (for safety) |
| `.ts` → `.ot` | Move (if owned) |
| `.js` → `.ot` | Move (caller loses access) |
| `*` → `.nroll` | Copy at boundary |
| `.nroll` → `*` | Copy at boundary |

## When to Use Each File Type

| Use Case | Recommended | Why |
|----------|-------------|-----|
| Performance-critical code | `.ot` | Full control, zero-copy |
| Application logic | `.ts` | Good balance of safety and convenience |
| Quick scripts | `.js` | Maximum compatibility |
| Porting existing JS | `.js` | Works immediately |
| Using npm packages | `.nroll` | Automatic via Unroll |
| Shared state / complex ownership | `.ot` | Explicit borrows required |

## Configuration

Configure interop behavior in `unroll.toml`:

```toml
[build]
# Error if .js files are detected (enforce .ts/.ot)
strict_mode = false

[interop]
# Default behavior for .js files
js_mode = "copy"  # "copy" (default) or "move" (advanced)

# Warn when .js performance overhead is high
warn_copy_overhead = true
warn_copy_threshold = 1000  # bytes
```

## FAQ

### Will my JavaScript code work in Oite?

Most JavaScript code will work in `.js` mode. Code that relies on shared mutable references will behave differently due to copy semantics.

### Is this slower than Node.js?

For `.js` files with heavy object passing, yes — copying has overhead. For `.ot` files, Oite is significantly faster due to zero-copy semantics and native compilation.

### Can I use npm packages?

Yes. npm packages are automatically wrapped as `.nroll` with copy-at-boundary semantics. See the [Unroll documentation](/unroll/intro) for details.

### What about React/JSX?

`.jsx` and `.tsx` files are supported with the same semantics as `.js` and `.ts` respectively. JSX is transformed during compilation.

### How do I share state between modules?

For shared mutable state, use `.ot` files with explicit ownership:

```javascript
// state.ot
export let appState = { count: 0 };

export function increment(state: &mut typeof appState) {
    state.count++;
}

// main.ot
import { appState, increment } from "./state.ot";
increment(&mut appState);
```

## See Also

- [Lifetimes](/lifetimes) — Deep dive into lifetime tracking
- [Unroll Package Manager](/unroll/intro) — npm interop details
- [Memory Model](/rolls/memory-model) — How ownership works internally
