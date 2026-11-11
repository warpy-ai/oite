#!/usr/bin/env sh
# install.sh - Install Oite (oitec + unroll + the core rolls + registry config)
# Usage: curl -fsSL https://oite.org/install | sh
#    or: sh install.sh [--prefix DIR] [--no-modify-path]
set -eu

OITE_VERSION="${OITE_VERSION:-latest}"
GITHUB_REPO="warpy-ai/oite"
UNROLL_REPO="warpy-ai/unroll"
REGISTRY_URL="https://registry.oite.org/api/v1"
PREFIX="${OITE_PREFIX:-$HOME/.oite}"
MODIFY_PATH=1

# Colors (only if terminal supports them)
if [ -t 1 ]; then
    BOLD='\033[1m'
    GREEN='\033[0;32m'
    YELLOW='\033[0;33m'
    RED='\033[0;31m'
    RESET='\033[0m'
else
    BOLD=''
    GREEN=''
    YELLOW=''
    RED=''
    RESET=''
fi

info() { printf "${GREEN}info${RESET}: %s\n" "$1"; }
warn() { printf "${YELLOW}warn${RESET}: %s\n" "$1"; }
err() { printf "${RED}error${RESET}: %s\n" "$1" >&2; exit 1; }

# Parse args
for arg in "$@"; do
    case "$arg" in
        --prefix=*) PREFIX="${arg#*=}" ;;
        --no-modify-path) MODIFY_PATH=0 ;;
        --version=*) OITE_VERSION="${arg#*=}" ;;
        --help)
            echo "Install Oite programming language toolchain"
            echo ""
            echo "Usage: curl -fsSL https://oite.org/install | sh"
            echo "   or: sh install.sh [OPTIONS]"
            echo ""
            echo "Options:"
            echo "  --prefix=DIR         Install to DIR (default: ~/.oite)"
            echo "  --version=TAG        Install specific version (default: latest)"
            echo "  --no-modify-path     Don't modify shell profile"
            echo "  --help               Show this help"
            exit 0
            ;;
        *) warn "Unknown option: $arg" ;;
    esac
done

detect_platform() {
    local os arch target

    os="$(uname -s)"
    arch="$(uname -m)"

    case "$os" in
        Darwin) os="apple-darwin" ;;
        Linux)  os="unknown-linux-gnu" ;;
        *)      err "Unsupported OS: $os. Oite supports macOS and Linux." ;;
    esac

    case "$arch" in
        x86_64|amd64)   arch="x86_64" ;;
        arm64|aarch64)   arch="aarch64" ;;
        *)               err "Unsupported architecture: $arch. Oite supports x86_64 and aarch64." ;;
    esac

    target="${arch}-${os}"
    echo "$target"
}

check_dependencies() {
    for cmd in curl tar git; do
        if ! command -v "$cmd" >/dev/null 2>&1; then
            err "Required command '$cmd' not found. Please install it and try again."
        fi
    done
}

get_latest_version() {
    local url="https://api.github.com/repos/${GITHUB_REPO}/releases/latest"
    local response

    response=$(curl -fsSL "$url" 2>/dev/null) || err "Failed to fetch latest version from GitHub"

    # Extract tag_name from JSON
    echo "$response" | grep '"tag_name"' | head -1 | sed 's/.*"tag_name": *"//;s/".*//'
}

download_oitec() {
    local target="$1"
    local version="$2"
    local bin_dir="${PREFIX}/bin"
    local tmpdir

    tmpdir=$(mktemp -d)
    trap "rm -rf '$tmpdir'" EXIT

    local artifact="oitec-${target}.tar.gz"
    local url="https://github.com/${GITHUB_REPO}/releases/download/${version}/${artifact}"

    info "Downloading oitec ${version} for ${target}..."
    if ! curl -fSL --progress-bar -o "${tmpdir}/${artifact}" "$url"; then
        rm -rf "$tmpdir"
        err "Failed to download ${url}. Check that a release exists for your platform."
    fi

    # Verify download
    if [ ! -s "${tmpdir}/${artifact}" ]; then
        rm -rf "$tmpdir"
        err "Downloaded file is empty"
    fi

    info "Extracting oitec..."
    mkdir -p "$bin_dir"
    tar -xzf "${tmpdir}/${artifact}" -C "$bin_dir"
    chmod +x "${bin_dir}/oitec"

    rm -rf "$tmpdir"
    trap - EXIT

    info "Installed oitec to ${bin_dir}/oitec"
}

