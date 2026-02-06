---
sidebar_position: 4
title: CLI Reference
description: Complete reference for all unroll CLI commands, flags, and options.
keywords: [cli, commands, reference, flags, options]
---

# CLI Reference

Complete reference for all `unroll` commands.

## Global Options

```bash
unroll [OPTIONS] <COMMAND> [ARGS]
```

| Option | Description |
|--------|-------------|
| `-h`, `--help` | Show help message |
| `-V`, `--version` | Show version |

## Project Management

### `unroll new`

Create a new project.

```bash
unroll new <name> [OPTIONS]
```

| Option | Description | Default |
|--------|-------------|---------|
| `--lib` | Create a library project | Binary project |
| `--path <DIR>` | Create in specified directory | `./<name>` |

**Examples:**

```bash
unroll new my-app                # Binary project
unroll new my-lib --lib          # Library project
unroll new my-app --path /tmp    # Custom path
```

**Generated files:**
- `unroll.toml` (or with `[roll]` for `--lib`)
- `src/main.ot` (or `src/lib.ot` for `--lib`)
- `tests/` directory
- `examples/` directory
- `.gitignore`

---

### `unroll init`

Initialize a project in the current directory.

```bash
unroll init [OPTIONS]
```

| Option | Description | Default |
|--------|-------------|---------|
| `--lib` | Initialize as library | Binary project |
| `--name <NAME>` | Set project name | Directory name |

**Examples:**

```bash
cd my-project
unroll init                      # Use directory name
unroll init --lib                # Library project
unroll init --name custom-name   # Custom name
```

Does not overwrite existing files.

---

## Dependency Management

### `unroll add`

Add one or more dependencies.

```bash
unroll add <PACKAGE>[@VERSION] [OPTIONS]
```

| Option | Description | Default |
|--------|-------------|---------|
| `--dev`, `-D` | Add as dev dependency | Regular dependency |
| `--optional` | Mark as optional | Required |
| `--features <LIST>` | Enable features (comma-separated) | None |

**Version specifiers:**

| Format | Meaning |
|--------|---------|
| `@rolls/http` | Latest compatible version (`*`) |
| `@rolls/http@0.1` | Caret range: `>=0.1.0, <0.2.0` |
| `@rolls/http@=0.1.5` | Exact version `0.1.5` |

**Examples:**

```bash
unroll add @rolls/http                     # Latest
unroll add @rolls/http@0.1                 # Caret range
unroll add @rolls/json @rolls/crypto       # Multiple packages
unroll add --dev @rolls/test               # Dev dependency
unroll add @rolls/http --features tls,json # With features
unroll add --optional @rolls/tls           # Optional
```

Updates `unroll.toml` and `unroll.lock`.

---

### `unroll remove`

Remove dependencies.

```bash
unroll remove <PACKAGE>...
```

**Examples:**

```bash
unroll remove @rolls/http
unroll remove @rolls/http @rolls/json    # Multiple
```

Removes from both regular and dev dependencies. Updates `unroll.toml` and `unroll.lock`.

---

### `unroll update`

Update dependencies to latest compatible versions.

```bash
unroll update [PACKAGE]... [OPTIONS]
```

| Option | Description | Default |
|--------|-------------|---------|
| `--all` | Update all dependencies | All (when no packages specified) |

**Examples:**

```bash
unroll update                    # Update all
unroll update @rolls/http        # Update specific
unroll update @rolls/http @rolls/json  # Update multiple
```

Re-resolves versions and updates `unroll.lock`.

---

## Build & Run

### `unroll build`

Compile the project.

```bash
unroll build [OPTIONS]
```

| Option | Description | Default |
|--------|-------------|---------|
| `--release`, `-r` | Release build (`-O3`, ThinLTO) | Debug build |
| `--dist` | Distribution build (`-O3`, FullLTO, stripped) | Debug build |
| `--target <TARGET>` | Cross-compilation target | `native` |
| `--jobs <N>`, `-j <N>` | Parallel compilation jobs | Auto |
| `--verbose`, `-v` | Show detailed compilation output | Quiet |
| `--no-std` | Disable implicit `@rolls/std` | Enabled |

**Build profiles:**

