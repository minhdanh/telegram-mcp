#!/usr/bin/env bash
set -euo pipefail

# Determine repository root
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
# Installation directories
INSTALL_DIR="${INSTALL_DIR:-${HOME}/.local/bin}"
BINARY_PATH="${INSTALL_DIR}/telegram-mcp"
CONFIG_DIR="${HOME}/.config/telegram-mcp"
CONFIG_PATH="${CONFIG_DIR}/config.yaml"

cd "${REPO_DIR}"

GITHUB_REPO="${GITHUB_REPO:-minhdanh/telegram-mcp}"
FORCE_BUILD=false

while [[ $# -gt 0 ]]; do
  case "$1" in
    --build)
      FORCE_BUILD=true
      shift
      ;;
    --install-dir)
      INSTALL_DIR="$2"
      shift 2
      ;;
    --config-dir)
      CONFIG_DIR="$2"
      shift 2
      ;;
    --help|-h)
      echo "Usage: $0 [--build] [--install-dir <dir>] [--config-dir <dir>]"
      exit 0
      ;;
    *)
      echo "Unknown argument: $1"
      echo "Usage: $0 [--build] [--install-dir <dir>] [--config-dir <dir>]"
      exit 1
      ;;
  esac
done

BINARY_PATH="${INSTALL_DIR}/telegram-mcp"
CONFIG_PATH="${CONFIG_DIR}/config.yaml"

echo "====================================================="
echo " Telegram MCP Server - macOS Setup & Install"
echo "====================================================="
echo "Repository: ${REPO_DIR}"
echo ""

# 1. Detect macOS architecture
OS="$(uname -s)"
ARCH="$(uname -m)"

if [ "${OS}" != "Darwin" ]; then
  echo "⚠️ Warning: This setup script is optimized for macOS (Darwin). Detected OS: ${OS}"
fi

case "${ARCH}" in
  arm64|aarch64)
    TARGET_ARCH="aarch64-apple-darwin"
    ARCH_DESC="Apple Silicon (M1/M2/M3/M4)"
    ;;
  x86_64)
    TARGET_ARCH="x86_64-apple-darwin"
    ARCH_DESC="Intel Mac (x86_64)"
    ;;
  *)
    echo "❌ Error: Unsupported architecture: ${ARCH}"
    exit 1
    ;;
esac

echo "--> Detected platform: macOS ${ARCH_DESC} (${TARGET_ARCH})"
echo ""

# 2. Check configuration file
echo "--> Checking configuration in ${CONFIG_PATH}..."
mkdir -p "${CONFIG_DIR}"

if [ ! -f "${CONFIG_PATH}" ]; then
  if [ -f "${REPO_DIR}/config.yaml" ]; then
    echo "Copying existing config.yaml to ${CONFIG_PATH}..."
    cp "${REPO_DIR}/config.yaml" "${CONFIG_PATH}"
    echo "✅ Copied config to ${CONFIG_PATH}"
  elif [ -f "${REPO_DIR}/config.yaml.example" ]; then
    echo "Creating config.yaml from config.yaml.example..."
    cp "${REPO_DIR}/config.yaml.example" "${CONFIG_PATH}"
    echo "⚠️  Created ${CONFIG_PATH}. Please edit it with your Telegram bot token and channels:"
    echo "   ${CONFIG_PATH}"
  else
    echo "❌ Error: config.yaml.example not found!"
    exit 1
  fi
else
  echo "✅ Configuration file exists: ${CONFIG_PATH}"
fi
echo ""

# 3. Obtain Binary (Download pre-built or Build from source)
mkdir -p "${INSTALL_DIR}"

download_success=false