install_unroll() {
    local version="$1"
    local lib_dir="${PREFIX}/lib/unroll"
    local bin_dir="${PREFIX}/bin"
    local tmpdir

    tmpdir=$(mktemp -d)
    trap "rm -rf '$tmpdir'" EXIT

    # Try downloading unroll source tarball from the release
    local url="https://github.com/${GITHUB_REPO}/releases/download/${version}/unroll-src.tar.gz"

    info "Downloading unroll package manager..."
    if curl -fSL --progress-bar -o "${tmpdir}/unroll-src.tar.gz" "$url" 2>/dev/null; then
        # Extract from release tarball
        mkdir -p "$lib_dir"
        tar -xzf "${tmpdir}/unroll-src.tar.gz" -C "$lib_dir"
    else
        # Fallback: clone from git
        warn "Release tarball not found, cloning unroll from git..."
        if ! git clone --depth 1 "https://github.com/${UNROLL_REPO}.git" "${tmpdir}/unroll" 2>/dev/null; then
            rm -rf "$tmpdir"
            err "Failed to download unroll source"
        fi
        mkdir -p "$lib_dir"
        cp -r "${tmpdir}/unroll/src" "$lib_dir/"
        if [ -f "${tmpdir}/unroll/unroll.toml" ]; then
            cp "${tmpdir}/unroll/unroll.toml" "$lib_dir/"
        fi
    fi

    rm -rf "$tmpdir"
    trap - EXIT

    # Create wrapper script
    mkdir -p "$bin_dir"
    cat > "${bin_dir}/unroll" << 'WRAPPER'
#!/usr/bin/env sh
OITE_HOME="${OITE_HOME:-$HOME/.oite}"
exec "$OITE_HOME/bin/oitec" "$OITE_HOME/lib/unroll/src/main.ot" -- "$@"
WRAPPER
    chmod +x "${bin_dir}/unroll"

    info "Installed unroll to ${lib_dir}"
}

install_core_rolls() {
    local version="$1"
    local tmpdir

    tmpdir=$(mktemp -d)
    trap "rm -rf '$tmpdir'" EXIT

    # The core rolls (@rolls/std and what it needs, @rolls/async), pinned for
    # this release. Releases before they were bundled have none.
    local url="https://github.com/${GITHUB_REPO}/releases/download/${version}/core-rolls.tar.gz"

    info "Downloading the core rolls..."
    if curl -fSL --progress-bar -o "${tmpdir}/core-rolls.tar.gz" "$url" 2>/dev/null \
        && [ -s "${tmpdir}/core-rolls.tar.gz" ]; then
        # The release owns ${PREFIX}/rolls; rolls fetched by unroll live
        # elsewhere, so replacing it loses nothing.
        rm -rf "${PREFIX}/rolls"
        mkdir -p "$PREFIX"
        tar -xzf "${tmpdir}/core-rolls.tar.gz" -C "$PREFIX"
        info "Installed the core rolls to ${PREFIX}/rolls:"
        sed 's/^/      /' "${PREFIX}/rolls/MANIFEST"
    else
        warn "This release has no bundled core rolls; unroll fetches them from the registry"
    fi

    rm -rf "$tmpdir"
    trap - EXIT
}

configure_registry() {
    local unroll_dir="$HOME/.unroll"
    local config_file="${unroll_dir}/config.toml"

    mkdir -p "$unroll_dir"

    if [ ! -f "$config_file" ]; then
        cat > "$config_file" << EOF
# Unroll package manager configuration
# Generated by install.sh

[registry]
default = "${REGISTRY_URL}"
EOF
        info "Configured registry: ${REGISTRY_URL}"
    else
        info "Registry config already exists at ${config_file}, skipping"
    fi
}

