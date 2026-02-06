---
sidebar_position: 12
title: Package Registry
description: How to search, browse, and use the Oite package registry for discovering and installing packages.
keywords: [registry, packages, search, info, rolls, crates]
---

# Package Registry

The Oite package registry at `registry.oite.org` hosts packages (called "rolls") that you can use in your projects.

## Registry URL

| Environment | URL |
|-------------|-----|
| Production | `https://registry.oite.org/api/v1` |
| Custom | Set via `UNROLL_REGISTRY` env var or `~/.unroll/config.toml` |

## Searching Packages

```bash
unroll search <query>
```

### Examples

```bash
unroll search http
```

Output:

```
@rolls/http     0.1.0    HTTP client and server for Oite          (125 downloads)
@rolls/tls      0.1.0    TLS/SSL support for Oite                 (89 downloads)

Showing 2 results (page 1)
```

### Pagination

```bash
unroll search json --limit 5            # 5 results per page
unroll search json --page 2             # Page 2
unroll search json --limit 10 --page 3  # Combined
```

| Option | Description | Default |
|--------|-------------|---------|
| `--limit <N>`, `-l <N>` | Results per page | 20 |
| `--page <N>`, `-p <N>` | Page number | 1 |

## Package Information

### Overview

```bash
unroll info @rolls/http
```

Output:

```
@rolls/http 0.1.0
HTTP client and server for Oite

Homepage:    https://oite.org/rolls/http
Repository:  https://github.com/warpy-ai/rolls
License:     Apache-2.0
Created:     2026-01-15
Updated:     2026-02-01

Owners:
  warpy-ai

Dependencies:
  @rolls/async ^0.1
  @rolls/json ^0.1
  @rolls/tls ^0.1
```

### List All Versions

```bash
unroll info @rolls/http --versions
```

Output:

```
@rolls/http versions:

  0.1.0    2026-01-15
  0.1.1    2026-01-20    (yanked)
  0.1.2    2026-02-01

3 version(s)
```

### Specific Version Details

```bash
unroll info @rolls/http --version 0.1.0
# or
unroll info @rolls/http@0.1.0
```

Shows version-specific metadata including checksum, download URL, dependencies, and yanked status.

## Official Packages (@rolls)

The `@rolls` scope contains the official standard library packages:

### Core Libraries (No Dependencies)

| Package | Description |
|---------|-------------|
| `@rolls/array` | Array utilities and operations |
| `@rolls/string` | String manipulation functions |
| `@rolls/math` | Mathematical functions |
| `@rolls/path` | File path utilities |
| `@rolls/date` | Date and time utilities |
| `@rolls/async` | Async primitives and utilities |
| `@rolls/crypto` | Cryptographic functions |

### Composed Libraries

| Package | Dependencies | Description |
|---------|-------------|-------------|
| `@rolls/json` | `@rolls/string` | JSON parsing and serialization |
| `@rolls/fs` | `@rolls/path`, `@rolls/async` | File system operations |
| `@rolls/tls` | `@rolls/async` | TLS/SSL support |
| `@rolls/http` | `@rolls/async`, `@rolls/json`, `@rolls/tls` | HTTP client and server |
| `@rolls/std` | `@rolls/string`, `@rolls/array`, `@rolls/math`, `@rolls/json` | Standard library (implicit) |
| `@rolls/websocket` | `@rolls/async`, `@rolls/http`, `@rolls/crypto`, `@rolls/tls` | WebSocket protocol |

### Dependency Graph

```
@rolls/websocket
├── @rolls/async
├── @rolls/http
│   ├── @rolls/async
│   ├── @rolls/json
│   │   └── @rolls/string
│   └── @rolls/tls
│       └── @rolls/async
├── @rolls/crypto
└── @rolls/tls
    └── @rolls/async

@rolls/std (implicit)
├── @rolls/string
├── @rolls/array
├── @rolls/math
└── @rolls/json
    └── @rolls/string
```

## Registry API

The registry exposes a REST API:

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/crates?q=<query>` | GET | Search packages |
| `/crates/<name>` | GET | Package info |
| `/crates/<name>/<version>` | GET | Version details |
| `/crates/new` | POST | Publish package (auth required) |
| `/crates/<name>/<version>/yank` | PUT | Yank version (auth required) |
| `/crates/<name>/<version>/yank` | DELETE | Unyank version (auth required) |
| `/me` | GET | Current user info (auth required) |
| `/auth/device` | POST | Start device auth flow |
| `/auth/device/poll` | POST | Poll device auth status |

Package names in URLs are URL-encoded: `@rolls/http` becomes `rolls%2Fhttp` (without the `@`).

## Git Index

The registry maintains a sparse git index at [github.com/warpy-ai/rolls-index](https://github.com/warpy-ai/rolls-index) for efficient dependency resolution. Each package has an entry file containing NDJSON with version metadata.

Index path structure:
- 1-2 char names: `1/` or `2/` prefix
- 3 char names: `3/{first-char}/`
- 4+ char names: `{first-two}/{next-two}/`

Example: `@rolls/array` (indexed as `rolls_array`) → `ro/ll/rolls_array`
