---
sidebar_position: 8
title: Managing Dependencies
description: How to add, remove, update, and manage package dependencies in Oite projects.
keywords: [dependencies, packages, add, remove, update, version resolution]
---

# Managing Dependencies

Unroll manages your project's dependencies through the manifest file (`unroll.toml`) and the lockfile (`unroll.lock`).

## Adding Dependencies

### Basic Usage

```bash
unroll add @rolls/http
```

This:
1. Queries the registry for available versions
2. Selects the latest compatible version
3. Adds the entry to `unroll.toml`
4. Resolves the full dependency tree
5. Writes `unroll.lock` with exact versions

### Version Constraints

```bash
# Latest compatible version
unroll add @rolls/http

# Caret range (most common)
unroll add @rolls/http@0.1

# Exact version
unroll add @rolls/http@=0.1.5
```

### Multiple Packages

```bash
unroll add @rolls/http @rolls/json @rolls/crypto
```

### Dev Dependencies

Dependencies only needed for testing:

```bash
unroll add --dev @rolls/test
```

These go in `[dev-dependencies]` and are not included in published packages.

### Optional Dependencies

```bash
unroll add --optional @rolls/tls
```

Optional dependencies are only included when a dependent package enables the corresponding feature.

### Features

```bash
unroll add @rolls/http --features tls,json
```

## Removing Dependencies

```bash
unroll remove @rolls/http
```

Removes from both `[dependencies]` and `[dev-dependencies]`, then re-resolves the dependency tree.

```bash
# Remove multiple
unroll remove @rolls/http @rolls/json
```

## Updating Dependencies

### Update All

```bash
unroll update
```

Re-resolves all dependencies to the latest versions that satisfy the constraints in `unroll.toml`.

### Update Specific Packages

```bash
unroll update @rolls/http
unroll update @rolls/http @rolls/json
```

## Using Dependencies

Once added, import packages in your source code:

```javascript
import { serve } from "@rolls/http";
import { parse } from "@rolls/json";
import { hash } from "@rolls/crypto";

function main(): void {
    let data = parse('{"key": "value"}');
    console.log(data);
}

main();
```

## Version Resolution

### Caret Ranges

The default version strategy is caret ranges, which allow backwards-compatible updates:

| Requirement | Matches | Does Not Match |
|-------------|---------|----------------|
| `"0.1"` | `0.1.0`, `0.1.1`, `0.1.99` | `0.2.0`, `1.0.0` |
| `"1.0"` | `1.0.0`, `1.5.0`, `1.99.0` | `2.0.0` |
| `"*"` | Any version | Nothing |
| `"=0.1.5"` | `0.1.5` only | Everything else |

### Resolution Algorithm

1. **Collect** all direct dependencies from `unroll.toml`
2. **Inject** `@rolls/std` implicitly (unless `no-std`)
3. **For each dependency:**
   - Fetch available versions from registry
   - Select best matching version
   - Recursively resolve transitive dependencies
4. **Detect cycles** - prevents circular dependency resolution
5. **Sort alphabetically** - deterministic lockfile output

### Implicit Standard Library

Every project implicitly depends on `@rolls/std` unless disabled:

```toml
# In unroll.toml
[package]
name = "my-app"
no-std = true
```

Or via command line:

```bash
unroll build --no-std
```

The standard library (`@rolls/std`) transitively includes:
- `@rolls/string` - String utilities
- `@rolls/array` - Array utilities
- `@rolls/math` - Math functions
- `@rolls/json` - JSON parsing

## Dependency Sources

### Registry (Default)

```toml
[dependencies]
"@rolls/http" = "0.1"
```

Resolved from the configured registry (default: `https://registry.oite.org/api/v1`).

### Git (Planned)

```toml
[dependencies]
"my-lib" = { git = "https://github.com/example/my-lib.git" }
"my-lib" = { git = "https://github.com/example/my-lib.git", branch = "develop" }
```

### Path (Planned)

```toml
[dependencies]
"my-local-lib" = { path = "../my-local-lib" }
```

Useful for local development and monorepos.

## Lockfile

The `unroll.lock` file records exact versions for reproducible builds. See [Lockfile Reference](./lockfile-reference) for format details.

### When to Commit

| Project Type | Commit `unroll.lock`? | Reason |
|--------------|----------------------|--------|
| Application | Yes | Ensures all environments use identical versions |
| Library | No | Allows consumers to resolve compatible versions |

## Inspecting Dependencies

### Search Registry

```bash
unroll search http
```

### View Package Info

```bash
unroll info @rolls/http
unroll info @rolls/http --versions
```

### View Specific Version

```bash
unroll info @rolls/http --version 0.1.0
```

## Troubleshooting

### "Error resolving dependencies"

This usually means a version constraint cannot be satisfied. Check that:
- The package name is spelled correctly (including `@scope/` prefix)
- The version range matches an available release
- You have internet access to reach the registry

### Cycle Detection

If resolution encounters a circular dependency, it stops with an error. Circular dependencies between packages are not supported.

### Stale Lockfile

If `unroll.lock` seems out of date:

```bash
# Force re-resolve everything
unroll update
```