setup_path() {
    if [ "$MODIFY_PATH" -eq 0 ]; then
        return 0
    fi

    local bin_dir="${PREFIX}/bin"
    local path_entry="export PATH=\"${bin_dir}:\$PATH\""

    # Check if already in PATH
    case ":$PATH:" in
        *":${bin_dir}:"*) info "PATH already includes ${bin_dir}"; return 0 ;;
    esac

    local shell_name
    shell_name="$(basename "${SHELL:-/bin/sh}")"

    local profile=""
    case "$shell_name" in
        zsh)
            profile="$HOME/.zshrc"
            ;;
        bash)
            if [ -f "$HOME/.bash_profile" ]; then
                profile="$HOME/.bash_profile"
            elif [ -f "$HOME/.bashrc" ]; then
                profile="$HOME/.bashrc"
            else
                profile="$HOME/.profile"
            fi
            ;;
        fish)
            # Fish uses a different syntax
            local fish_dir="$HOME/.config/fish/conf.d"
            mkdir -p "$fish_dir"
            echo "set -gx PATH ${bin_dir} \$PATH" > "${fish_dir}/oite.fish"
            info "Added ${bin_dir} to PATH in ${fish_dir}/oite.fish"
            return 0
            ;;
        *)
            profile="$HOME/.profile"
            ;;
    esac

    if [ -n "$profile" ]; then
        # Check if already added
        if [ -f "$profile" ] && grep -q "\.oite/bin" "$profile" 2>/dev/null; then
            info "PATH entry already exists in ${profile}"
            return 0
        fi

        echo "" >> "$profile"
        echo "# Oite programming language" >> "$profile"
        echo "$path_entry" >> "$profile"
        info "Added ${bin_dir} to PATH in ${profile}"
    fi
}

verify_install() {
    local bin_dir="${PREFIX}/bin"

    echo ""
    if [ -x "${bin_dir}/oitec" ]; then
        # oitec must be able to start on this machine. A missing shared library
        # makes the loader kill it (status >= 126) before it runs at all; any
        # lower status is oitec itself answering, which is all this checks.
        local output status=0
        output=$("${bin_dir}/oitec" --version 2>&1) || status=$?
        if [ "$status" -ge 126 ] || printf '%s' "$output" | grep -q -E 'Library not loaded|error while loading shared libraries|GLIBC_[0-9.]+. not found'; then
            warn "oitec is installed at ${bin_dir}/oitec but cannot start on this machine:"
            printf '%s\n' "$output" | sed 's/^/    /' >&2
            warn "Please report this at https://github.com/${GITHUB_REPO}/issues"
            return 1
        fi
        info "oitec installed: ${bin_dir}/oitec"
    else
        warn "oitec binary not found at ${bin_dir}/oitec"
        return 1
    fi

    if [ -x "${bin_dir}/unroll" ]; then
        info "unroll installed: ${bin_dir}/unroll"
    else
        warn "unroll not found at ${bin_dir}/unroll"
        return 1
    fi

    return 0
}

print_success() {
    echo ""
    printf "${BOLD}${GREEN}Oite has been installed successfully!${RESET}\n"
    echo ""
    echo "To get started, you may need to restart your shell or run:"
    echo ""
    printf "  ${BOLD}export PATH=\"${PREFIX}/bin:\$PATH\"${RESET}\n"
    echo ""
    echo "Then create your first project:"
    echo ""
    printf "  ${BOLD}unroll new hello${RESET}\n"
    printf "  ${BOLD}cd hello${RESET}\n"
    printf "  ${BOLD}unroll build${RESET}\n"
    echo ""
    echo "Documentation: https://oite.org/docs"
    echo "Registry:      https://registry.oite.org"
    echo ""
}

# ─── Main ───────────────────────────────────────────────────────────────────

main() {
    echo ""
    printf "${BOLD}Oite Installer${RESET}\n"
    echo ""

    check_dependencies

    local target
    target=$(detect_platform)
    info "Detected platform: ${target}"

    local version
    if [ "$OITE_VERSION" = "latest" ]; then
        version=$(get_latest_version)
        if [ -z "$version" ]; then
            err "Could not determine latest version"
        fi
        info "Latest version: ${version}"
    else
        version="$OITE_VERSION"
    fi

    download_oitec "$target" "$version"
    install_unroll "$version"
    install_core_rolls "$version"
    configure_registry
    setup_path

    if verify_install; then
        print_success
    else
        warn "Installation completed with warnings. Check the output above."
    fi
}

main
