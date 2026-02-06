---
sidebar_position: 15
title: Upgrading the Toolchain
description: How to update oitec and unroll to the latest version using the built-in upgrade command.
keywords: [upgrade, update, version, toolchain, self-update]
---

# Upgrading the Toolchain

Keep your Oite toolchain up to date with the built-in upgrade command.

## Automatic Version Check

Every time you run an unroll command, it checks for available updates (at most once every 24 hours). If a newer version exists, you'll see:

```
A new version of Oite is available: v0.8.0 (current: 0.7.0). Run 'unroll upgrade' to update.
```

This check is:
- **Cached** - Only queries GitHub once per 24 hours
- **Silent on failure** - Never blocks execution if the network is unavailable
- **Non-blocking** - Shown before your command runs, doesn't affect the result

### Cache Location

The version check result is cached at `~/.unroll/last_version_check`:

```
1706745600
v0.8.0
```

Delete this file to force an immediate check:

```bash
rm ~/.unroll/last_version_check
```

## Upgrade Command

```bash
unroll upgrade
```

### What It Does

1. **Detects your platform** - Uses `uname` to determine OS and architecture
2. **Fetches latest version** - Queries the GitHub releases API
3. **Compares versions** - Shows current vs latest
4. **Downloads oitec** - New compiler binary from GitHub releases
5. **Downloads unroll** - New package manager source from GitHub releases
6. **Verifies** - Runs `oitec --version` to confirm

### Example Output

```
Checking for updates...
Current oitec: oitec 0.7.0
Latest release: v0.8.0

Platform: aarch64-apple-darwin

Downloading oitec v0.8.0...
######################################## 100%
Extracting oitec...
Installed oitec to /Users/you/.oite/bin/oitec

Downloading unroll...
######################################## 100%
Installed unroll to /Users/you/.oite/lib/unroll

Verifying installation...
oitec: oitec 0.8.0

Upgrade complete!
```

### Already Up to Date

```
Checking for updates...
Current oitec: oitec 0.8.0
Latest release: v0.8.0

Already up to date!
```

### Force Reinstall

```bash
unroll upgrade --force
```

Reinstalls even if the current version matches the latest. Useful for repairing a corrupted installation.

## What Gets Updated

| Component | Location | Source |
|-----------|----------|--------|
| oitec binary | `~/.oite/bin/oitec` | GitHub release: `oitec-{target}.tar.gz` |
| unroll source | `~/.oite/lib/unroll/` | GitHub release: `unroll-src.tar.gz` or git clone |

The wrapper script at `~/.oite/bin/unroll` is not modified (it delegates to oitec + unroll source).

## Supported Platforms

| Platform | Target Triple |
|----------|---------------|
| macOS Apple Silicon | `aarch64-apple-darwin` |
| macOS Intel | `x86_64-apple-darwin` |
| Linux x86_64 | `x86_64-unknown-linux-gnu` |
| Linux ARM64 | `aarch64-unknown-linux-gnu` |

## Version Source

Versions are pulled from the [GitHub releases](https://github.com/warpy-ai/oite/releases) of the oite repository. The `tag_name` field of the latest release is used as the version identifier.

## Fallback: Unroll Source

If the release tarball (`unroll-src.tar.gz`) is not available, the upgrade command falls back to:

```bash
git clone --depth 1 https://github.com/warpy-ai/unroll.git
```

This ensures upgrades work even if the release doesn't include the unroll source bundle.

## Troubleshooting

### "Error: Could not fetch latest version from GitHub"

Check your internet connection. The upgrade command needs to reach `api.github.com`.

### "Error: Could not detect platform"

The upgrade command uses `uname -s` and `uname -m` to detect your platform. This should work on all macOS and Linux systems.

### "Error: Failed to download oitec"

The release may not have a binary for your platform. Check the [releases page](https://github.com/warpy-ai/oite/releases) for available artifacts.

### Permissions

The upgrade command overwrites files in `~/.oite/`. If you installed to a different prefix (e.g., `/usr/local`), you may need elevated permissions.

## Manual Upgrade

If the upgrade command doesn't work, you can manually upgrade by re-running the installer:

```bash
curl -fsSL https://oite.org/install | sh
```

Or install a specific version:

```bash
curl -fsSL https://oite.org/install | sh -s -- --version=v0.8.0
```
