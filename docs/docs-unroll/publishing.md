---
sidebar_position: 13
title: Publishing Packages
description: How to publish Oite packages to the registry, including manifest setup, authentication, and versioning.
keywords: [publishing, publish, registry, packages, roll.toml, tarball]
---

# Publishing Packages

Publish your Oite packages to the registry to share them with the community.

## Prerequisites

1. **Authentication** - Run `unroll login` first (see [Authentication](./authentication))
2. **Library manifest** - Your package needs a `roll.toml` with `[roll]` section
3. **Scoped name** - Package name must use `@scope/name` format

## Library Setup

### Create a Library

```bash
unroll new my-lib --lib
cd my-lib
```

### Configure `roll.toml`

```toml
[roll]
name = "@yourscope/my-lib"
version = "0.1.0"
edition = "2025"
license = "MIT"
description = "A useful library for Oite"
repository = "https://github.com/you/my-lib"
keywords = ["utility", "helpers"]

[dependencies]
"@rolls/string" = "0.1"
```

### Required Fields

| Field | Description |
|-------|-------------|
| `name` | Must be `@scope/name` format (e.g., `"@myorg/utils"`) |
| `version` | Semantic version (e.g., `"0.1.0"`) |

### Recommended Fields

| Field | Description |
|-------|-------------|
| `description` | Shown in search results |
| `license` | SPDX license identifier |
| `repository` | Source code URL |
| `keywords` | Help users find your package |

## Publishing

### Publish

```bash
unroll publish
```

This:
1. Reads `roll.toml` (or `unroll.toml`)
2. Validates the authentication token
3. Validates the package name format (`@scope/name`)
4. Creates a `.crate` tarball containing:
   - `roll.toml` (manifest)
   - `src/` directory (all source files)
5. Uploads to the registry
6. Registry validates, indexes, and makes the package available

### Dry Run

Test the publish process without uploading:

```bash
unroll publish --dry-run
```

Creates the tarball locally so you can inspect it, but does not upload.

## Package Structure

When published, your package tarball contains:

```
@yourscope/my-lib-0.1.0/
├── roll.toml
└── src/
    ├── lib.ot
    ├── utils.ot
    └── parser/
        ├── json.ot
        └── xml.ot
```

## Versioning

### Semantic Versioning

Packages use semantic versioning: `MAJOR.MINOR.PATCH`

| Change | When |
|--------|------|
| Major (1.0.0 → 2.0.0) | Breaking API changes |
| Minor (0.1.0 → 0.2.0) | New features, backwards-compatible |
| Patch (0.1.0 → 0.1.1) | Bug fixes, backwards-compatible |

### Publishing New Versions

1. Update the `version` field in `roll.toml`
2. Run `unroll publish`

```toml
[roll]
name = "@yourscope/my-lib"
version = "0.2.0"    # Bumped from 0.1.0
```

Each version can only be published once. You cannot overwrite an existing version.

## Yanking

Mark a version as deprecated (yanked):

```bash
unroll yank @yourscope/my-lib@0.1.0
```

Yanked versions:
- Won't be selected during new dependency resolution
- Remain downloadable for existing lockfiles
- Appear with a "yanked" label in `unroll info --versions`

### Undo a Yank

```bash
unroll yank @yourscope/my-lib@0.1.0 --undo
```

Restores the version to normal status.

## Publishing from CI

### GitHub Actions

```yaml
name: Publish
on:
  push:
    tags:
      - 'v*'

jobs:
  publish:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: Install Oite
        run: curl -fsSL https://oite.org/install | sh

      - name: Add to PATH
        run: echo "$HOME/.oite/bin" >> $GITHUB_PATH

      - name: Publish
        env:
          UNROLL_TOKEN: ${{ secrets.UNROLL_TOKEN }}
        run: unroll publish
```

Set `UNROLL_TOKEN` as a repository secret.

### Custom Registry

```yaml
      - name: Publish to private registry
        env:
          UNROLL_TOKEN: ${{ secrets.REGISTRY_TOKEN }}
          UNROLL_REGISTRY: https://registry.internal.company.com/api/v1
        run: unroll publish
```

## Troubleshooting

### "Error: no authentication token"

Run `unroll login` or set the `UNROLL_TOKEN` environment variable.

### "Error: package name must use @scope/name format"

Update the `name` field in `roll.toml`:

```toml
# Wrong
name = "my-lib"

# Correct
name = "@myscope/my-lib"
```

### "Error: version already exists"

You cannot publish the same version twice. Bump the version in `roll.toml`:

```toml
version = "0.1.1"  # Bump from 0.1.0
```

### "Error: could not find roll.toml"

Make sure you're in the project root directory and the manifest file exists. Libraries use `roll.toml`, applications use `unroll.toml`.
