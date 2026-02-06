---
sidebar_position: 2
title: Installation
description: How to install the Oite toolchain including oitec compiler and unroll package manager.
keywords: [install, setup, oitec, unroll, toolchain]
---

# Installation

Install the complete Oite toolchain (oitec compiler + unroll package manager) with a single command.

## Quick Install

```bash
curl -fsSL https://oite.org/install | sh
```

This installs:
- **oitec** - The Oite compiler and interpreter
- **unroll** - The package manager and build system
- **Registry config** - Pre-configured to use the official package registry

## Requirements

The installer requires the following tools to be available on your system:

| Tool | Purpose |
|------|---------|
| `curl` | Downloading binaries and sources |
| `tar` | Extracting archives |
| `git` | Fallback source download |

## Supported Platforms

| Platform | Architecture | Target Triple |
|----------|-------------|---------------|
| macOS | Apple Silicon (M1/M2/M3/M4) | `aarch64-apple-darwin` |
| macOS | Intel x86_64 | `x86_64-apple-darwin` |
| Linux | x86_64 | `x86_64-unknown-linux-gnu` |
| Linux | ARM64 | `aarch64-unknown-linux-gnu` |

The installer automatically detects your platform and downloads the correct binary.

## Installer Options

```bash
sh install.sh [OPTIONS]
```

| Option | Description | Default |
|--------|-------------|---------|
| `--prefix=DIR` | Installation directory | `~/.oite` |
| `--version=TAG` | Install specific release version | `latest` |
| `--no-modify-path` | Don't add to shell PATH | Modifies PATH |
| `--help` | Show help message | |

### Examples

```bash
# Install latest version
curl -fsSL https://oite.org/install | sh

# Install specific version
curl -fsSL https://oite.org/install | sh -s -- --version=v0.7.0

# Install to custom directory
curl -fsSL https://oite.org/install | sh -s -- --prefix=/opt/oite

# Install without modifying shell profile
curl -fsSL https://oite.org/install | sh -s -- --no-modify-path
```

## Installation Layout

After installation, your filesystem will have:

```
~/.oite/
├── bin/
│   ├── oitec          # Oite compiler/interpreter binary
│   └── unroll         # Package manager wrapper script
└── lib/
    └── unroll/        # Unroll source code
        └── src/
            ├── main.ot
            ├── cli/
            ├── config/
            ├── build/
            ├── registry/
            └── resolver/

~/.unroll/
├── config.toml        # Registry configuration
└── credentials        # Authentication tokens (after login)
```

### How Unroll Runs

The `unroll` binary at `~/.oite/bin/unroll` is a wrapper script that invokes:

```bash
exec "$HOME/.oite/bin/oitec" "$HOME/.oite/lib/unroll/src/main.ot" -- "$@"
```

This means unroll is itself an Oite program, interpreted by oitec.

## PATH Configuration

The installer automatically adds `~/.oite/bin` to your PATH by modifying your shell configuration file:

| Shell | File Modified |
|-------|--------------|
| zsh | `~/.zshrc` |
| bash | `~/.bash_profile` or `~/.bashrc` or `~/.profile` |
| fish | `~/.config/fish/conf.d/oite.fish` |

After installation, restart your shell or run:

```bash
export PATH="$HOME/.oite/bin:$PATH"
```

## Verify Installation

```bash
# Check oitec
oitec --version

# Check unroll
unroll --version

# Check help
unroll --help
```

## Environment Variables

| Variable | Description | Default |
|----------|-------------|---------|
| `OITE_PREFIX` | Installation prefix | `~/.oite` |
| `OITE_VERSION` | Version to install | `latest` |
| `OITE_HOME` | Runtime home directory | `~/.oite` |
| `OITEC_PATH` | Override oitec binary location | Auto-detected |

## Updating

After installation, use unroll itself to update the toolchain:

```bash
unroll upgrade
```

See [Upgrading the Toolchain](./upgrade) for details.

## Uninstalling

To completely remove Oite:

```bash
# Remove installation
rm -rf ~/.oite

# Remove configuration
rm -rf ~/.unroll

# Remove PATH entry from your shell profile
# Edit ~/.zshrc, ~/.bashrc, or ~/.bash_profile
# and remove the line: export PATH="$HOME/.oite/bin:$PATH"
```

## Troubleshooting

### "command not found: oitec"

Your PATH doesn't include `~/.oite/bin`. Either restart your terminal or run:

```bash
export PATH="$HOME/.oite/bin:$PATH"
```

### "Failed to download" errors

Check your internet connection. If behind a proxy, ensure `curl` is configured to use it:

```bash
export HTTPS_PROXY=http://proxy.example.com:8080
curl -fsSL https://oite.org/install | sh
```

### "Unsupported OS/architecture"

Oite currently supports macOS and Linux on x86_64 and ARM64. Windows support is planned for a future release.

### Permission denied

If installing to a system directory, you may need elevated permissions:

```bash
curl -fsSL https://oite.org/install | sudo sh -s -- --prefix=/usr/local
```

The default `~/.oite` prefix should not require elevated permissions.
