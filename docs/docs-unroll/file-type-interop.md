---
sidebar_position: 4
title: File Type Interoperability
description: How Unroll handles different file types (.ot, .ts, .js, .nroll) with ownership-based memory management. No garbage collection.
keywords:
  [
    file types,
    interop,
    javascript,
    typescript,
    npm,
    nroll,
    ownership,
    memory,
  ]
---

# File Type Interoperability

Unroll manages imports across different file types, each with specific ownership semantics. All modes use **ownership-based memory management** — there is no garbage collector.

## Architecture Overview

```
┌─────────────────────────────────────────────────────────────────────────┐
│                         USER APPLICATION                                 │
│   import { serve } from "@rolls/http";     ← Full lifetime support       │
│   import { connect } from "@rolls/db";     ← Full lifetime support       │
│   import lodash from "lodash";             ← .nroll (copy at boundary)   │
│   import { utils } from "./helpers.ts";    ← TS compat (scope-based)     │
│   import { legacy } from "./old.js";       ← JS compat (copy-by-default) │
└───────────────────────────────────────────────────────────────────────────┘
                                    │
          ┌─────────────────────────┼─────────────────────────┐
          │                         │                         │
          ▼                         ▼                         ▼
┌─────────────────┐     ┌─────────────────┐     ┌─────────────────────────┐
│     ROLLS       │     │   NPM (.nroll)  │     │   JS/TS FILES           │
│  @rolls/http    │     │   lodash.nroll  │     │   *.js, *.ts            │
│  @rolls/fs      │     │   uuid.nroll    │     │                         │
│                 │     │                 │     │                         │
│ Full lifetime   │     │ No lifetimes    │     │ Scope-based / Copy      │
│ Zero-copy APIs  │     │ Copy at boundary│     │ Backwards compat        │
└─────────────────┘     └─────────────────┘     └─────────────────────────┘
```

## File Type Summary

| File Type | Extension | Borrow Checking | Lifetime Params | Return Borrows | Memory Model |
|-----------|-----------|-----------------|-----------------|----------------|--------------|
| Oite | `.ot` | Full | Yes | Yes | Ownership, compile-time |
| TypeScript | `.ts`, `.tsx` | Scope-based | No | No (copies) | Ownership, compile-time |
| JavaScript | `.js`, `.jsx` | None | No | No | Copy-by-default |
| npm Package | `.nroll` | N/A | No | No (copies) | Copy at boundary |

## Detailed Behavior

### `.ot` Files — Full Ownership

Native Oite files have complete ownership and lifetime tracking:

```javascript
// math.ot
export function sum<'a>(arr: &'a number[]): number {
    let total = 0;
    for (let i = 0; i < arr.length; i++) {
        total = total + arr[i];
    }
    return total;
}

// Returns a borrow with lifetime tied to input
export function first<'a, T>(arr: &'a T[]): &'a T | null {
    if (arr.length === 0) return null;
    return &arr[0];
}
```

**Characteristics:**
- Explicit `&` for borrows, `&mut` for mutable borrows
- Lifetime parameters (`'a`) for cross-function tracking
- Zero-copy performance
- Compile-time borrow checking

### `.ts` / `.tsx` Files — Scope-Based Ownership

TypeScript files use automatic scope-based borrowing:

```typescript
// utils.ts
export function process(data: number[]): number[] {
    // 'data' is automatically borrowed within this function
    return data.filter(x => x > 0).map(x => x * 2);
}

export function getFirst<T>(arr: T[]): T | null {
    // Returns a COPY, not a borrow (no lifetime annotations)
    if (arr.length === 0) return null;
    return arr[0];  // Implicit copy
}
```

**Characteristics:**
- Move by default on assignment
- Automatic borrows within scope
- No explicit lifetime annotations
- Copies when returning potentially-borrowed values
- Type annotations improve optimization

### `.js` / `.jsx` Files — Copy-by-Default

JavaScript files use implicit copying for maximum compatibility:

```javascript
// legacy.js
export function transform(data) {
    // 'data' is a COPY — original unchanged
    data.push(999);
    return data;
}

export function modify(obj) {
    // 'obj' is a COPY — original unchanged
    obj.modified = true;
    return obj;
}
```

**Characteristics:**
- Copy on assignment
- Copy when passing to functions
- No static ownership analysis
- Safe but slower
- Maximum JavaScript compatibility

### `.nroll` Files — npm Package Interop

npm packages use copy-at-boundary semantics:

```javascript
// Using lodash (npm package)
import _ from "lodash";

let items = [{ name: "b" }, { name: "a" }];
let sorted = _.sortBy(items, "name");  // items COPIED in, result COPIED out

console.log(items[0].name);   // "b" — unchanged
console.log(sorted[0].name);  // "a" — new array
```

**Rationale:**
1. JavaScript libraries assume garbage collection
2. Libraries may hold references indefinitely
3. Cannot retrofit lifetime annotations onto npm code
4. Copy-at-boundary is safe and predictable
5. Oite has no GC — ownership model only

