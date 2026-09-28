#!/usr/bin/env bash
# Spacer Installer
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/<owner>/spacer/main/install.sh | bash
#   or:
#   sh install.sh

set -e

REPO="${GITHUB_REPO:-"meow687687/spacer"}"
BINARY_NAME="spacer"

# Styling
BOLD="$(tput bold 2>/dev/null || echo '')"
GREEN="$(tput setaf 2 2>/dev/null || echo '')"
CYAN="$(tput setaf 6 2>/dev/null || echo '')"
YELLOW="$(tput setaf 3 2>/dev/null || echo '')"
RED="$(tput setaf 1 2>/dev/null || echo '')"
RESET="$(tput sgr0 2>/dev/null || echo '')"

info() {
    echo "${CYAN}${BOLD}[INFO]${RESET} $*"
}

success() {
    echo "${GREEN}${BOLD}[SUCCESS]${RESET} $*"
}

warn() {
    echo "${YELLOW}${BOLD}[WARN]${RESET} $*"
}

error() {
    echo "${RED}${BOLD}[ERROR]${RESET} $*" >&2
}

# 1. Detect OS & Architecture
detect_target() {
    OS="$(uname -s | tr '[:upper:]' '[:lower:]')"
    ARCH="$(uname -m)"

    case "$OS" in
        linux*)
            TARGET_OS="unknown-linux-gnu"
            ;;
        darwin*)
            TARGET_OS="apple-darwin"
            ;;
        msys*|mingw*|cygwin*)
            TARGET_OS="pc-windows-msvc"
            ;;
        *)
            error "Unsupported operating system: $OS"
            exit 1
            ;;
    esac

    case "$ARCH" in
        x86_64|amd64)
            TARGET_ARCH="x86_64"
            ;;
        aarch64|arm64)
            TARGET_ARCH="aarch64"
            ;;
        *)
            error "Unsupported architecture: $ARCH"
            exit 1
            ;;
    esac

    TARGET="${TARGET_ARCH}-${TARGET_OS}"
}

# 2. Determine Installation Directory
determine_install_dir() {
    if [ -n "$SPACER_INSTALL_DIR" ]; then
        INSTALL_DIR="$SPACER_INSTALL_DIR"
    elif [ -w "/usr/local/bin" ]; then
        INSTALL_DIR="/usr/local/bin"
    elif [ -d "$HOME/.local/bin" ] || mkdir -p "$HOME/.local/bin" 2>/dev/null; then
        INSTALL_DIR="$HOME/.local/bin"
    elif [ -d "$HOME/bin" ] || mkdir -p "$HOME/bin" 2>/dev/null; then
        INSTALL_DIR="$HOME/bin"
    else
        INSTALL_DIR="/usr/local/bin"
    fi
}