| Profile | Optimization | Debug | LTO | Use Case |
|---------|-------------|-------|-----|----------|
| `dev` | `-O0` | Yes | None | Development |
| `release` | `-O3` | No | Thin | Production |
| `dist` | `-O3` | No | Fat | Distribution |

**Examples:**

```bash
unroll build                              # Debug
unroll build --release                    # Release
unroll build --dist                       # Distribution
unroll build --target wasm32              # WebAssembly
unroll build --verbose --jobs 4           # Verbose, 4 jobs
unroll build --no-std                     # No standard library
```

**Output:** `./target/{profile}/{name}`

---

### `unroll run`

Build and execute the project.

```bash
unroll run [OPTIONS] [-- <ARGS>...]
```

| Option | Description | Default |
|--------|-------------|---------|
| `--release`, `-r` | Run release build | Debug |

Arguments after `--` are passed to your program.

**Examples:**

```bash
unroll run                               # Run debug
unroll run --release                     # Run release
unroll run -- --port 8080 --verbose      # Pass args
unroll run -- arg1 arg2                  # Multiple args
```

Finds `src/main.ot` and executes it through the oitec interpreter.

---

### `unroll watch`

Watch for file changes and rebuild.

```bash
unroll watch [OPTIONS]
```

| Option | Description | Default |
|--------|-------------|---------|
| `--release`, `-r` | Use release build | Debug |
| `--run` | Also execute after rebuild | Build only |

**Examples:**

```bash
unroll watch                             # Rebuild on change
unroll watch --release                   # Release rebuilds
unroll watch --run                       # Rebuild and run
```

Monitors the `./src` directory for changes. Press `Ctrl+C` to stop.

---

### `unroll clean`

Remove build artifacts.

```bash
unroll clean
```

Deletes the entire `./target` directory.

---

## Testing

### `unroll test`

Discover and run tests.

```bash
unroll test [OPTIONS]
```

| Option | Description | Default |
|--------|-------------|---------|
| `--filter <PATTERN>` | Filter tests by glob pattern | All tests |
| `--coverage` | Generate coverage report | No coverage |
| `--verbose`, `-v` | Show individual test results | Summary only |
| `--nocapture` | Show stdout/stderr from tests | Captured |
| `--jobs <N>`, `-j <N>` | Parallel test execution | Sequential |

**Test discovery:**
- All `.ot` files in `./tests/` directory
- Files matching `*_test.ot` or `*.test.ot` in `./src/`

**Filter patterns:**

| Pattern | Matches |
|---------|---------|
| `http*` | `http_client`, `http_server` |
| `*_key` | `api_key`, `session_key` |
| `auth` | `auth` (exact match) |

**Examples:**

```bash
unroll test                              # All tests
unroll test --filter "http*"             # HTTP tests
unroll test --verbose                    # Detailed output
unroll test --nocapture --filter auth    # See output from auth tests
unroll test --jobs 4                     # Parallel execution
```

---

## Developer Tools

### `unroll fmt`

Format source code.

```bash
unroll fmt [OPTIONS] [FILES...]
```

| Option | Description | Default |
|--------|-------------|---------|
| `--check` | Check formatting without modifying | Modifies files |
| `--verbose`, `-v` | Show affected files | Quiet |

**Formatting rules:**
- Strips trailing whitespace
- Ensures files end with a newline

**Examples:**

```bash
unroll fmt                               # Format all in ./src
unroll fmt --check                       # CI mode (fails if unformatted)
unroll fmt --verbose                     # Show affected files
unroll fmt src/main.ot src/lib.ot        # Specific files
```

Auto-discovers `.ot` files in `./src` when no files are specified.

---

### `unroll lint`

Check code for style issues.

```bash
unroll lint [OPTIONS] [FILES...]
```

| Option | Description | Default |
|--------|-------------|---------|
| `--fix` | Auto-fix issues | Report only |
| `--verbose`, `-v` | Show affected files | Quiet |
| `--rule <RULE>` | Check specific rule only | All rules |

**Current rules:**

| Rule | Description |
|------|-------------|
| Line length | Lines must not exceed 100 characters |

**Examples:**

```bash
unroll lint                              # Lint all in ./src
unroll lint --fix                        # Auto-fix issues
unroll lint --rule line-length           # Specific rule
unroll lint src/main.ot                  # Specific file
```

