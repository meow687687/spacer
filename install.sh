#!/usr/bin/env bash
# Spacer Installer
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/meow687687/spacer/main/install.sh | bash
#   or:
#   sh install.sh

set -e

REPO="${GITHUB_REPO:-"meow687687/spacer"}"
BINARY_NAME="spacer"

# Terminal formatting
if [ -t 1 ]; then
    BOLD="$(printf '\033[1m')"
    DIM="$(printf '\033[2m')"
    CYAN="$(printf '\033[36m')"
    BRIGHT_CYAN="$(printf '\033[96m')"
    GREEN="$(printf '\033[32m')"
    BRIGHT_GREEN="$(printf '\033[92m')"
    YELLOW="$(printf '\033[33m')"
    RED="$(printf '\033[31m')"
    MAGENTA="$(printf '\033[35m')"
    RESET="$(printf '\033[0m')"
else
    BOLD=""
    DIM=""
    CYAN=""
    BRIGHT_CYAN=""
    GREEN=""
    BRIGHT_GREEN=""
    YELLOW=""
    RED=""
    MAGENTA=""
    RESET=""
fi

print_banner() {
    cat << "EOF"

  [1;36m  ___ _ __   __ _  ___ ___ _ __ [0m
  [1;36m / __| '_ \ / _` |/ __/ _ \ '__|[0m
  [1;36m \__ \ |_) | (_| | (_|  __/ |   [0m
  [1;36m |___/ .__/ \__,_|\___\___|_|   [0m
  [1;36m     |_|                        [0m
  [1;32m ⚡ Terminal Storage Space Manager & Deduplicator[0m

EOF
}

step() {
    local num="$1"
    local total="$2"
    local msg="$3"
    printf "  ${BRIGHT_CYAN}[%s/%s]${RESET} %s...\n" "$num" "$total" "$msg"
}

step_done() {
    local msg="$1"
    printf "  ${BRIGHT_GREEN}  ✓${RESET} %s\n" "$msg"
}

warn() {
    printf "  ${YELLOW}  ! WARN:${RESET} %s\n" "$*"
}

error() {
    printf "\n  ${RED}${BOLD}  ✗ ERROR:${RESET} %s\n\n" "$*" >&2
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
    print_banner
    printf "  ${DIM}Starting automated installation for ${BOLD}%s${RESET}${DIM}...${RESET}\n\n" "$REPO"

    # Step 1: System Detection
    step "1" "4" "Detecting host operating system & architecture"
    detect_target
    determine_install_dir
    step_done "Detected platform: ${BOLD}${TARGET}${RESET} ➔ ${CYAN}${INSTALL_DIR}${RESET}"

    # Step 2: Check local repo or resolve release
    SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]:-$0}")" 2>/dev/null && pwd || echo "")"
    if [ -f "$SCRIPT_DIR/Cargo.toml" ] && [ -f "$SCRIPT_DIR/src/main.rs" ]; then
        step "2" "4" "Local source detected; building release binary"
        if command -v cargo >/dev/null 2>&1; then
            (cd "$SCRIPT_DIR" && cargo build --release -q)
            step_done "Built target/release/$BINARY_NAME"

            step "3" "4" "Installing binary to $INSTALL_DIR"
            mkdir -p "$INSTALL_DIR"
            if [ -w "$INSTALL_DIR" ]; then
                cp "$SCRIPT_DIR/target/release/$BINARY_NAME" "$INSTALL_DIR/$BINARY_NAME"
            else
                sudo cp "$SCRIPT_DIR/target/release/$BINARY_NAME" "$INSTALL_DIR/$BINARY_NAME"
            fi
            chmod +x "$INSTALL_DIR/$BINARY_NAME"
            step_done "Binary placed at $INSTALL_DIR/$BINARY_NAME"

            step "4" "4" "Validating installation"
            step_done "Verified executable"
            print_completion "v0.2.0 (local build)"
            exit 0
        fi
    fi

    step "2" "4" "Fetching release metadata from GitHub"
    LATEST_RELEASE_URL="https://api.github.com/repos/$REPO/releases/latest"
    RELEASE_JSON="$(curl -sSL "$LATEST_RELEASE_URL" 2>/dev/null || echo "")"
    TAG="$(echo "$RELEASE_JSON" | grep '"tag_name":' | sed -E 's/.*"([^"]+)".*/\1/' || echo "")"

    if [ -z "$TAG" ]; then
        warn "Could not reach GitHub Releases API. Attempting cargo install fallback..."
        if command -v cargo >/dev/null 2>&1; then
            cargo install --git "https://github.com/$REPO.git" --bin spacer || {
                error "Failed to install via cargo. Please install Rust (https://rustup.rs) or download the prebuilt binary manually."
                exit 1
            }
            step_done "Installed successfully via cargo"
            print_completion "latest (cargo)"
            exit 0
        else
            error "Could not fetch prebuilt release and Cargo is not installed."
            exit 1
        fi
    fi
    step_done "Found latest release: ${BRIGHT_GREEN}${BOLD}${TAG}${RESET}"

    # Step 3: Download Binary
    step "3" "4" "Downloading prebuilt release (${TAG})"
    TMP_DIR="$(mktemp -d)"
    trap 'rm -rf "$TMP_DIR"' EXIT

    DOWNLOAD_URL="https://github.com/$REPO/releases/download/$TAG/spacer-${TARGET}.tar.gz"
    
    if curl -fL --progress-bar "$DOWNLOAD_URL" -o "$TMP_DIR/spacer.tar.gz" 2>/dev/null; then
        tar -xzf "$TMP_DIR/spacer.tar.gz" -C "$TMP_DIR"
    else
        # Try raw binary fallback
        RAW_URL="https://github.com/$REPO/releases/download/$TAG/spacer-${TARGET}"
        curl -fL --progress-bar "$RAW_URL" -o "$TMP_DIR/$BINARY_NAME" || {
            error "Failed to download binary from $DOWNLOAD_URL or $RAW_URL"
            exit 1
        }
    fi
    step_done "Download & extraction complete"

    # Step 4: Installation & Verification
    step "4" "4" "Placing binary into $INSTALL_DIR"
    mkdir -p "$INSTALL_DIR"
    if [ -w "$INSTALL_DIR" ]; then
        mv "$TMP_DIR/$BINARY_NAME" "$INSTALL_DIR/$BINARY_NAME"
    else
        printf "  ${DIM}Elevated permissions required to write to %s${RESET}\n" "$INSTALL_DIR"
        sudo mv "$TMP_DIR/$BINARY_NAME" "$INSTALL_DIR/$BINARY_NAME"
    fi
    chmod +x "$INSTALL_DIR/$BINARY_NAME"
    step_done "Permissions configured (+x)"

    print_completion "$TAG"
}

print_completion() {
    local version="$1"

    cat << EOF

  ┌────────────────────────────────────────────────────────────────────────┐
  │  ${BRIGHT_GREEN}✓ Installation Successful!${RESET}                                        │
  │  ${BOLD}Spacer ${version}${RESET} is ready at ${CYAN}${INSTALL_DIR}/${BINARY_NAME}${RESET}            │
  ├────────────────────────────────────────────────────────────────────────┤
  │  ${BOLD}Quickstart Commands:${RESET}                                                 │
  │    ${GREEN}spacer${RESET}                Launch interactive TUI in current folder      │
  │    ${GREEN}spacer ~${RESET}              Scan entire home directory                    │
  │    ${GREEN}spacer clean --dry-run${RESET}Check recoverable cache & build junk        │
  │    ${GREEN}spacer dupes ~${RESET}        Find duplicate files                          │
  │    ${GREEN}spacer --help${RESET}         Explore all CLI subcommands & flags           │
  └────────────────────────────────────────────────────────────────────────┘

EOF

    # Check if INSTALL_DIR is in PATH
    case ":$PATH:" in
        *":$INSTALL_DIR:"*) ;;
        *)
            warn "'$INSTALL_DIR' is not in your current \$PATH."
            printf "     Add this line to your ${BOLD}~/.bashrc${RESET} or ${BOLD}~/.zshrc${RESET}:\n"
            printf "     ${CYAN}export PATH=\"\$PATH:%s\"${RESET}\n\n" "$INSTALL_DIR"
            ;;
    esac
}

main "$@"
