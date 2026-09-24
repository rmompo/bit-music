#!/usr/bin/env bash
# Builds the Windows executables and signs them in one step: the same as
# scripts/build-windows.sh followed by scripts/sign-windows.sh.
#
# Usage: scripts/build-signed-windows.sh [package ...]
#        (default: bm and gui-player)
#
# The signed copies end up in dist/signed/; run those, not the unsigned ones
# in target/. Close a running copy first: Windows will not let its file be
# replaced. Needs the certificate from scripts/make-dev-cert.sh.

set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."

if [[ $# -gt 0 ]]; then
    PACKAGES=("$@")
else
    PACKAGES=(bm gui-player)
fi

RELEASE="target/x86_64-pc-windows-gnu/release"
FILES=()
for package in "${PACKAGES[@]}"; do
    scripts/build-windows.sh "${package}"
    # Each package builds an executable with its own name, except the GUI
    # (package gui-player, executable bm-gui).
    binary="${package}"
    if [[ "${package}" == "gui-player" ]]; then
        binary="bm-gui"
    fi
    FILES+=("${RELEASE}/${binary}.exe")
done

scripts/sign-windows.sh "${FILES[@]}"
