#!/usr/bin/env bash
# Signs the Windows executables with the development certificate made by
# scripts/make-dev-cert.sh, writing the signed copies to dist/signed/ (the
# originals in target/ are left untouched).
#
# Usage: scripts/sign-windows.sh [file.exe ...]
#        (default: bm.exe and bm-gui.exe from the release build)
#
# Needs osslsigncode:  sudo apt-get install -y osslsigncode
#
# Windows only treats the result as validly signed on a machine where the
# certificate is installed (scripts/install-dev-cert.ps1); there, Smart App
# Control lets the signed copies run. Run the ones in dist/signed/, not the
# unsigned ones in target/ (see specs/ci-and-signing.md).

set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."

DIR="${BM_SIGN_DIR:-$HOME/.bit-music-signing}"
# Which certificate: BM_SIGN_FILE (base name, as asked by make-dev-cert.sh), or
# the only one in the directory, or "kangaroo-development" among several.
FILE="${BM_SIGN_FILE:-}"
if [[ -z "${FILE}" ]]; then
    shopt -s nullglob
    found=("${DIR}"/*.crt)
    shopt -u nullglob
    if [[ ${#found[@]} -eq 1 ]]; then
        FILE="$(basename "${found[0]}" .crt)"
    elif [[ -f "${DIR}/kangaroo-development.crt" ]]; then
        FILE="kangaroo-development"
    fi
fi
KEY="${DIR}/${FILE}.key"
CRT="${DIR}/${FILE}.crt"
RELEASE="target/x86_64-pc-windows-gnu/release"
OUT="dist/signed"
# Tried in order; the timestamp keeps the signature valid after the
# certificate expires.
TIMESTAMP_SERVERS=(http://timestamp.digicert.com http://timestamp.sectigo.com http://time.certum.pl)

if ! command -v osslsigncode >/dev/null 2>&1; then
    echo "Error: osslsigncode not found. Install with:" >&2
    echo "  sudo apt-get update && sudo apt-get install -y osslsigncode" >&2
    exit 1
fi
if [[ -z "${FILE}" || ! -f "${KEY}" || ! -f "${CRT}" ]]; then
    echo "Error: no (or no single) certificate in ${DIR}. Run scripts/make-dev-cert.sh," >&2
    echo "or choose one with BM_SIGN_FILE=<base name>." >&2
    exit 1
fi

if [[ $# -gt 0 ]]; then
    FILES=("$@")
else
    FILES=("${RELEASE}/bm.exe" "${RELEASE}/bm-gui.exe")
fi

mkdir -p "${OUT}"
for exe in "${FILES[@]}"; do
    if [[ ! -f "${exe}" ]]; then
        echo "Error: ${exe} not found (build it with scripts/build-windows.sh)." >&2
        exit 1
    fi
    name="$(basename "${exe}")"
    # osslsigncode refuses to overwrite, and a re-run should refresh the copy.
    rm -f "${OUT}/${name}"
    signed=false
    last_error=""
    for ts in "${TIMESTAMP_SERVERS[@]}"; do
        if last_error="$(osslsigncode sign -h sha256 -certs "${CRT}" -key "${KEY}" \
            -n "bit-music ${name%.exe}" -ts "${ts}" \
            -in "${exe}" -out "${OUT}/${name}" 2>&1)"; then
            signed=true
            echo "Signed ${name} (timestamp: ${ts})"
            break
        fi
        rm -f "${OUT}/${name}"
    done
    if ! ${signed}; then
        echo "Error: could not sign ${name} (no timestamp server reachable, or:)" >&2
        echo "${last_error}" | tail -3 >&2
        exit 1
    fi
    # The certificate is self-signed, so it is its own trust anchor here.
    if osslsigncode verify -CAfile "${CRT}" -in "${OUT}/${name}" >/dev/null 2>&1; then
        echo "  signature verified against ${CRT}"
    else
        echo "  warning: osslsigncode could not verify the signature (a timestamp" >&2
        echo "  check can fail without the timestamp CA); check it on Windows with" >&2
        echo "  Get-AuthenticodeSignature." >&2
    fi
done
echo "Signed files are in ${OUT}/"
