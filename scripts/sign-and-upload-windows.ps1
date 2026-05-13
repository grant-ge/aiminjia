# Windows post-build: verify Authenticode signature, generate Tauri updater
# signature, and upload to OSS.
#
# Workflow:
#   1. CI builds unsigned exe on GitHub-hosted runner
#   2. Download "windows-installers" artifact from GitHub Actions
#   3. Sign the exe with SimpleSign (EV certificate)
#   4. Run this script
#
# Usage:
#   .\scripts\sign-and-upload-windows.ps1 <version> <release|beta> [exe-path]
#
# Examples:
#   .\scripts\sign-and-upload-windows.ps1 0.5.22 beta
#   .\scripts\sign-and-upload-windows.ps1 0.5.22 beta C:\Downloads\AIjia_0.5.22_x64-setup.exe
#
# Prerequisites:
#   - Node.js (for npx @tauri-apps/cli signer)
#   - TAURI_SIGNING_PRIVATE_KEY and TAURI_SIGNING_PRIVATE_KEY_PASSWORD env vars
#   - OSS_ACCESS_KEY_ID and OSS_ACCESS_KEY_SECRET env vars
#   - Python with oss2: pip install oss2

$ErrorActionPreference = "Stop"

if ($args.Count -lt 2) {
    Write-Host "Usage: .\scripts\sign-and-upload-windows.ps1 <version> <release|beta> [exe-path]"
    Write-Host ""
    Write-Host "Examples:"
    Write-Host "  .\scripts\sign-and-upload-windows.ps1 0.5.22 beta"
    Write-Host "  .\scripts\sign-and-upload-windows.ps1 0.5.22 beta C:\Downloads\AIjia_0.5.22_x64-setup.exe"
    exit 1
}

$version = $args[0]
$releaseType = $args[1]
$ExeName = "AIjia_${version}_x64-setup.exe"

# Find exe: explicit path > current dir > Downloads
if ($args.Count -ge 3) {
    $ExePath = $args[2]
} elseif (Test-Path $ExeName) {
    $ExePath = Resolve-Path $ExeName
} elseif (Test-Path (Join-Path $env:USERPROFILE "Downloads\$ExeName")) {
    $ExePath = Join-Path $env:USERPROFILE "Downloads\$ExeName"
} else {
    Write-Host "ERROR: Cannot find $ExeName" -ForegroundColor Red
    Write-Host "  Looked in: current dir, ~/Downloads"
    Write-Host "  Or specify path: .\scripts\sign-and-upload-windows.ps1 $version $releaseType C:\path\to\$ExeName"
    exit 1
}

$ExePath = [System.IO.Path]::GetFullPath($ExePath)
$SigPath = "${ExePath}.sig"
Write-Host "Exe: $ExePath"
Write-Host ""

# ── 1. Verify Authenticode signature ──
Write-Host "=== Step 1: Verify Authenticode signature ===" -ForegroundColor Cyan
$sig = Get-AuthenticodeSignature $ExePath
if ($sig.Status -eq "Valid") {
    Write-Host "  Signature valid: $($sig.SignerCertificate.Subject)" -ForegroundColor Green
} else {
    Write-Host "  WARNING: Exe is NOT signed (status: $($sig.Status))" -ForegroundColor Yellow
    Write-Host "  Sign it with SimpleSign first, then re-run this script."
    $confirm = Read-Host "  Continue anyway? (y/N)"
    if ($confirm -ne "y") { exit 1 }
}

# ── 2. Generate Tauri updater signature (.sig) ──
Write-Host ""
Write-Host "=== Step 2: Generate Tauri updater signature ===" -ForegroundColor Cyan
if (-not $env:TAURI_SIGNING_PRIVATE_KEY) {
    Write-Host "ERROR: TAURI_SIGNING_PRIVATE_KEY env var not set" -ForegroundColor Red
    Write-Host '  Set it: $env:TAURI_SIGNING_PRIVATE_KEY = "<base64-key>"'
    exit 1
}
if (Test-Path $SigPath) { Remove-Item $SigPath }

# Prefer globally installed tauri-cli, fallback to npx
if (Get-Command tauri -ErrorAction SilentlyContinue) {
    Write-Host "  Using global tauri-cli"
    tauri signer sign "$ExePath"
} else {
    Write-Host "  Using npx @tauri-apps/cli (tip: npm install -g @tauri-apps/cli for faster runs)"
    npx --yes @tauri-apps/cli@latest signer sign "$ExePath"
}
if ($LASTEXITCODE -ne 0) { throw "Tauri signer failed" }
if (-not (Test-Path $SigPath)) { throw ".sig not created at $SigPath" }
Write-Host "  Updater signature created: $SigPath" -ForegroundColor Green

# ── 3. Upload to OSS ──
Write-Host ""
Write-Host "=== Step 3: Upload to OSS ===" -ForegroundColor Cyan
if (-not $env:OSS_ACCESS_KEY_ID -or -not $env:OSS_ACCESS_KEY_SECRET) {
    Write-Host "ERROR: OSS_ACCESS_KEY_ID / OSS_ACCESS_KEY_SECRET env vars not set" -ForegroundColor Red
    exit 1
}

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$UploadScript = Join-Path $ScriptDir "ci-upload-windows.py"

# If running outside the repo, use inline upload
if (Test-Path $UploadScript) {
    $env:PYTHONUTF8 = "1"
    python -m pip install --disable-pip-version-check oss2 2>$null
    python $UploadScript $version $releaseType
    if ($LASTEXITCODE) { throw "Upload failed" }
} else {
    Write-Host "  ci-upload-windows.py not found, using inline upload..." -ForegroundColor Yellow
    $env:PYTHONUTF8 = "1"
    python -m pip install --disable-pip-version-check oss2 2>$null
    $pyScript = @"
import os, sys, oss2
version, release_type = sys.argv[1], sys.argv[2]
auth = oss2.Auth(os.environ['OSS_ACCESS_KEY_ID'], os.environ['OSS_ACCESS_KEY_SECRET'])
bucket = oss2.Bucket(auth, 'https://oss-cn-beijing.aliyuncs.com', 'lotus-releases')
prefix = f'aijia/{"beta/" if release_type == "beta" else ""}v{version}'
for ext in ['', '.sig']:
    local = r'$($ExePath.Replace("'","''"))' + ext
    key = f'{prefix}/AIjia_{version}_x64-setup.exe{ext}'
    if not os.path.exists(local):
        print(f'[skip] {local} not found')
        continue
    size = os.path.getsize(local) / 1024 / 1024
    print(f'[upload] {os.path.basename(local)} ({size:.1f}MB) -> {key}')
    if ext == '':
        oss2.resumable_upload(bucket, key, local, multipart_threshold=10*1024*1024, part_size=5*1024*1024, num_threads=4)
    else:
        bucket.put_object_from_file(key, local)
if release_type == 'release':
    exe_key = f'{prefix}/AIjia_{version}_x64-setup.exe'
    bucket.copy_object('lotus-releases', exe_key, f'aijia/latest/windows-x64',
        headers={'x-oss-metadata-directive':'REPLACE','Content-Type':'application/octet-stream',
                 'Content-Disposition':f'attachment; filename="AIjia_{version}_x64-setup.exe"'})
    print('  -> latest updated')
print(f'\n[ok] Windows v{version} ({release_type}) uploaded')
"@
    python -c $pyScript $version $releaseType
    if ($LASTEXITCODE) { throw "Upload failed" }
}

Write-Host ""
Write-Host "=== Done ===" -ForegroundColor Green
Write-Host "Windows v$version ($releaseType) signed and uploaded to OSS."
