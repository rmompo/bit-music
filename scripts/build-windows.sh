#!/usr/bin/env bash
# Builds a bit-music executable for Windows (.exe) via cross-compilation
# from Linux/WSL2 using mingw-w64.
#
# Usage: scripts/build-windows.sh [package]     (default package: bm)
#
# One-time requirements:
#   sudo apt-get update && sudo apt-get install -y mingw-w64
#   rustup target add x86_64-pc-windows-gnu
#
# The resulting binary can be run directly from WSL2 thanks to the
# Windows interop, or copied to a Windows path (/mnt/c/...).

set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."

PACKAGE="${1:-bm}"
TARGET="x86_64-pc-windows-gnu"

if ! command -v x86_64-w64-mingw32-gcc >/dev/null 2>&1; then
    echo "Error: x86_64-w64-mingw32-gcc not found. Install with:" >&2
    echo "  sudo apt-get update && sudo apt-get install -y mingw-w64" >&2
    exit 1
fi

if ! rustup target list --installed | grep -q "^${TARGET}\$"; then
    echo "Installing Rust target ${TARGET}..."
    rustup target add "${TARGET}"
fi

cargo build --release --target "${TARGET}" -p "${PACKAGE}"

BIN="target/${TARGET}/release/${PACKAGE}.exe"
echo
echo "Executable generated: ${BIN}"
