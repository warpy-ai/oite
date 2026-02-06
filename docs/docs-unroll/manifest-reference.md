---
sidebar_position: 5
title: Manifest Reference
description: Complete reference for the unroll.toml and roll.toml project manifest format.
keywords: [manifest, unroll.toml, roll.toml, configuration, toml]
---

# Manifest Reference

The manifest file defines your project's metadata, dependencies, and build configuration. Binary projects use `unroll.toml`, libraries use `roll.toml`.

## Binary Project (`unroll.toml`)

```toml
[package]
name = "my-app"
version = "0.1.0"
edition = "2025"
license = "MIT"
description = "My awesome application"
repository = "https://github.com/example/my-app"
keywords = ["web", "server"]
authors = ["Your Name <you@example.com>"]

[dependencies]
"@rolls/http" = "0.1"
"@rolls/json" = "0.1"

[dev-dependencies]
"@rolls/test" = "0.1"

[features]
default = ["json"]
json = ["@rolls/json"]
full = ["json", "@rolls/tls"]

[build]
target = "native"
optimization = "release"
lto = true

[profile.dev]
opt-level = 0
debug = true
incremental = true

[profile.release]
opt-level = 3
lto = "thin"

[profile.dist]
opt-level = 3
lto = "fat"
strip = "all"
```

## Library Project (`roll.toml`)

Libraries use `roll.toml` with a `[roll]` section instead of `[package]`:

```toml
[roll]
name = "@rolls/http"
version = "0.1.0"
edition = "2025"
license = "Apache-2.0"
description = "HTTP client and server for Oite"
repository = "https://github.com/warpy-ai/rolls"
keywords = ["http", "web", "server", "client"]
```

The `[roll]` section accepts the same fields as `[package]`.

## `[package]` / `[roll]` Section

### Required Fields

| Field | Type | Description |
|-------|------|-------------|
| `name` | string | Package identifier. Libraries must use `@scope/name` format. |
| `version` | string | Semantic version (`major.minor.patch`) |

### Optional Fields

| Field | Type | Description | Default |
|-------|------|-------------|---------|
| `edition` | string | Oite language edition | `"2025"` |
| `license` | string | SPDX license identifier | `""` |
| `description` | string | Short package description | `""` |
| `repository` | string | Source repository URL | `""` |
| `keywords` | string[] | Search keywords | `[]` |
| `authors` | string[] | Author names and emails | `[]` |
| `no-std` | boolean | Disable implicit `@rolls/std` dependency | `false` |

### Package Naming

- **Binary projects:** Any valid identifier (e.g., `"my-app"`, `"server"`)
- **Libraries (for registry):** Must use scoped format: `"@scope/name"` (e.g., `"@rolls/http"`)

## `[dependencies]` Section

Dependencies are key-value pairs where the key is the package name and the value is the version requirement.

```toml
[dependencies]
"@rolls/http" = "0.1"
"@rolls/json" = "*"
"@rolls/tls" = "=0.1.5"
```

### Version Requirements

| Syntax | Meaning | Example |
|--------|---------|---------|
| `"0.1"` | Caret range: `>=0.1.0, <0.2.0` | Matches `0.1.0`, `0.1.5` |
| `"1.0"` | Caret range: `>=1.0.0, <2.0.0` | Matches `1.0.0`, `1.5.0` |
| `"*"` | Any version (latest) | Matches everything |
| `"=0.1.5"` | Exact version | Only `0.1.5` |

### Caret Range Semantics

Caret ranges allow patch and minor updates that are assumed to be backwards-compatible:

- For `0.x` versions: only patch updates allowed (`0.1` matches `0.1.*` but not `0.2.0`)
- For `1.x+` versions: minor and patch updates allowed (`1.0` matches `1.*.*` but not `2.0.0`)

### Dependency Sources

Dependencies can come from different sources:

```toml
[dependencies]
# Registry (default)
"@rolls/http" = "0.1"

# Git repository (planned)
"my-lib" = { git = "https://github.com/example/my-lib.git", branch = "main" }

# Local path (planned)
"my-local-lib" = { path = "../my-local-lib" }
```

### Optional Dependencies

```toml
[dependencies]
"@rolls/tls" = { version = "0.1", optional = true }
```

Optional dependencies are only included when explicitly enabled via features.

## `[dev-dependencies]` Section

Dependencies only needed for testing and development:

```toml
[dev-dependencies]
"@rolls/test" = "0.1"
"@rolls/bench" = "0.1"
```

Dev dependencies are not included in the published package or in production builds.

## `[features]` Section

Features allow conditional compilation and optional dependencies:

```toml
[features]
default = ["json"]
json = ["@rolls/json"]
tls = ["@rolls/tls"]
full = ["json", "tls"]
```

- `default`: Features enabled by default
- Other entries: Named feature sets that can be enabled by dependents

## `[build]` Section

Build configuration:

| Field | Type | Description | Default |
|-------|------|-------------|---------|
| `target` | string | Compilation target | `"native"` |
| `optimization` | string | Optimization level | `"debug"` |
| `lto` | boolean | Enable link-time optimization | `false` |

```toml
[build]
target = "native"
optimization = "release"
lto = true
```

## `[profile.*]` Sections

Build profiles control compilation settings:

### `[profile.dev]`

```toml
[profile.dev]
opt-level = 0        # No optimization
debug = true         # Include debug info
incremental = true   # Incremental compilation
```

### `[profile.release]`

```toml
[profile.release]
opt-level = 3        # Maximum optimization
lto = "thin"         # Fast LTO
```

### `[profile.dist]`

```toml
[profile.dist]
opt-level = 3        # Maximum optimization
lto = "fat"          # Full LTO (slower build, smaller binary)
strip = "all"        # Strip all symbols
```

### Profile Fields

| Field | Type | Values | Description |
|-------|------|--------|-------------|
| `opt-level` | number | `0`, `1`, `2`, `3` | Optimization level |
| `debug` | boolean | `true`, `false` | Include debug symbols |
| `lto` | string | `"none"`, `"thin"`, `"fat"` | Link-time optimization |
| `strip` | string | `"none"`, `"all"` | Symbol stripping |
| `incremental` | boolean | `true`, `false` | Incremental compilation |

## Manifest Generated by `unroll new`

### Binary Project

```toml
[package]
name = "my-app"
version = "0.1.0"
edition = "2025"
```

### Library Project

```toml
[roll]
name = "my-lib"
version = "0.1.0"
edition = "2025"
```

## TOML Syntax Notes

The manifest uses a subset of TOML:

- **Strings:** `"value"` (double quotes)
- **Booleans:** `true` / `false`
- **Numbers:** `0`, `3`
- **Arrays:** `["item1", "item2"]`
- **Tables:** `[section]` and `[section.subsection]`
- **Comments:** Lines starting with `#`

Dependency names with special characters (like `@` and `/`) must be quoted:

```toml
[dependencies]
"@rolls/http" = "0.1"    # Correct - quoted key
```
