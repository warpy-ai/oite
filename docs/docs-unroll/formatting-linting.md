---
sidebar_position: 11
title: Formatting & Linting
description: Code formatting and linting tools in unroll for maintaining code quality.
keywords: [formatting, linting, fmt, lint, code style, code quality]
---

# Formatting & Linting

Unroll includes built-in tools for maintaining code quality.

## Code Formatting

### Format Code

```bash
unroll fmt
```

Automatically formats all `.ot` files in `./src/`:
- Strips trailing whitespace from all lines
- Ensures files end with a single newline

### Check Mode (CI)

```bash
unroll fmt --check
```

Reports files that need formatting and exits with code `1` if any files are unformatted. Does not modify files.

```
Formatting check failed:
  src/server.ot
  src/utils/parser.ot

2 file(s) need formatting. Run 'unroll fmt' to fix.
```

### Format Specific Files

```bash
unroll fmt src/main.ot src/server.ot
```

### Verbose Output

```bash
unroll fmt --verbose
```

Shows each file that was formatted:

```
Formatted: src/server.ot
Formatted: src/utils/parser.ot
Formatted 2 file(s)
```

### Options

| Option | Description |
|--------|-------------|
| `--check` | Check without modifying (exit 1 if unformatted) |
| `--verbose`, `-v` | Show affected files |

## Code Linting

### Run Linter

```bash
unroll lint
```

Checks all `.ot` files in `./src/` for style issues.

### Current Rules

| Rule | Description | Threshold |
|------|-------------|-----------|
| Line length | Lines should not exceed maximum length | 100 characters |

### Example Output

```
src/server.ot:42: Line exceeds 100 characters (127)
src/utils/parser.ot:88: Line exceeds 100 characters (105)

Found 2 warning(s) in 2 file(s)
```

### Auto-Fix

```bash
unroll lint --fix
```

Automatically fixes issues where possible.

### Filter by Rule

```bash
unroll lint --rule line-length
```

### Lint Specific Files

```bash
unroll lint src/main.ot src/server.ot
```

### Options

| Option | Description |
|--------|-------------|
| `--fix` | Auto-fix issues |
| `--verbose`, `-v` | Show affected files |
| `--rule <RULE>` | Check specific rule only |

## Type Checking

### Check Types

```bash
unroll check
```

Runs the type checker without producing output binaries. Reports errors and warnings.

```bash
unroll check --verbose
```

## Documentation Generation

### Generate Docs

```bash
unroll doc
```

Generates HTML documentation from source code and doc comments. Output goes to `./target/doc/`.

### Open in Browser

```bash
unroll doc --open
```

Generates docs and opens `./target/doc/index.html` in your default browser.

### Options

| Option | Description |
|--------|-------------|
| `--open` | Open in browser after generation |
| `--document-private-items` | Include private items |
| `--no-deps` | Don't document dependencies |

## CI Pipeline

A typical CI pipeline checks formatting, linting, and tests:

```yaml
name: CI
on: [push, pull_request]

jobs:
  check:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: Install Oite
        run: curl -fsSL https://oite.org/install | sh

      - name: Add to PATH
        run: echo "$HOME/.oite/bin" >> $GITHUB_PATH

      - name: Check formatting
        run: unroll fmt --check

      - name: Lint
        run: unroll lint

      - name: Type check
        run: unroll check

      - name: Test
        run: unroll test --verbose
```