---

### `unroll check`

Type check without building.

```bash
unroll check [OPTIONS]
```

| Option | Description | Default |
|--------|-------------|---------|
| `--verbose`, `-v` | Show detailed diagnostics | Summary |

Reports type errors and warnings without producing output binaries.

---

### `unroll doc`

Generate HTML documentation from source code and comments.

```bash
unroll doc [OPTIONS]
```

| Option | Description | Default |
|--------|-------------|---------|
| `--open` | Open docs in browser after generation | Don't open |
| `--document-private-items` | Include private items | Public only |
| `--no-deps` | Don't document dependencies | Include deps |

**Output:** `./target/doc/index.html`

**Examples:**

```bash
unroll doc                               # Generate docs
unroll doc --open                        # Generate and open
unroll doc --document-private-items      # Include private items
```

---

## Registry

### `unroll search`

Search the package registry.

```bash
unroll search <QUERY> [OPTIONS]
```

| Option | Description | Default |
|--------|-------------|---------|
| `--limit <N>`, `-l <N>` | Results per page | 20 |
| `--page <N>`, `-p <N>` | Page number | 1 |

**Examples:**

```bash
unroll search http                       # Search for HTTP packages
unroll search json --limit 5             # First 5 results
unroll search crypto --page 2            # Page 2
```

**Output includes:** Package name, latest version, description, download count.

---

### `unroll info`

Show package information.

```bash
unroll info <PACKAGE>[@VERSION] [OPTIONS]
```

| Option | Description | Default |
|--------|-------------|---------|
| `--versions`, `-v` | List all available versions | Overview only |
| `--version <VER>` | Show specific version details | Latest |

**Examples:**

```bash
unroll info @rolls/http                  # Package overview
unroll info @rolls/http --versions       # All versions
unroll info @rolls/http --version 0.1.0  # Specific version
unroll info @rolls/http@0.1.0            # Same as above
```

**Output includes:** Name, version, description, homepage, repository, owners, dependencies.

---

### `unroll publish`

Publish a package to the registry.

```bash
unroll publish [OPTIONS]
```

| Option | Description | Default |
|--------|-------------|---------|
| `--dry-run`, `-n` | Create tarball without uploading | Upload |

Requires authentication (run `unroll login` first). Package name must use `@scope/name` format.

**Examples:**

```bash
unroll publish                           # Publish package
unroll publish --dry-run                 # Test without uploading
```

**Process:**
1. Reads `roll.toml` (library) or `unroll.toml` (project)
2. Creates `.crate` tarball with source files
3. Uploads to registry with authentication token

---

### `unroll yank`

Mark a version as yanked (deprecated) or undo a yank.

```bash
unroll yank <PACKAGE>@<VERSION> [OPTIONS]
```

| Option | Description | Default |
|--------|-------------|---------|
| `--undo` | Restore a yanked version | Yank |

**Examples:**

```bash
unroll yank @rolls/http@0.1.0            # Yank version
unroll yank @rolls/http@0.1.0 --undo     # Restore version
```

Yanked versions won't be selected during dependency resolution but remain downloadable for existing lockfiles.

---

### `unroll login`

Authenticate with the package registry.

```bash
unroll login
```

Uses device authorization flow:
1. Opens your browser to the authorization page
2. You authorize the device
3. Token is saved to `~/.unroll/credentials`

Falls back to manual token entry if device auth is unavailable.

---

### `unroll logout`

Clear stored authentication credentials.

```bash
unroll logout
```

Removes the token from `~/.unroll/credentials`.

---

## Toolchain

### `unroll upgrade`

Update the oitec compiler and unroll package manager to the latest version.

```bash
unroll upgrade [OPTIONS]
```

| Option | Description | Default |
|--------|-------------|---------|
| `--force`, `-f` | Force reinstall even if up to date | Skip if current |

**Examples:**

```bash
unroll upgrade                           # Update to latest
unroll upgrade --force                   # Force reinstall
```

Downloads from GitHub releases and replaces binaries in `~/.oite/`.

See [Upgrading the Toolchain](./upgrade) for details.

---

## Exit Codes

| Code | Meaning |
|------|---------|
| `0` | Success |
| `1` | Error (compilation failure, missing files, network error, etc.) |
