---
sidebar_position: 7
title: Configuration
description: Global configuration for unroll, including registry settings, credentials, and environment variables.
keywords: [config, configuration, registry, credentials, environment variables]
---

# Configuration

Unroll uses several configuration files and environment variables for global settings.

## Configuration Files

### `~/.unroll/config.toml`

Global configuration file for registry settings.

```toml
# Unroll package manager configuration

[registry]
default = "https://registry.oite.org/api/v1"
```

Created automatically by the installer. You can edit it to point to a different registry.

#### Fields

| Field | Description | Default |
|-------|-------------|---------|
| `default` or `registry` | Registry URL | `https://registry.oite.org/api/v1` |
| `token` | Authentication token (or env var reference) | None |

#### Token Configuration

Tokens can be specified directly or as environment variable references:

```toml
# Direct token
token = "your-token-here"

# Environment variable reference
token = "env:UNROLL_TOKEN"
```

### `~/.unroll/credentials`

Stores authentication tokens per registry URL. Created by `unroll login`.

```
https://registry.oite.org/api/v1 = "oat_abc123def456"
```

Format: One line per registry as `url = "token"`.

### `~/.unroll/last_version_check`

Cache file for the automatic update check. Contains:

```
1706745600
v0.7.0
```

Line 1: Unix timestamp of last check. Line 2: Latest version found.

This file is managed automatically. Delete it to force a version check on next run.

## Environment Variables

### Registry & Authentication

| Variable | Description | Overrides |
|----------|-------------|-----------|
| `UNROLL_TOKEN` | Registry authentication token | `~/.unroll/credentials` |
| `UNROLL_REGISTRY` | Registry URL | `~/.unroll/config.toml` |

### Toolchain

| Variable | Description | Default |
|----------|-------------|---------|
| `OITEC_PATH` | Override oitec binary location | Auto-detected |
| `OITE_HOME` | Override oite installation directory | `~/.oite` |
| `OITE_PREFIX` | Install prefix (used by installer) | `~/.oite` |
| `HOME` | User home directory | System default |

### Priority Order

When multiple sources provide the same setting, the priority is (highest first):

**Registry URL:**
1. `UNROLL_REGISTRY` environment variable
2. `~/.unroll/config.toml` `default` field
3. Built-in default: `https://registry.oite.org/api/v1`

**Authentication Token:**
1. `UNROLL_TOKEN` environment variable
2. `~/.unroll/credentials` file
3. `~/.unroll/config.toml` `token` field

### oitec Location

The oitec compiler is located by checking (in order):

1. `OITEC_PATH` environment variable
2. `oitec` in `PATH` (via `which`)
3. `/usr/local/bin/oitec`
4. `~/.oite/bin/oitec`

## CI/CD Configuration

### GitHub Actions

```yaml
name: Build
on: [push]
jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: Install Oite
        run: curl -fsSL https://oite.org/install | sh

      - name: Add to PATH
        run: echo "$HOME/.oite/bin" >> $GITHUB_PATH

      - name: Build
        run: unroll build --release

      - name: Test
        run: unroll test

      - name: Lint
        run: unroll fmt --check
```

### Publishing from CI

```yaml
      - name: Publish
        env:
          UNROLL_TOKEN: ${{ secrets.UNROLL_TOKEN }}
        run: unroll publish
```

Set `UNROLL_TOKEN` as a repository secret in your GitHub settings.

### Custom Registry

```yaml
      - name: Publish to private registry
        env:
          UNROLL_TOKEN: ${{ secrets.REGISTRY_TOKEN }}
          UNROLL_REGISTRY: https://registry.internal.company.com/api/v1
        run: unroll publish
```

## Project-Level Configuration

Project-specific settings are in `unroll.toml` (see [Manifest Reference](./manifest-reference)):

```toml
[package]
name = "my-app"
version = "0.1.0"

[build]
target = "native"
optimization = "release"
```

## Directory Structure

```
~/.oite/                    # Oite installation
├── bin/
│   ├── oitec              # Compiler/interpreter binary
│   └── unroll             # Package manager wrapper
└── lib/
    └── unroll/            # Unroll source code
        └── src/

~/.unroll/                  # Unroll configuration
├── config.toml            # Registry settings
├── credentials            # Auth tokens
└── last_version_check     # Update check cache

./                          # Project directory
├── unroll.toml            # Project manifest
├── unroll.lock            # Dependency lockfile
├── src/                   # Source code
├── tests/                 # Tests
├── examples/              # Examples
└── target/                # Build output
    ├── dev/
    ├── release/
    └── dist/
```
