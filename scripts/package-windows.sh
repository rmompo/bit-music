#!/usr/bin/env bash
# Packages the Windows build into dist/bit-music-<version>-windows-x86_64.zip:
# both executables, the editable help file, the licenses, the README, and the
# demo composition with its samples (kept in the same relative layout, since
# a .bm1 finds its samples relative to itself).
#
# Usage: scripts/package-windows.sh [version-label]     (default label: dev)
#
# Requires `zip`. Run scripts/build-windows.sh for bm and gui-player first.

set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."

VERSION="${1:-dev}"
VERSION="${VERSION//\//-}"   # a branch name like feature/x must not break the file name
BIN="target/x86_64-pc-windows-gnu/release"
NAME="bit-music-${VERSION}-windows-x86_64"
STAGE="dist/${NAME}"

for exe in bm.exe gui-player.exe; do
    if [ ! -f "${BIN}/${exe}" ]; then
        echo "Error: ${BIN}/${exe} not found. Build it first:" >&2
        echo "  scripts/build-windows.sh bm && scripts/build-windows.sh gui-player" >&2
        exit 1
    fi
done

rm -rf dist
mkdir -p "${STAGE}/demos/songs"

cp "${BIN}/bm.exe" "${BIN}/gui-player.exe" "${STAGE}/"
cp player/bm.hlp LICENSE-MIT LICENSE-APACHE THIRD_PARTY_LICENSES.md README.md "${STAGE}/"
cp demos/songs/*.bm1 "${STAGE}/demos/songs/"
cp -r demos/samples "${STAGE}/demos/samples"
rm -f "${STAGE}/demos/samples/README.md"

(cd dist && zip -qr "${NAME}.zip" "${NAME}")

echo "Package generated: dist/${NAME}.zip ($(du -h "dist/${NAME}.zip" | cut -f1))"
