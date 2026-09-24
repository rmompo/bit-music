# Installs (or removes) the development code-signing certificate on this
# Windows machine. Run it from an ADMINISTRATOR PowerShell.
#
#   .\scripts\install-dev-cert.ps1 -CertPath C:\path\to\kangaroo-development.crt
#   .\scripts\install-dev-cert.ps1 -Remove -CertPath C:\path\to\kangaroo-development.crt
#
# This is a security decision: the certificate is added to "Trusted Root
# Certification Authorities" and "Trusted Publishers" of the whole machine,
# so anything signed with its private key will be trusted here. Keep that key
# private (see scripts/make-dev-cert.sh) and remove the certificate when you
# no longer need it. With it installed, Smart App Control lets executables signed
# by scripts/sign-windows.sh run (see specs/ci-and-signing.md).

param(
    [Parameter(Mandatory = $true)][string]$CertPath,
    [switch]$Remove
)

$ErrorActionPreference = "Stop"

$admin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()
    ).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $admin) {
    throw "Run this from an administrator PowerShell."
}

$cert = New-Object System.Security.Cryptography.X509Certificates.X509Certificate2 (Resolve-Path $CertPath).Path
$stores = @("Cert:\LocalMachine\Root", "Cert:\LocalMachine\TrustedPublisher")

if ($Remove) {
    foreach ($store in $stores) {
        Get-ChildItem $store | Where-Object { $_.Thumbprint -eq $cert.Thumbprint } | Remove-Item
    }
    Write-Host "Removed $($cert.Subject) ($($cert.Thumbprint))."
    return
}

Write-Host "Certificate: $($cert.Subject)"
Write-Host "Thumbprint:  $($cert.Thumbprint)"
foreach ($store in $stores) {
    Import-Certificate -FilePath $CertPath -CertStoreLocation $store | Out-Null
    Write-Host "Installed in $store"
}
Write-Host ""
Write-Host "Check a signed file with:  Get-AuthenticodeSignature .\gui-player.exe | Format-List"
