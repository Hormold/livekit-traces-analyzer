#!/bin/bash
set -e

REPO="Hormold/livekit-traces-analyzer"
BINARY_NAME="livekit-analyzer"
INSTALL_DIR="${INSTALL_DIR:-/usr/local/bin}"

# Detect OS and architecture
OS=$(uname -s | tr '[:upper:]' '[:lower:]')
ARCH=$(uname -m)

case "$OS" in
    darwin)
        case "$ARCH" in
            x86_64) ASSET_NAME="livekit-analyzer-macos-x86_64" ;;
            arm64)  ASSET_NAME="livekit-analyzer-macos-arm64" ;;
            *)      echo "Unsupported architecture: $ARCH"; exit 1 ;;
        esac
        ;;
    linux)
        case "$ARCH" in
            x86_64) ASSET_NAME="livekit-analyzer-linux-x86_64" ;;
            *)      echo "Unsupported architecture: $ARCH"; exit 1 ;;
        esac
        ;;
    *)
        echo "Unsupported OS: $OS"
        echo "Please download manually from: https://github.com/$REPO/releases"
        exit 1
        ;;
esac

# Get latest release tag
echo "Fetching latest release..."
LATEST_TAG=$(curl -s "https://api.github.com/repos/$REPO/releases/latest" | grep '"tag_name":' | sed -E 's/.*"([^"]+)".*/\1/')

if [ -z "$LATEST_TAG" ]; then
    echo "Error: Could not find latest release"
    echo "Please check: https://github.com/$REPO/releases"
    exit 1
fi

DOWNLOAD_URL="https://github.com/$REPO/releases/download/$LATEST_TAG/$ASSET_NAME"

echo "Downloading $BINARY_NAME $LATEST_TAG for $OS/$ARCH..."
echo "URL: $DOWNLOAD_URL"

# Download to temp file
TEMP_FILE=$(mktemp)
curl -sL "$DOWNLOAD_URL" -o "$TEMP_FILE"

# Make executable
chmod +x "$TEMP_FILE"

# Install
if [ -w "$INSTALL_DIR" ]; then
    mv "$TEMP_FILE" "$INSTALL_DIR/$BINARY_NAME"
else
    echo "Installing to $INSTALL_DIR (requires sudo)..."
    sudo mv "$TEMP_FILE" "$INSTALL_DIR/$BINARY_NAME"
fi

echo ""
echo "Installed $BINARY_NAME $LATEST_TAG to $INSTALL_DIR/$BINARY_NAME"
echo ""
echo "Usage:"
echo "  $BINARY_NAME <path-to-observability-folder>"