main() {
    echo "${CYAN}${BOLD}⚡ Installing Spacer (Terminal Storage Space Manager)...${RESET}"
    detect_target
    determine_install_dir

    info "Detected platform: ${BOLD}$TARGET${RESET}"
    info "Target installation directory: ${BOLD}$INSTALL_DIR${RESET}"

    # Check if building from local repository or downloading release
    SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]:-$0}")" 2>/dev/null && pwd || echo "")"
    if [ -f "$SCRIPT_DIR/Cargo.toml" ] && [ -f "$SCRIPT_DIR/src/main.rs" ]; then
        info "Local Spacer source directory detected at $SCRIPT_DIR"
        if command -v cargo >/dev/null 2>&1; then
            info "Compiling optimized release binary with cargo..."
            (cd "$SCRIPT_DIR" && cargo build --release)
            mkdir -p "$INSTALL_DIR"
            if [ -w "$INSTALL_DIR" ]; then
                cp "$SCRIPT_DIR/target/release/$BINARY_NAME" "$INSTALL_DIR/$BINARY_NAME"
            else
                sudo cp "$SCRIPT_DIR/target/release/$BINARY_NAME" "$INSTALL_DIR/$BINARY_NAME"
            fi
            chmod +x "$INSTALL_DIR/$BINARY_NAME"
            print_completion
            exit 0
        fi
    fi

    # Download from GitHub Releases
    TMP_DIR="$(mktemp -d)"
    trap 'rm -rf "$TMP_DIR"' EXIT

    LATEST_RELEASE_URL="https://api.github.com/repos/$REPO/releases/latest"
    info "Fetching release metadata from $REPO..."

    RELEASE_JSON="$(curl -sSL "$LATEST_RELEASE_URL" 2>/dev/null || echo "")"
    TAG="$(echo "$RELEASE_JSON" | grep '"tag_name":' | sed -E 's/.*"([^"]+)".*/\1/' || echo "")"

    if [ -z "$TAG" ]; then
        warn "Could not reach GitHub Releases for $REPO. Attempting cargo install fallback..."
        if command -v cargo >/dev/null 2>&1; then
            info "Cargo found. Installing via cargo install..."
            cargo install --git "https://github.com/$REPO.git" --bin spacer || {
                error "Failed to install via cargo. Please install Rust (https://rustup.rs) or download the prebuilt binary manually."
                exit 1
            }
            success "Spacer installed successfully via cargo!"
            exit 0
        else
            error "Could not fetch prebuilt release and Cargo is not installed."
            error "Please install Rust (curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh) or download a release from https://github.com/$REPO/releases"
            exit 1
        fi
    fi

    DOWNLOAD_URL="https://github.com/$REPO/releases/download/$TAG/spacer-${TARGET}.tar.gz"
    info "Downloading Spacer $TAG for $TARGET..."
    
    if curl -fL "$DOWNLOAD_URL" -o "$TMP_DIR/spacer.tar.gz" 2>/dev/null; then
        tar -xzf "$TMP_DIR/spacer.tar.gz" -C "$TMP_DIR"
    else
        # Try raw binary format fallback
        RAW_URL="https://github.com/$REPO/releases/download/$TAG/spacer-${TARGET}"
        info "Trying direct binary download: $RAW_URL..."
        curl -fL "$RAW_URL" -o "$TMP_DIR/$BINARY_NAME" || {
            error "Failed to download binary from $DOWNLOAD_URL or $RAW_URL"
            exit 1
        }
    fi

    mkdir -p "$INSTALL_DIR"
    if [ -w "$INSTALL_DIR" ]; then
        mv "$TMP_DIR/$BINARY_NAME" "$INSTALL_DIR/$BINARY_NAME"
    else
        info "Elevated permissions required to write to $INSTALL_DIR"
        sudo mv "$TMP_DIR/$BINARY_NAME" "$INSTALL_DIR/$BINARY_NAME"
    fi

    chmod +x "$INSTALL_DIR/$BINARY_NAME"
    print_completion
}

print_completion() {
    echo ""
    success "Spacer was successfully installed to ${BOLD}$INSTALL_DIR/$BINARY_NAME${RESET}!"
    echo ""

    # Check if INSTALL_DIR is in PATH
    case ":$PATH:" in
        *":$INSTALL_DIR:"*) ;;
        *)
            warn "Note: '$INSTALL_DIR' is not in your current PATH."
            echo "To use 'spacer' from any terminal, add it to your shell configuration (e.g. ~/.bashrc or ~/.zshrc):"
            echo "  ${CYAN}export PATH=\"\$PATH:$INSTALL_DIR\"${RESET}"
            echo ""
            ;;
    esac

    echo "${BOLD}Get started by running:${RESET}"
    echo "  ${GREEN}spacer${RESET}              # Scan and manage current directory"
    echo "  ${GREEN}spacer ~${RESET}            # Scan entire home directory"
    echo "  ${GREEN}spacer /var/log${RESET}     # Scan specific directory"
    echo ""
}

main "$@"
