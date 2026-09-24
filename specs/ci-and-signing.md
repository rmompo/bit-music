# CI, Windows builds and code signing

Status: **there is no CI workflow in the repository at the moment.** It was
built, validated and run successfully once, then removed on purpose: for
day-to-day use it is not needed (see [Running unsigned builds
locally](#running-unsigned-builds-locally)). This document keeps everything
needed to bring it back, and the steps that lead to signed Windows binaries.

## Why it matters: Windows blocks unsigned programs

On Windows 11, **Smart App Control** blocks executables that are neither signed
with a certificate from a CA in the Microsoft Trusted Root Program nor known to
Microsoft's reputation service. A freshly built `.exe` is neither, so it fails
with *"An App Control policy blocked this file"*. Microsoft's FAQ states there is
no per-app exception; the answer for developers is to sign the app.

CI does **not** sign anything. It only produces the binaries in an automated,
verifiable way, which is the prerequisite for free signing through SignPath.
The full chain is: CI build -> signing service -> signed `.exe`.

## Running unsigned builds locally

For development on your own machine, signing is not needed:

- **Developer Mode** (`Win+R` -> `ms-settings:developers` -> *Developer Mode*):
  according to Microsoft's FAQ, Smart App Control is turned off when developer
  mode is configured. This was verified to let the unsigned `.exe` run.
- Alternatively, turn Smart App Control off in *Windows Security > App & browser
  control > Smart App Control settings*. Recent Windows updates allow turning it
  back on without reinstalling Windows.
- Launching the `.exe` from WSL also worked in tests, without changing settings,
  but that is not guaranteed to keep working.
- Neither is Developer Mode: Smart App Control can get switched on again (an
  update, or someone re-enabling it) and then it blocks unsigned builds even
  with Developer Mode on. If that happens, sign your own builds with a
  self-signed certificate ([see below](#signing-for-development-with-a-self-signed-certificate))
  instead of turning Smart App Control off.

To check the state (read-only), from PowerShell:

```powershell
# 0 = off, 1 = on, 2 = evaluation
(Get-ItemProperty 'HKLM:\SYSTEM\CurrentControlSet\Control\CI\Policy').VerifiedAndReputablePolicyState
```

Blocks are also logged in *Event Viewer > Applications and Services Logs >
Microsoft > Windows > CodeIntegrity > Operational* (event id 3077).

## What the CI did

Three chained jobs, triggered by pushes to `main`/`develop`, pull requests,
manual runs, and `v*` tags:

1. **Test (Linux)**: `cargo test --workspace --locked` (needs `pkg-config` and
   `libasound2-dev`, because `cpal` builds against ALSA on Linux).
2. **Build (Windows x86_64)**, only if the tests pass: cross-compiles `bm` and
   `gui-player` for `x86_64-pc-windows-gnu` with MinGW (the same chain as
   `scripts/build-windows.sh`), packages them with `scripts/package-windows.sh`
   and uploads the zip as an artifact.
3. **Release**, only for `v*` tags: attaches the zip to a GitHub release.

The one recorded run (2026-09-23, commit `96dffc8`) passed on the first try:
tests about 2 minutes, Windows build about 4.5 minutes, a 6.9 MB artifact. The
workflow was also checked with `actionlint` (no findings).

## How to recreate it

1. Create `.github/workflows/ci.yml` with the file below.
2. Keep `scripts/package-windows.sh` (it stays in the repository and also works
   locally; it needs `zip`).
3. Push. The first run takes longer because nothing is cached.
4. To validate the YAML before pushing, run `actionlint` (a single Go binary
   from <https://github.com/rhysd/actionlint>).

Things worth knowing:

- The Windows build relies on `Cargo.lock` pinning `gpu-allocator` to the same
  `windows` crate version `wgpu-hal` uses; otherwise the Windows target fails to
  compile (see `gui-player/specs/layout.md`, *Framework and platform notes*).
  That is why the workflow builds with `--locked`.
- Actions are referenced by major version tags (`@v4`, `@v2`,
  `@stable`); update them when they age.
- The workflow uses least-privilege `permissions`; only the release job gets
  `contents: write`.
- Artifacts expire (90 days by default). They and the runs can be deleted from
  the repository's *Actions* tab.

### `.github/workflows/ci.yml`

```yaml
name: CI

on:
  push:
    branches: [main, develop]
    tags: ["v*"]
  pull_request:
  workflow_dispatch:

# Least privilege by default; the release job asks for more where it needs it.
permissions:
  contents: read

concurrency:
  group: ci-${{ github.ref }}
  cancel-in-progress: true

env:
  CARGO_TERM_COLOR: always

jobs:
  test:
    name: Test (Linux)
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      # cpal needs the ALSA headers to build on Linux.
      - name: Install system libraries
        run: sudo apt-get update && sudo apt-get install -y pkg-config libasound2-dev

      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2

      - name: Run the whole test suite
        run: cargo test --workspace --locked

  windows:
    name: Build (Windows x86_64)
    runs-on: ubuntu-latest
    # Only produce binaries from a revision whose tests pass.
    needs: test
    steps:
      - uses: actions/checkout@v4

      # Same cross-compilation chain as scripts/build-windows.sh.
      - name: Install the MinGW toolchain and zip
        run: sudo apt-get update && sudo apt-get install -y mingw-w64 zip

      - uses: dtolnay/rust-toolchain@stable
        with:
          targets: x86_64-pc-windows-gnu
      - uses: Swatinem/rust-cache@v2
        with:
          key: windows-gnu

      - name: Build bm and gui-player
        run: cargo build --release --locked --target x86_64-pc-windows-gnu -p bm -p gui-player

      - name: Package
        run: scripts/package-windows.sh "$GITHUB_REF_NAME"

      - uses: actions/upload-artifact@v4
        with:
          name: bit-music-windows-x86_64
          path: dist/*.zip
          if-no-files-found: error

  release:
    name: Release
    # Only for version tags (v0.1.0, ...).
    if: startsWith(github.ref, 'refs/tags/v')
    needs: windows
    runs-on: ubuntu-latest
    permissions:
      contents: write
    steps:
      - uses: actions/download-artifact@v4
        with:
          name: bit-music-windows-x86_64
          path: dist

      # NOTE: these binaries are not code-signed yet. Code signing (SignPath)
      # goes between the download above and the publishing step below.
      - uses: softprops/action-gh-release@v2
        with:
          files: dist/*.zip
          generate_release_notes: true
```

### `scripts/package-windows.sh` (kept in the repository)

```bash
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
```

## Getting signed binaries (SignPath Foundation)

SignPath Foundation offers free code signing for qualifying open source
projects. Conditions, from <https://signpath.org/terms.html> (check the page for
the current wording before applying):

- An OSI-approved open source license, without commercial dual-licensing for any
  component. *(This project: `MIT OR Apache-2.0`.)*
- No proprietary or non-open-source components. *(The demo samples are
  synthesized from code for this reason; see `demos/samples/README.md`.)*
- No malware or potentially unwanted programs, and no hacking tools.
- Actively maintained, and **already released in the form that will be signed**.
- Functionality described on the download page or app store entry.
- Binaries built from source in a verifiable, automated way (**this is what the
  CI provides**), with manual approval of every release for signing.
- A published *code signing policy* that includes the sentence *"Free code
  signing provided by SignPath.io, certificate by SignPath Foundation"* and the
  team roles.
- Multi-factor authentication for every team member on SignPath and on the
  source repository.

Steps:

1. Turn on multi-factor authentication on the GitHub account.
2. Recreate the CI (above) and publish a first release with a `v*` tag, so there
   is a released artifact in the form to be signed.
3. Add a download section and the code signing policy to the README.
4. Apply through <https://signpath.org> (there is human review; the timeline is
   not predictable).
5. Once approved, add the signing steps to the workflow's release path: upload
   the unsigned artifact, submit a signing request to SignPath, wait for the
   signed result, and publish that instead of the unsigned zip. The exact action
   and its inputs come from SignPath's documentation at that time and were not
   tried here. Every release then needs manual approval in SignPath.

### Other options considered

| Option | Notes |
|---|---|
| Azure Artifact Signing (formerly Trusted Signing) | Microsoft's managed service. Per its documentation, individual developers must be in the United States or Canada; organizations can be in the US, Canada, the EU, the UK and a few other regions. Not available to an individual in Spain. |
| Commercial code signing certificate (OV/EV) | From a CA in the Microsoft Trusted Root Program; the private key must be kept in hardware or a cloud HSM. Certum offers certificates for individuals and an open source variant that cannot be used for commercially distributed software. Check current prices and conditions. |
| Self-signed certificate | **Works on a machine that trusts it**: verified on Windows 11 with Smart App Control on (see the experiment below). It does nothing for other people's machines, which do not trust your certificate, so it is for development, not for distribution. |

## Signing for development with a self-signed certificate

**Verified on Windows 11 with Smart App Control on:** executables signed with
your own certificate run on a machine that trusts that certificate, without
turning Smart App Control off. It is the recommended way to run your own builds
on your own machine. It does **not** help anyone else: other machines do not
trust your certificate, so for distribution the route is still SignPath (below).

Everything is scripted in `scripts/`. The private key lives outside the
repository and never goes to Windows; only the public certificate does.

### 1. Requirements (Linux / WSL)

```bash
sudo apt-get install -y osslsigncode      # openssl is normally already there
```

### 2. Create the certificate: `scripts/make-dev-cert.sh`

```bash
scripts/make-dev-cert.sh
```

It asks two things; press Enter to accept the default of each:

| Question | Default | What it is |
|---|---|---|
| Certificate name | `kangaroo (development)` | What Windows shows as the publisher and what you look for in the certificate manager to find or remove it. |
| File name, without extension | `kangaroo-development` | Base name of `<name>.key` and `<name>.crt`. |

`BM_SIGN_NAME` and `BM_SIGN_FILE` skip the questions (without a terminal the
defaults are used). The name cannot contain `/ = , + \`; the file name only
letters, digits, `.`, `_` and `-`. Then it prints the SHA-256 fingerprint, the
expiry date (three years) and the Windows path of the `.crt`.

- Files go to `~/.bit-music-signing/` (override with `BM_SIGN_DIR`), the key with
  permissions for you only. `.gitignore` also excludes `*.key`, `*.pfx`, `*.p12`
  and `*.crt`. **Never commit or share the key**: whoever has it can sign
  programs that a machine trusting the certificate will accept.
- The script waits 15 seconds at the end: this machine's clock can be a few
  seconds ahead of the timestamp servers, and signing right away would give a
  timestamp older than the certificate ("not yet valid").
- It refuses to overwrite an existing certificate unless given `--force`.

### 3. Sign the executables: `scripts/sign-windows.sh`

```bash
scripts/build-windows.sh gui-player      # and/or: scripts/build-windows.sh
scripts/sign-windows.sh                   # or: scripts/sign-windows.sh some.exe
```

or both in one step with `scripts/build-signed-windows.sh [package ...]`
(default: `bm` and `gui-player`). Close a running copy of the program first:
Windows does not let its file be replaced.

It signs `bm.exe` and `gui-player.exe` (or the files given) with a timestamp
from a public server (so the signature outlives the certificate) and verifies
them. The signed copies go to **`dist/signed/`**; the originals in `target/` are
not touched. If several certificates exist, choose one with
`BM_SIGN_FILE=<base name>`.

### 4. Install the certificate on Windows (once)

Copy or reach the public `.crt` from Windows (for example
`dist\signed\<name>.crt`, which is easy to find; the path is printed in step 2).
It must go into **two stores of the local machine**: *Trusted Root Certification
Authorities* and *Trusted Publishers*. This is a security decision about that
machine: while installed, anything signed with the private key is trusted there.

**With windows (certificate manager)**

1. `Win + R`, type `certlm.msc`, Enter, accept the administrator prompt. (The
   title must read *Certificates - Local Computer*; `certmgr.msc` is the
   per-user store and does not work.)
2. Right-click **Trusted Root Certification Authorities** > *All Tasks* >
   *Import...* (or the *Certificates* subfolder if it is shown).
3. In the wizard: *Next* > *Browse...* > set the file-type filter to *All files*
   > pick the `.crt` > *Next* > keep the *Trusted Root Certification
   Authorities* store > *Finish*. Windows warns that you are installing a root
   certificate: answer *Yes*.
4. Repeat on **Trusted Publishers** with the same file. If it shows no
   *Certificates* subfolder (empty stores often do not), import from the store
   folder itself; reopen the console afterwards to see it.

**From the console** (PowerShell as administrator)

```powershell
$c = "C:\path\to\kangaroo-development.crt"
Import-Certificate -FilePath $c -CertStoreLocation Cert:\LocalMachine\Root
Import-Certificate -FilePath $c -CertStoreLocation Cert:\LocalMachine\TrustedPublisher
```

or, from the repository, `scripts\install-dev-cert.ps1 -CertPath <the .crt>`
(it must run as administrator; it installs in both stores and prints the
thumbprint).

**Check it.** In `certlm.msc`, both folders should list a certificate issued to
and by `kangaroo (development)` (self-signed, so both match). From PowerShell:

```powershell
Get-ChildItem Cert:\LocalMachine\Root, Cert:\LocalMachine\TrustedPublisher |
  Where-Object Subject -like "*kangaroo*"
Get-AuthenticodeSignature .\dist\signed\gui-player.exe | Format-List   # Status: Valid
```

Or with windows: right-click the signed `.exe` > *Properties* > *Digital
Signatures* > select the entry > *Details*: "This digital signature is OK".
Before installing, Windows says the chain ends in a root that is not trusted.

### 5. Run the signed copies

Run the files in **`dist\signed\`**, not the ones in `target\`, which are
unsigned and stay blocked. A shortcut or a pinned taskbar entry that points at
`target\` keeps being blocked.

### 6. If a signed file is blocked anyway

Sign again (`scripts/sign-windows.sh`) and run the new copy. In our test one
signed `bm.exe` kept being rejected (also when copied elsewhere or renamed),
while a fresh signing of the same program, and the signed `gui-player.exe`, ran
fine; the Code Integrity log shows Smart App Control consulting Defender's
cloud service and a per-file cache, so a rejection seems to stick to that exact
file. This is a hypothesis; the fix that worked is simply to sign again.

To see why something was blocked (read-only), in PowerShell:

```powershell
Get-WinEvent -LogName "Microsoft-Windows-CodeIntegrity/Operational" -MaxEvents 20 |
  Where-Object Id -eq 3077 | Format-List TimeCreated, Message
```

The message names the blocked file and the process that tried to start it.

### 7. Remove it

When you no longer need it: in `certlm.msc`, right-click the
`kangaroo (development)` entry in **each** of the two folders > *Delete*; or

```powershell
Get-ChildItem Cert:\LocalMachine\Root, Cert:\LocalMachine\TrustedPublisher |
  Where-Object Thumbprint -eq "<the thumbprint>" | Remove-Item
```

(or `scripts\install-dev-cert.ps1 -Remove -CertPath <the .crt>`). Executables
signed with it then go back to being untrusted here. The certificate expires
after three years; to renew, run `scripts/make-dev-cert.sh --force`, sign again
and reinstall it (removing the old one).
