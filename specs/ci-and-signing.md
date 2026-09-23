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
| Self-signed certificate | Does not satisfy Smart App Control, which only trusts CAs in the Microsoft Trusted Root Program. |
