#!/usr/bin/env bash
# Creates a self-signed code-signing certificate for signing the Windows
# executables during development. On a machine that trusts it (see
# scripts/install-dev-cert.ps1) Smart App Control lets the signed executables
# run; see specs/ci-and-signing.md.
#
# Usage: scripts/make-dev-cert.sh [--force]
#
# It asks two things (press Enter for the default of each):
#   - the certificate name, which Windows shows as the publisher and which you
#     will look for to find or remove it later ("kangaroo (development)"), and
#   - the base name of the files, <name>.key and <name>.crt
#     ("kangaroo-development").
# To skip the questions set BM_SIGN_NAME and/or BM_SIGN_FILE; without a
# terminal the defaults are used.
#
# The private key is a secret: it is created OUTSIDE the repository, in
# $BM_SIGN_DIR (default: ~/.bit-music-signing), readable only by you. Anyone
# holding it can sign programs that a Windows machine trusting this
# certificate will accept, so never commit it or share it. Only the public
# certificate (.crt) goes to Windows.

set -euo pipefail

DIR="${BM_SIGN_DIR:-$HOME/.bit-music-signing}"
DAYS=1095   # 3 years

if ! command -v openssl >/dev/null 2>&1; then
    echo "Error: openssl not found." >&2
    exit 1
fi

DEFAULT_NAME="kangaroo (development)"
NAME="${BM_SIGN_NAME:-}"
if [[ -z "${NAME}" && -t 0 ]]; then
    read -r -p "Certificate name [${DEFAULT_NAME}]: " NAME
fi
NAME="${NAME:-${DEFAULT_NAME}}"
# The name goes into an openssl "-subj" string, where these are separators.
if [[ "${NAME}" == *[/=,+\\]* ]]; then
    echo "Error: the name cannot contain any of / = , + \\" >&2
    exit 1
fi

DEFAULT_FILE="kangaroo-development"
FILE="${BM_SIGN_FILE:-}"
if [[ -z "${FILE}" && -t 0 ]]; then
    read -r -p "File name, without extension [${DEFAULT_FILE}]: " FILE
fi
FILE="${FILE:-${DEFAULT_FILE}}"
if [[ ! "${FILE}" =~ ^[A-Za-z0-9._-]+$ ]]; then
    echo "Error: the file name can only use letters, digits, . _ and -" >&2
    exit 1
fi

KEY="${DIR}/${FILE}.key"
CRT="${DIR}/${FILE}.crt"
if [[ -e "${KEY}" || -e "${CRT}" ]] && [[ "${1:-}" != "--force" ]]; then
    echo "${FILE}.key / ${FILE}.crt already exist in ${DIR}." >&2
    echo "Use --force to replace them (anything signed with the old one keeps" >&2
    echo "working only while the old certificate stays trusted)." >&2
    exit 1
fi

umask 077
mkdir -p "${DIR}"

# Extended key usage "codeSigning" is what Windows checks for Authenticode.
openssl req -x509 -newkey rsa:3072 -sha256 -days "${DAYS}" -nodes \
    -keyout "${KEY}" -out "${CRT}" \
    -subj "/CN=${NAME}" \
    -addext "basicConstraints=critical,CA:FALSE" \
    -addext "keyUsage=critical,digitalSignature" \
    -addext "extendedKeyUsage=critical,codeSigning" 2>/dev/null

chmod 600 "${KEY}"
chmod 644 "${CRT}"

# The certificate is valid from "now" by this machine's clock, which can run
# a few seconds ahead of the timestamp servers; signing right away would give a
# timestamp that predates the certificate ("not yet valid"). Wait it out.
sleep 15

echo "Created \"${NAME}\" in ${DIR}:"
echo "  ${KEY}   (PRIVATE, keep it here)"
echo "  ${CRT}   (public: this is the one to install on Windows)"
echo
echo "SHA-256 fingerprint: $(openssl x509 -in "${CRT}" -noout -fingerprint -sha256 | cut -d= -f2)"
echo "Valid until:         $(openssl x509 -in "${CRT}" -noout -enddate | cut -d= -f2)"
if command -v wslpath >/dev/null 2>&1; then
    echo "Windows path:        $(wslpath -w "${CRT}")"
fi
echo
echo "Next: scripts/sign-windows.sh (BM_SIGN_FILE=${FILE} if you keep several"
echo "certificates), then install the .crt on Windows"
echo "(scripts/install-dev-cert.ps1, from an administrator PowerShell)."