## Cross-File Import Behavior

When importing between different file types:

| From → To | At Call Site | At Return |
|-----------|--------------|-----------|
| `.ot` → `.ot` | Move or borrow | Move or borrow |
| `.ot` → `.ts` | Move or auto-borrow | Move |
| `.ot` → `.js` | Copy | Move |
| `.ts` → `.ot` | Move (if owned) | Move |
| `.ts` → `.ts` | Move or auto-borrow | Move |
| `.ts` → `.js` | Copy | Move |
| `.js` → `.ot` | Move (caller copied) | Move |
| `.js` → `.ts` | Move (caller copied) | Move |
| `.js` → `.js` | Copy | Move |
| `*` → `.nroll` | Copy | Copy |
| `.nroll` → `*` | Copy | Copy |

### Example: Mixed Imports

```javascript
// main.ot
import { optimized } from "./fast.ot";      // Full ownership
import { convenient } from "./helper.ts";   // Scope-based
import { legacy } from "./old.js";          // Copy semantics
import _ from "lodash";                     // npm, copy at boundary

let data = [1, 2, 3, 4, 5];

// .ot function: borrows data, zero-copy
let sum = optimized(&data);

// .ts function: auto-borrows data
let doubled = convenient(data);  // data still valid after

// .js function: data is copied
let processed = legacy(data);    // data unchanged

// npm function: data is copied at boundary
let sorted = _.sortBy(data, x => -x);  // data unchanged
```

## Declaration Files for npm Packages

Provide lifetime-annotated declarations for npm packages via `.d.ot` files:

```typescript
// lodash.d.ot
declare module "lodash" {
    // Declare that first() returns a borrow into the input array
    export function first<'a, T>(arr: &'a T[]): &'a T | null;

    // Declare that find() returns a borrow
    export function find<'a, T>(
        arr: &'a T[],
        pred: fn(&T) -> bool
    ): &'a T | null;

    // Most functions return new values (no lifetime needed)
    export function map<T, U>(arr: T[], fn: fn(T) -> U): U[];
}
```

**Note:** Declaration files override the default copy-at-boundary behavior for specific functions, enabling zero-copy access to npm library results when safe.

## Configuration

Configure interop behavior in `unroll.toml`:

```toml
[build]
# Require all source files to be .ot (no .js/.ts)
strict_oite = false

# Error on .js files (allow .ts but not .js)
no_javascript = false

[interop]
# Default behavior for .js files
# "copy" = copy on assignment and call (default, safest)
# "move" = move semantics (faster, may break some patterns)
js_mode = "copy"

# Default behavior for .ts files
# "scope" = scope-based auto-borrowing (default)
# "strict" = require explicit borrows like .ot
ts_mode = "scope"

# Warn when copy overhead may impact performance
warn_copy_overhead = true
warn_copy_threshold_bytes = 1024

[npm]
# How to handle npm packages
# "copy" = copy at boundary (default, safest)
# "declare" = use .d.ot declarations when available
npm_mode = "copy"

# Directory for .d.ot declaration files
declarations = "./declarations"
```

## Performance Considerations

### Copy Overhead

`.js` files and `.nroll` packages incur copy overhead:

```javascript
// Slow: large array copied multiple times
import { process } from "./legacy.js";

let bigArray = new Array(1000000).fill(0);
let result = process(bigArray);  // Copies 1M elements
```

**Optimization:** Convert to `.ts` or `.ot`:

```typescript
// Fast: array is borrowed, not copied
// helper.ts
export function process(arr: number[]): number[] {
    return arr.map(x => x * 2);  // Borrows arr
}
```

### When to Use Each Type

| Scenario | Recommended | Why |
|----------|-------------|-----|
| New code | `.ot` | Maximum performance and safety |
| Migrating large codebase | `.ts` first | Good balance, easy migration |
| Quick prototyping | `.js` | Just works, optimize later |
| npm dependencies | `.nroll` | Automatic, safe |
| Performance-critical npm usage | `.d.ot` + `.nroll` | Zero-copy where declared safe |

## Migration Strategy

### Phase 1: Run Existing JavaScript

```javascript
// Just rename or import your .js files — they work immediately
import { oldCode } from "./legacy.js";
```

### Phase 2: Convert to TypeScript

```typescript
// Add types, get scope-based optimization
// helper.ts
export function transform(data: number[]): number[] {
    return data.filter(x => x > 0);
}
```

### Phase 3: Convert to Oite

```javascript
// Add explicit ownership for maximum control
// helper.ot
export function transform(data: &number[]): number[] {
    return data.filter(x => x > 0);
}
```

## See Also

- [JavaScript Interoperability](/javascript-interop) — Detailed .js/.ts behavior
- [ECMAScript Conformance](/ecmascript-conformance) — Semantic differences
- [Lifetimes](/lifetimes) — Lifetime system deep dive
- [Module System](/unroll/modules) — Import resolution