if [ "${FORCE_BUILD}" = false ]; then
  echo "--> Checking for pre-compiled binary from GitHub Releases (${GITHUB_REPO})..."
  
  # Determine version/tag
  TAG="${VERSION:-latest}"
  if [ "${TAG}" = "latest" ]; then
    LATEST_TAG=$(curl -sSL -H "Accept: application/vnd.github.v3+json" "https://api.github.com/repos/${GITHUB_REPO}/releases/latest" 2>/dev/null | grep '"tag_name":' | head -n 1 | sed -E 's/.*"tag_name": *"([^"]+)".*/\1/' || true)
    if [ -n "${LATEST_TAG}" ]; then
      TAG="${LATEST_TAG}"
    else
      TAG="v0.1.0"
    fi
  fi

  TAR_NAME="telegram-mcp-${TARGET_ARCH}.tar.gz"
  DOWNLOAD_URL="https://github.com/${GITHUB_REPO}/releases/download/${TAG}/${TAR_NAME}"

  echo "Attempting to download ${TAG} (${TARGET_ARCH})..."
  TMP_DIR=$(mktemp -d)
  
  HTTP_STATUS=$(curl -sSL -w "%{http_code}" -o "${TMP_DIR}/${TAR_NAME}" "${DOWNLOAD_URL}" 2>/dev/null || echo "000")
  
  if [ "${HTTP_STATUS}" = "200" ] && [ -s "${TMP_DIR}/${TAR_NAME}" ]; then
    echo "✅ Downloaded pre-compiled binary successfully."
    echo "Extracting binary..."
    tar -xzf "${TMP_DIR}/${TAR_NAME}" -C "${TMP_DIR}"
    
    if [ -f "${TMP_DIR}/telegram-mcp" ]; then
      cp "${TMP_DIR}/telegram-mcp" "${BINARY_PATH}"
      chmod +x "${BINARY_PATH}"
      # Remove macOS Gatekeeper quarantine attribute if present
      xattr -d com.apple.quarantine "${BINARY_PATH}" 2>/dev/null || true
      download_success=true
    fi
  fi
  rm -rf "${TMP_DIR}"
fi

if [ "${download_success}" = false ]; then
  if [ "${FORCE_BUILD}" = true ]; then
    echo "--> '--build' flag requested. Compiling locally with Cargo..."
  else
    echo "ℹ️  No remote pre-compiled release found yet for ${GITHUB_REPO} (or offline)."
  fi

  # Check if binary was already compiled previously
  if [ -f "${REPO_DIR}/target/release/telegram-mcp" ] && [ "${FORCE_BUILD}" = false ]; then
    echo "✅ Found existing compiled binary at target/release/telegram-mcp"
    cp "${REPO_DIR}/target/release/telegram-mcp" "${BINARY_PATH}"
    chmod +x "${BINARY_PATH}"
  elif command -v cargo &> /dev/null; then
    echo "Building release binary using local Rust toolchain..."
    cargo build --release
    cp "${REPO_DIR}/target/release/telegram-mcp" "${BINARY_PATH}"
    chmod +x "${BINARY_PATH}"
  else
    echo ""
    echo "❌ Error: Could not download pre-built binary, and Rust (cargo) is not installed locally."
    echo ""
    echo "Next steps:"
    echo "1. Once a release tag (e.g. v0.1.0) is pushed to GitHub, GitHub Actions will publish pre-built binaries automatically."
    echo "2. Or install Rust to build locally: curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
    echo ""
    exit 1
  fi
fi

echo ""
echo "✅ telegram-mcp installed successfully at:"
echo "   ${BINARY_PATH}"
echo ""

# 4. Display client configuration snippets
echo "====================================================="
echo " macOS Client Configuration Snippets"
echo "====================================================="
echo ""
echo "1. Claude Desktop for Mac:"
echo "   Config path: ~/Library/Application Support/Claude/claude_desktop_config.json"
echo ""
cat <<EOF
{
  "mcpServers": {
    "telegram": {
      "command": "${BINARY_PATH}",
      "args": ["--config", "${CONFIG_PATH}"]
    }
  }
}
EOF
echo ""
echo "-----------------------------------------------------"
echo "2. Cursor for Mac:"
echo "   Config path: ~/.cursor/mcp.json (or project .cursor/mcp.json)"
echo ""
cat <<EOF
{
  "mcpServers": {
    "telegram": {
      "command": "${BINARY_PATH}",
      "args": ["--config", "${CONFIG_PATH}"]
    }
  }
}
EOF
echo ""
echo "-----------------------------------------------------"
echo "3. Gemini / Antigravity IDE:"
echo "   Config path: ~/.gemini/antigravity-ide/mcp_config.json"
echo ""
cat <<EOF
{
  "mcpServers": {
    "telegram": {
      "command": "${BINARY_PATH}",
      "args": ["--config", "${CONFIG_PATH}"]
    }
  }
}
EOF
echo ""
echo "====================================================="
echo " Quick Test Command (Manual stdio verify)"
echo "====================================================="
echo "Run this command in terminal to test the server directly:"
echo ""
echo "printf '{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{}}\\\\n{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/list\"}\\\\n' | \"${BINARY_PATH}\" --config \"${CONFIG_PATH}\""
echo ""
