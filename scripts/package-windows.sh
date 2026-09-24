#!/usr/bin/env bash
# Packages the Windows build into dist/bit-music-<version>-windows-x86_64.zip:
# both executables (bm.exe and bm-gui.exe), the editable help file, the
# licenses, the README, and the demo composition with its samples (kept in
# the same relative layout, since a .bm1 finds its samples relative to
# itself).
#
# Usage: scripts/package-windows.sh [--signed] [version-label]
#        (default label: dev)
#
# By default the executables come from the release build (target/); with
# --signed they come from dist/signed/, where scripts/sign-windows.sh puts the
# signed copies. Only the package's own folder and zip are replaced: dist/signed
# is left alone.
#
# Requires `zip`. Run scripts/build-windows.sh for bm and gui-player first (or
# scripts/build-signed-windows.sh, for --signed).

set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."

BIN="target/x86_64-pc-windows-gnu/release"
if [ "${1:-}" = "--signed" ]; then
    BIN="dist/signed"
    shift
fi

VERSION="${1:-dev}"
VERSION="${VERSION//\//-}"   # a branch name like feature/x must not break the file name
NAME="bit-music-${VERSION}-windows-x86_64"
STAGE="dist/${NAME}"

for exe in bm.exe bm-gui.exe; do
    if [ ! -f "${BIN}/${exe}" ]; then
        echo "Error: ${BIN}/${exe} not found. Build it first:" >&2
        echo "  scripts/build-windows.sh bm && scripts/build-windows.sh gui-player" >&2
        echo "  (or scripts/build-signed-windows.sh for --signed)" >&2
        exit 1
    fi
done

rm -rf "${STAGE}" "${STAGE}.zip"
mkdir -p "${STAGE}/demos/songs"

cp "${BIN}/bm.exe" "${BIN}/bm-gui.exe" "${STAGE}/"
cp player/bm.hlp LICENSE-MIT LICENSE-APACHE THIRD_PARTY_LICENSES.md README.md "${STAGE}/"
cp demos/songs/*.bm1 "${STAGE}/demos/songs/"
cp -r demos/samples "${STAGE}/demos/samples"
rm -f "${STAGE}/demos/samples/README.md"

(cd dist && zip -qr "${NAME}.zip" "${NAME}")

echo "Package generated: dist/${NAME}.zip ($(du -h "dist/${NAME}.zip" | cut -f1))"
