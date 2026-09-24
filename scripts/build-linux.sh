#!/usr/bin/env bash
# Builds a bit-music executable for Linux (ALSA audio backend).
# Meant for development/testing inside WSL2 or a native Linux box.
#
# Usage: scripts/build-linux.sh [package]     (default package: bm)
#
# One-time requirements:
#   sudo apt-get update && sudo apt-get install -y pkg-config libasound2-dev

set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."

PACKAGE="${1:-bm}"
TARGET="x86_64-unknown-linux-gnu"

if ! command -v pkg-config >/dev/null 2>&1; then
    echo "Error: pkg-config not found. Install with:" >&2
    echo "  sudo apt-get update && sudo apt-get install -y pkg-config libasound2-dev" >&2
    exit 1
fi

if ! pkg-config --exists alsa; then
    echo "Error: libasound2-dev (ALSA) not found. Install with:" >&2
    echo "  sudo apt-get update && sudo apt-get install -y pkg-config libasound2-dev" >&2
    exit 1
fi

cargo build --release --target "${TARGET}" -p "${PACKAGE}"

# The executable has the package's name, except the GUI: package gui-player
# builds bm-gui.
BINARY="${PACKAGE}"
if [ "${PACKAGE}" = "gui-player" ]; then
    BINARY="bm-gui"
fi

BIN="target/${TARGET}/release/${BINARY}"
echo
echo "Executable generated: ${BIN}"
