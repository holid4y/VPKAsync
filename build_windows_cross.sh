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

EXE="target/x86_64-pc-windows-gnullvm/release/async_vpk.exe"
echo "Checking Windows imports..."
IMPORTS="$("${MINGW_DIR}/bin/llvm-objdump" -p "${EXE}" | sed -n 's/^[[:space:]]*DLL Name: //p' | sort -fu)"
printf '%s\n' "${IMPORTS}" | sed 's/^/  /'

ALLOWED_IMPORT_RE='^(advapi32|bcrypt|bcryptprimitives|cfgmgr32|comctl32|comdlg32|crypt32|d2d1|d3d11|d3d12|d3d9|dbghelp|dwmapi|dxgi|gdi32|hid|imm32|iphlpapi|kernel32|mpr|msimg32|msvcrt|ncrypt|normaliz|ntdll|ole32|oleacc|oleaut32|opengl32|propsys|rpcrt4|runtimeobject|secur32|setupapi|shcore|shell32|shlwapi|user32|userenv|uxtheme|version|windowscodecs|winhttp|wininet|winmm|winspool\.drv|ws2_32|wtsapi32)\.dll$|^(api-ms-win-|ext-ms-win-)'
UNEXPECTED="$(printf '%s\n' "${IMPORTS}" | tr '[:upper:]' '[:lower:]' | grep -Ev "${ALLOWED_IMPORT_RE}" || true)"
if [[ -n "${UNEXPECTED}" ]]; then
    echo "ERROR: Windows build imports non-system DLLs:"
    printf '%s\n' "${UNEXPECTED}" | sed 's/^/  /'
    echo "The Windows release must be self-contained; check static CRT/runtime flags."
    exit 1
fi

mkdir -p dist
OUT="dist/VPKAsync_v${VERSION}-Windows.exe"
cp -f "${EXE}" "${OUT}"

echo "BUILD OK! Created: ${OUT}"
