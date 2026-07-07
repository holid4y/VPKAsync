#!/usr/bin/env bash
set -euo pipefail

# Cross-compile VPKAsync for Windows from Linux using llvm-mingw.
# Requires: rustup, x86_64-pc-windows-gnullvm target, and .tools/llvm-mingw/

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$ROOT"

if ! command -v cargo >/dev/null 2>&1; then
    echo "ERROR: 'cargo' was not found in PATH."
    exit 1
fi

VERSION=$(grep '^version =' Cargo.toml | head -n 1 | cut -d '"' -f 2)
if [[ -z "${VERSION}" ]]; then
    echo "ERROR: Could not read version from Cargo.toml."
    exit 1
fi

MINGW_DIR="${ROOT}/.tools/llvm-mingw"
MINGW_ARCHIVE="llvm-mingw-20260602-ucrt-ubuntu-22.04-x86_64.tar.xz"
MINGW_URL="https://github.com/mstorsjo/llvm-mingw/releases/download/20260602/${MINGW_ARCHIVE}"

if [[ ! -x "${MINGW_DIR}/bin/x86_64-w64-mingw32-clang" ]]; then
    echo "Downloading llvm-mingw toolchain..."
    mkdir -p "${ROOT}/.tools"
    curl -fsSL -o "${ROOT}/.tools/${MINGW_ARCHIVE}" "${MINGW_URL}"
    tar -xf "${ROOT}/.tools/${MINGW_ARCHIVE}" -C "${ROOT}/.tools"
    mv "${ROOT}/.tools/llvm-mingw-20260602-ucrt-ubuntu-22.04-x86_64" "${MINGW_DIR}"
fi

export PATH="${MINGW_DIR}/bin:${PATH}"
ln -sf x86_64-w64-mingw32-windres "${MINGW_DIR}/bin/windres"

if command -v rustup >/dev/null 2>&1; then
    rustup target add x86_64-pc-windows-gnullvm >/dev/null
fi

echo "Building Windows release (x86_64-pc-windows-gnullvm)..."
cargo build --release --target x86_64-pc-windows-gnullvm

mkdir -p dist
OUT="dist/VPKAsync_v${VERSION}.exe"
cp -f "target/x86_64-pc-windows-gnullvm/release/async_vpk.exe" "${OUT}"

echo "BUILD OK! Created: ${OUT}"
