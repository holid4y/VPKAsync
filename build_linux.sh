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
BASE="dist/VPKAsync_v${VERSION}"

# Copy and rename for common Linux conventions
echo "Copying binaries to dist/..."
cp -f target/release/async_vpk "${BASE}.bin"
cp -f target/release/async_vpk "${BASE}.elf"
cp -f target/release/async_vpk "${BASE}.x86_64"

chmod +x "${BASE}.bin" "${BASE}.elf" "${BASE}.x86_64"

echo ""
echo "BUILD OK! Created Linux binaries in 'dist/':"
echo "  - ${BASE}.bin"
echo "  - ${BASE}.elf"
echo "  - ${BASE}.x86_64"
echo "------------------------------------------------"
