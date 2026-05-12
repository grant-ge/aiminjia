# Sign Windows exe (after CI build) and upload to OSS.
#
# Run this on the Windows build machine AFTER:
#   1. CI build-desktop workflow completes
#   2. You have signed the exe with your EV certificate signing tool
#
# Usage:
#   .\scripts\sign-and-upload-windows.ps1 <version> <release|beta>
#
# Example:
#   .\scripts\sign-and-upload-windows.ps1 0.5.22 beta
#
# Prerequisites:
#   - The exe has already been signed with EV cert (via vendor signing tool)
#   - TAURI_SIGNING_PRIVATE_KEY and TAURI_SIGNING_PRIVATE_KEY_PASSWORD env vars set
#   - OSS_ACCESS_KEY_ID and OSS_ACCESS_KEY_SECRET env vars set
#   - Python with oss2 installed: pip install oss2

$ErrorActionPreference = "Stop"

if ($args.Count -lt 2) {
    Write-Host "Usage: .\scripts\sign-and-upload-windows.ps1 <version> <release|beta>"
    Write-Host "Example: .\scripts\sign-and-upload-windows.ps1 0.5.22 beta"
    exit 1
}

$version = $args[0]
$releaseType = $args[1]
$ProjectDir = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
$ExePath = Join-Path $ProjectDir "src-tauri\target\release\bundle\nsis\AIjia_${version}_x64-setup.exe"
$SigPath = "${ExePath}.sig"

if (-not (Test-Path $ExePath)) {
    Write-Host "ERROR: Exe not found: $ExePath" -ForegroundColor Red
    Write-Host "Make sure the CI build has completed first."
    exit 1
}

# ── 1. Verify signature ──
Write-Host "=== Step 1: Verify Authenticode signature ===" -ForegroundColor Cyan
$sig = Get-AuthenticodeSignature $ExePath
if ($sig.Status -eq "Valid") {
    Write-Host "  Signature valid: $($sig.SignerCertificate.Subject)" -ForegroundColor Green
} else {
    Write-Host "  WARNING: Exe is NOT signed or signature invalid (status: $($sig.Status))" -ForegroundColor Yellow
    Write-Host "  Please sign the exe first with your EV certificate signing tool."
    Write-Host "  Exe path: $ExePath"
    $confirm = Read-Host "  Continue anyway? (y/N)"
    if ($confirm -ne "y") { exit 1 }
}

# ── 2. Regenerate Tauri updater .sig ──
Write-Host ""
Write-Host "=== Step 2: Regenerate Tauri updater signature ===" -ForegroundColor Cyan
if (-not $env:TAURI_SIGNING_PRIVATE_KEY) {
    Write-Host "ERROR: TAURI_SIGNING_PRIVATE_KEY not set" -ForegroundColor Red
    exit 1
}
if (Test-Path $SigPath) { Remove-Item $SigPath }
Push-Location $ProjectDir
pnpm exec tauri signer sign "$ExePath"
$exitCode = $LASTEXITCODE
Pop-Location
if ($exitCode -ne 0) { throw "Tauri signer failed" }
if (-not (Test-Path $SigPath)) { throw ".sig not created at $SigPath" }
Write-Host "  Updater signature regenerated" -ForegroundColor Green

# ── 3. Upload to OSS ──
Write-Host ""
Write-Host "=== Step 3: Upload to OSS ===" -ForegroundColor Cyan
if (-not $env:OSS_ACCESS_KEY_ID -or -not $env:OSS_ACCESS_KEY_SECRET) {
    Write-Host "ERROR: OSS_ACCESS_KEY_ID / OSS_ACCESS_KEY_SECRET not set" -ForegroundColor Red
    exit 1
}
$env:PYTHONUTF8 = "1"
$env:PYTHONIOENCODING = "utf-8"
python --version
if ($LASTEXITCODE) { throw "python not available" }
python -m pip install --disable-pip-version-check oss2
if ($LASTEXITCODE) { throw "pip install oss2 failed" }
python (Join-Path $ProjectDir "scripts\ci-upload-windows.py") $version $releaseType
if ($LASTEXITCODE) { throw "Upload failed (exit $LASTEXITCODE)" }

Write-Host ""
Write-Host "=== Done ===" -ForegroundColor Green
Write-Host "Windows v$version ($releaseType) signed and uploaded to OSS."
