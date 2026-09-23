#!/usr/bin/env bash
# Builds a package (default: bm) for every supported target (Windows + Linux).
#
# Usage: scripts/build-all.sh [package]

set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")"

./build-windows.sh "$@"
./build-linux.sh "$@"
