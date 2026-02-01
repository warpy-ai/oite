---
sidebar_position: 3
title: Module System
description: Oite module resolution, linking, and import/export system.
keywords: [modules, imports, exports, module resolution, linking]
---

# Module System

> **Status**: Future Implementation
>
> This document describes the planned module system for Oite.

## Overview

Oite supports ES module syntax for organizing code into reusable modules:

```javascript
// Exporting
export function greet(name) {
    return "Hello, " + name;
}

export const VERSION = "1.0.0";

// Importing
import { greet, VERSION } from "./greeting";
import * as utils from "./utils";
```

## Module Resolution

Unroll resolves modules in the following order:

1. **Relative paths** - `./module` or `../module`
2. **Absolute paths** - `/path/to/module`
3. **Package imports** - `@rolls/http` or `lodash`

### Resolution Algorithm

```
import { foo } from "bar"
         │
         ▼
┌─────────────────────┐
│  Is it a relative   │──Yes──▶ Resolve relative to current file
│  path (./  ../)     │
└─────────┬───────────┘
          │ No
          ▼
┌─────────────────────┐
│  Is it in           │──Yes──▶ Use package from unroll.lock
│  dependencies?      │
└─────────┬───────────┘
          │ No
          ▼
┌─────────────────────┐
│      Error:         │
│  Module not found   │
└─────────────────────┘
```

## File Extensions

Unroll supports the following file extensions:

| Extension | Description |
|-----------|-------------|
| `.ot` | Oite source file |
| `.nroll` | Precompiled npm package |

When importing without an extension:

```javascript
import { foo } from "./bar";
// Tries: ./bar.ot, ./bar/index.ot
```

## Package Imports

### Rolls Packages

Official Rolls packages use the `@rolls/` prefix:

```javascript
import { serve } from "@rolls/http";
import { readFile } from "@rolls/fs";
import { hash } from "@rolls/crypto";
```

### NPM Packages (via .nroll)

NPM packages are converted to `.nroll` format for use in Oite:

```javascript
import _ from "lodash";
import { v4 as uuid } from "uuid";
```

## Tree Shaking

Unroll performs dead code elimination at the module level:

```javascript
// utils.ot
export function used() { return 1; }
export function unused() { return 2; }  // Will be removed

// main.ot
import { used } from "./utils";
console.log(used());
```

Only `used()` is included in the final binary.

## Circular Dependencies

Oite handles circular dependencies through deferred initialization:

```javascript
// a.ot
import { b } from "./b";
export const a = () => b();

// b.ot
import { a } from "./a";
export const b = () => a();
```

The linker detects cycles and ensures proper initialization order.

## Future Enhancements

1. **Dynamic imports** - `await import("./module")`
2. **Import assertions** - `import data from "./data.json" assert { type: "json" }`
3. **Namespace re-exports** - `export * from "./module"`
