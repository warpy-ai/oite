---
sidebar_position: 16
title: Architecture & Design
description: Architecture and design document for Unroll, Oite's package manager, build system, and developer tooling ecosystem.
keywords: [unroll, architecture, design, internals, package manager, build system]
---

# Architecture & Design

This document describes the architecture and internal design of Unroll.

## Overview

Unroll is itself written in Oite (`.ot` files) and runs on the oitec interpreter. It manages the full lifecycle of Oite projects: creation, dependency resolution, building, testing, formatting, linting, publishing, and toolchain management.

## Source Structure

```
unroll/
├── src/
│   ├── main.ot              # Entry point, command dispatch
│   ├── cli/                  # CLI command implementations
│   │   ├── args.ot           # Argument parsing, help text
│   │   ├── new.ot            # Project creation
│   │   ├── init.ot           # Directory initialization
│   │   ├── deps.ot           # add, remove, update commands
│   │   ├── build.ot          # build, run, watch, clean
│   │   ├── test.ot           # Test runner
│   │   ├── fmt.ot            # Code formatter
│   │   ├── lint.ot           # Linter
│   │   ├── check.ot          # Type checker
│   │   ├── doc.ot            # Documentation generator
│   │   ├── registry.ot       # search, info, publish, yank, login
│   │   └── upgrade.ot        # Self-upgrade, version check
│   ├── config/               # Configuration and manifest handling
│   │   ├── manifest.ot       # TOML parser for unroll.toml / roll.toml
│   │   ├── lockfile.ot       # Lockfile parser and writer
│   │   └── scaffold.ot       # Project templates and scaffolding
│   ├── build/                # Build system
│   │   ├── compiler.ot       # Compiler interface (oitec invocation)
│   │   ├── linker.ot         # Linker interface
│   │   └── test_discovery.ot # Test file discovery
│   ├── registry/             # Registry client
│   │   ├── config.ot         # Registry configuration and credentials
│   │   └── client.ot         # HTTP client, JSON parser, API calls
│   └── resolver/             # Dependency resolution
│       └── mod.ot            # Version resolution algorithm
└── unroll.toml               # Unroll's own manifest
```

## Command Dispatch

The entry point (`main.ot`) uses a linear if/else chain to dispatch commands:

```
process.argv
    → parseArgs()
    → if "new" → runNew()
    → if "add" → runAdd()
    → if "build" → runBuild()
    → ...
    → Unknown command error
```

Before dispatch, `checkForUpdates(VERSION)` runs to notify users of available updates (cached, non-blocking).

## TOML Parser

Unroll implements its own TOML parser in `manifest.ot` since oitec doesn't have a built-in TOML library. The parser handles:

- `[section]` headers (e.g., `[package]`, `[roll]`, `[dependencies]`)
- Key-value pairs with string, number, and boolean values
- Quoted keys (required for scoped package names like `"@rolls/http"`)
- Array values
- Nested sections (e.g., `[profile.release]`)
- Comments (lines starting with `#`)

The parser tracks the current section and maps fields to the `Manifest` interface.

## Dependency Resolution

The resolver (`resolver/mod.ot`) implements a depth-first resolution algorithm:

```
resolveDependencies(manifest)
    │
    ├── Inject @rolls/std (unless no-std)
    │
    ├── For each dependency:
    │   ├── Check visited set (cycle detection)
    │   ├── Fetch versions from registry
    │   ├── Select best matching version
    │   ├── Recursively resolve transitive deps
    │   └── Add to resolved set
    │
    └── Sort alphabetically → write lockfile
```

### Version Selection

Version matching uses caret semantics:

- `"0.1"` matches `>=0.1.0, <0.2.0` (patch updates only for 0.x)
- `"1.0"` matches `>=1.0.0, <2.0.0` (minor + patch updates)
- `"*"` matches any version (picks latest)
- `"=0.1.5"` matches exactly `0.1.5`

### Cycle Detection

The resolver maintains a `visited` set of package names. If a package is encountered again during resolution, it's skipped to prevent infinite recursion.

## Registry Client

The registry client (`registry/client.ot`) communicates with the rolls-registry server via HTTP:

- **HTTP layer**: Uses `curl` via `process.exec()` for all HTTP operations
- **JSON parser**: Custom recursive-descent parser in `client.ot` (handles objects, arrays, strings, numbers, booleans, null, escape sequences)
- **Authentication**: Bearer tokens passed via `Authorization` header

### Device Authorization Flow

