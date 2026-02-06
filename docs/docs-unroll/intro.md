---
sidebar_position: 1
title: Unroll - Build System
description: Documentation for Unroll, Oite's package manager, build system, and developer tooling ecosystem.
keywords: [unroll, package manager, build system, tooling, modules]
---

# Unroll - Build System & Package Manager

**Unroll** is the build system, package manager, and developer tooling ecosystem for Oite. It manages dependencies, handles compilation, provides developer experience features, and integrates with the official package registry.

## Overview

Unroll provides:

- **Package Management** - Add, update, and resolve dependencies from the registry
- **Build System** - Compile projects with multiple optimization profiles
- **Developer Tools** - Formatting, linting, type checking, and documentation generation
- **Registry Integration** - Search, publish, and manage packages on [registry.oite.org](https://registry.oite.org)
- **Self-Updating** - Built-in upgrade command to keep the toolchain current

## Architecture

```
┌─────────────────────────────────────────┐
│            User App Code                │
└──────────────────┬──────────────────────┘
                   │
┌──────────────────▼──────────────────────┐
│   Rolls (official system libs)          │
│   @rolls/http, @rolls/tls, @rolls/std  │
└──────────────────┬──────────────────────┘
                   │
┌──────────────────▼──────────────────────┐
│   Unroll (runtime + tooling)            │  <-- THIS SECTION
│   pkg manager, lockfiles, build, LSP    │
└──────────────────┬──────────────────────┘
                   │
┌──────────────────▼──────────────────────┐
│   Oite (language core)                  │
│   compiler, type system, ABI, bootstrap │
└─────────────────────────────────────────┘
```

## Quick Start

```bash
# Install the toolchain
curl -fsSL https://oite.org/install | sh

# Create a new project
unroll new my-app
cd my-app

# Add a dependency
unroll add @rolls/http

# Run the project
unroll run

# Build for release
unroll build --release
```

## Documentation

### Getting Started
- **[Installation](./installation)** - Install oitec and unroll
- **[Getting Started](./getting-started)** - Create your first project

### Guides
- **[Managing Dependencies](./dependencies)** - Add, remove, and update packages
- **[Building Projects](./building)** - Build profiles, optimization, cross-compilation
- **[Testing](./testing)** - Write and run tests
- **[Formatting & Linting](./formatting-linting)** - Code quality tools

### Registry
- **[Package Registry](./registry)** - Search and browse packages
- **[Publishing Packages](./publishing)** - Publish your packages
- **[Authentication](./authentication)** - Login and token management

### Reference
- **[CLI Reference](./cli-reference)** - Complete command reference
- **[Manifest Reference](./manifest-reference)** - `unroll.toml` / `roll.toml` format
- **[Lockfile Reference](./lockfile-reference)** - `unroll.lock` format
- **[Configuration](./configuration)** - Global settings and environment variables

### Toolchain
- **[Upgrading](./upgrade)** - Update oitec and unroll

### Architecture
- **[Design](./design)** - Architecture and design principles
- **[Module System](./modules)** - Module resolution and imports
- **[File Type Interop](./file-type-interop)** - Cross-file-type memory semantics

## Project Manifest

Binary projects use `unroll.toml`:

```toml
[package]
name = "my-app"
version = "0.1.0"
edition = "2025"

[dependencies]
"@rolls/http" = "0.1"
"@rolls/json" = "0.1"
```

Libraries use `roll.toml` with a `[roll]` section:

```toml
[roll]
name = "@yourscope/my-lib"
version = "0.1.0"
edition = "2025"
```

## CLI Commands

| Command | Description |
|---------|-------------|
| `unroll new <name>` | Create new project |
| `unroll init` | Initialize in current directory |
| `unroll add <pkg>` | Add dependency |
| `unroll remove <pkg>` | Remove dependency |
| `unroll update` | Update dependencies |
| `unroll build` | Build project |
| `unroll run` | Run project |
| `unroll test` | Run tests |
| `unroll fmt` | Format code |
| `unroll lint` | Lint code |
| `unroll check` | Type check |
| `unroll doc` | Generate docs |
| `unroll search <q>` | Search registry |
| `unroll info <pkg>` | Package info |
| `unroll publish` | Publish package |
| `unroll login` | Authenticate |
| `unroll upgrade` | Update toolchain |
