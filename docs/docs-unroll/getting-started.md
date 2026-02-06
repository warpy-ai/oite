---
sidebar_position: 3
title: Getting Started
description: Create your first Oite project with unroll, add dependencies, build, and run.
keywords: [tutorial, quickstart, new project, first project, hello world]
---

# Getting Started

This guide walks you through creating your first Oite project with unroll.

## Create a New Project

```bash
unroll new hello
cd hello
```

This creates the following structure:

```
hello/
├── unroll.toml        # Project manifest
├── .gitignore         # Git ignore rules
├── src/
│   └── main.ot        # Entry point
├── tests/             # Test directory
└── examples/          # Example directory
```

### Project Manifest

The generated `unroll.toml`:

```toml
[package]
name = "hello"
version = "0.1.0"
edition = "2025"
```

### Entry Point

The generated `src/main.ot`:

```javascript
// Main entry point

function main(): void {
    console.log("Hello, Oite!");
}

main();
```

## Run Your Project

```bash
unroll run
```

Output:
```
Running hello...

Hello, Oite!
```

The `run` command finds the oitec interpreter and executes `src/main.ot` directly.

## Create a Library

To create a library instead of a binary:

```bash
unroll new my-lib --lib
```

This generates `src/lib.ot` instead of `src/main.ot`:

```javascript
// Library entry point

/// Greet a person by name
export function greet(name: string): string {
    return "Hello, " + name + "!";
}

/// Add two numbers
export function add(a: number, b: number): number {
    return a + b;
}
```

Library manifests use the `[roll]` section header instead of `[package]`:

```toml
[roll]
name = "@yourscope/my-lib"
version = "0.1.0"
edition = "2025"
```

## Add Dependencies

Add a package from the registry:

```bash
unroll add @rolls/http
```

This updates your `unroll.toml`:

```toml
[dependencies]
"@rolls/http" = "0.1"
```

And creates/updates `unroll.lock` with exact resolved versions.

### Add with Version Constraint

```bash
# Specific version range
unroll add @rolls/json@0.1

# Add as dev dependency
unroll add --dev @rolls/test

# Add with features
unroll add @rolls/http --features tls,json
```

## Use Dependencies

Import packages in your source code:

```javascript
// src/main.ot
import { serve } from "@rolls/http";

function main(): void {
    console.log("Starting server...");
    // Use the http package
}

main();
```

## Build Your Project

```bash
# Debug build (fast compilation)
unroll build

# Release build (optimized)
unroll build --release

# Distribution build (maximum optimization)
unroll build --dist
```

Build output goes to `./target/{profile}/`:

```
target/
├── dev/
│   └── hello          # Debug binary
├── release/
│   └── hello          # Optimized binary
└── dist/
    └── hello          # Distribution binary
```

## Run with Arguments

Pass arguments to your program using `--`:

```bash
unroll run -- --port 8080 --verbose
```

Your program receives `--port`, `8080`, `--verbose` as arguments.

## Project Commands Summary

| Command | Description |
|---------|-------------|
| `unroll new <name>` | Create new project |
| `unroll run` | Run the project |
| `unroll build` | Build the project |
| `unroll add <pkg>` | Add a dependency |
| `unroll test` | Run tests |
| `unroll fmt` | Format code |
| `unroll --help` | Show all commands |

## Initialize in Existing Directory

If you already have a directory:

```bash
mkdir my-project
cd my-project
unroll init
```

This creates `unroll.toml` and scaffolds the directory structure without overwriting existing files. The project name defaults to the directory name.

```bash
# Specify a different name
unroll init --name my-custom-name

# Initialize as library
unroll init --lib
```

## Standard Library

Every project implicitly depends on `@rolls/std` (the standard library) unless you disable it:

```bash
# Build without standard library
unroll build --no-std
```

Or in `unroll.toml`:

```toml
[package]
name = "my-app"
version = "0.1.0"
no-std = true
```

## Next Steps

- [Managing Dependencies](./dependencies) - Add, update, and remove packages
- [Building Projects](./building) - Build profiles, cross-compilation, and optimization
- [CLI Reference](./cli-reference) - Complete command reference
- [Publishing Packages](./publishing) - Share your code on the registry