```
Client                      Registry                    Browser
  │                           │                           │
  ├── POST /auth/device ──────►│                           │
  │◄── device_code, user_code──┤                           │
  │                            │                           │
  ├── Open browser ────────────┼──────────────────────────►│
  │                            │          User authorizes  │
  │                            │◄──────────────────────────┤
  │                            │                           │
  ├── POST /auth/device/poll──►│                           │
  │◄── status: pending ───────┤                           │
  │    (repeat every N sec)    │                           │
  │                            │                           │
  ├── POST /auth/device/poll──►│                           │
  │◄── status: complete, token─┤                           │
  │                            │                           │
  └── Save to credentials      │                           │
```

## Build System

The build pipeline has two modes:

### Interpreted Mode (`unroll run`)

Source files are executed directly by oitec:

```
oitec src/main.ot -- [args]
```

No compilation or linking step. This is the current default for `unroll run`.

### Compiled Mode (`unroll build`)

Full compilation pipeline:

```
Source (.ot) → oitec build → Objects (.o) → Linker → Binary
```

1. **Discovery**: Recursively find all `.ot` files in `./src/`
2. **Compilation**: Invoke `oitec build <source> -o <output>` for each file
3. **Dependencies**: Compile dependencies from lockfile
4. **Linking**: Link all objects into a binary

### Profile Flags

| Profile | oitec flags | Linker flags |
|---------|------------|--------------|
| dev | `-O0 -g` | Default |
| release | `-O3 --lto=thin` | `-flto=thin` |
| dist | `-O3 --lto=fat` | `-flto -s` (strip) |

## Self-Upgrade Mechanism

The upgrade system (`upgrade.ot`) mirrors the installer script logic:

1. **Version check**: `curl` to GitHub API → extract `tag_name`
2. **Platform detection**: `uname -s` + `uname -m` → target triple
3. **Download**: `curl` + `tar` from GitHub releases
4. **Install**: Replace files in `~/.oite/`

The 24-hour cache prevents excessive API calls during normal usage.

## Error Handling Pattern

All functions return result objects instead of throwing exceptions:

```typescript
interface Result {
    success: boolean;
    error: string;
}
```

This pattern is used consistently across:
- File operations (read, write, delete)
- Network operations (HTTP requests)
- Build operations (compile, link)
- Registry operations (publish, yank)

## Key Interfaces

### Manifest

```typescript
interface Manifest {
    name: string;
    version: string;
    edition: string;
    license: string;
    description: string;
    repository: string;
    keywords: string[];
    authors: string[];
    dependencies: Dependency[];
    devDependencies: Dependency[];
    features: Feature[];
    build: BuildConfig;
    profiles: Profile[];
    noStd: boolean;
}
```

### Dependency

```typescript
interface Dependency {
    name: string;
    version: string;
    optional: boolean;
    features: string[];
    git: string;
    branch: string;
    path: string;
}
```

### LockedPackage

```typescript
interface LockedPackage {
    name: string;
    version: string;
    source: string;
    checksum: string;
    dependencies: string[];
}
```

### RegistryConfig

```typescript
interface RegistryConfig {
    url: string;
    token: string;
}
```

## Project Structure

```
my-app/
├── unroll.toml         # Project manifest
├── unroll.lock         # Lockfile (auto-generated)
├── src/
│   ├── main.ot         # Entry point (binary)
│   └── lib.ot          # Library root (library)
├── tests/
│   └── test.ot         # Integration tests
├── examples/
│   └── example.ot      # Example programs
└── target/
    ├── dev/            # Debug output
    ├── release/        # Release output
    ├── dist/           # Distribution output
    └── doc/            # Generated documentation
```

## Cross-Compilation

Supported targets:

| Target | Description |
|--------|-------------|
| `native` | Host platform (default) |
| `x86_64-unknown-linux-gnu` | Linux x64 (glibc) |
| `x86_64-unknown-linux-musl` | Linux x64 (static) |
| `aarch64-unknown-linux-gnu` | Linux ARM64 |
| `x86_64-apple-darwin` | macOS x64 |
| `aarch64-apple-darwin` | macOS ARM64 |
| `x86_64-pc-windows-msvc` | Windows x64 |
| `wasm32-unknown-unknown` | WebAssembly |
| `wasm32-wasi` | WASM + WASI |

## Future Work

- **Workspaces**: Monorepo support with shared dependencies
- **Build cache**: Incremental and distributed compilation cache
- **LSP**: Language server protocol integration for IDE support
- **Plugins**: Build-time code generation
- **Git/Path dependencies**: Resolve from git repos and local paths
- **Feature resolution**: Conditional compilation based on features
- **Coverage reports**: Code coverage during testing
