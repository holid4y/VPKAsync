#!/usr/bin/env bash
set -euo pipefail

# This script builds VPKAsync for Linux.
# It requires Rust and Cargo to be installed.
# For GUI support, you may need: libgtk-3-dev, libwayland-dev, libx11-dev, libxkbcommon-dev

echo "------------------------------------------------"
echo "  Building VPKAsync for Linux..."
echo "------------------------------------------------"

if ! command -v cargo >/dev/null 2>&1; then
    echo "ERROR: 'cargo' was not found in PATH."
    echo "Install Rust/Cargo from: https://rustup.rs/"
    exit 1
fi

# Extract version from Cargo.toml
VERSION=$(grep "^version =" Cargo.toml | head -n 1 | cut -d '"' -f 2)

if [[ -z "${VERSION}" ]]; then
    echo "ERROR: Could not read version from Cargo.toml."
    exit 1
fi

echo "Detected Version: ${VERSION}"

# Run the build
echo "Running cargo build --release..."
cargo build --release

# Prepare dist directory
mkdir -p dist
LINUX_MAIN="dist/VPKAsync_v${VERSION}-LinuxMain.bin"
LINUX_ELF="dist/VPKAsync_v${VERSION}-Linux.elf"
LINUX_X86_64="dist/VPKAsync_v${VERSION}-Linux.x86_64"

# Copy and rename for common Linux conventions
echo "Copying binaries to dist/..."
cp -f target/release/async_vpk "${LINUX_MAIN}"
cp -f target/release/async_vpk "${LINUX_ELF}"
cp -f target/release/async_vpk "${LINUX_X86_64}"

chmod +x "${LINUX_MAIN}" "${LINUX_ELF}" "${LINUX_X86_64}"

echo ""
echo "BUILD OK! Created Linux binaries in 'dist/':"
echo "  - ${LINUX_ELF}"
echo "  - ${LINUX_X86_64}"
echo "  - ${LINUX_MAIN}"
echo "------------------------------------------------"
