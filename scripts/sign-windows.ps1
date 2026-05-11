<#
.SYNOPSIS
    Download unsigned Windows exe from OSS, code-sign locally, regenerate Tauri
    updater signature (.sig), upload signed artifacts to OSS final path.

.DESCRIPTION
    This script runs on the local Windows signing machine. It:
    1. Downloads the unsigned .exe from OSS staging
    2. Signs it with signtool (Windows Authenticode)
    3. Regenerates the Tauri updater .sig file (minisign via tauri CLI)
    4. Uploads signed exe + new .sig to the final OSS path

    For beta builds: uploads to aijia/beta/v{version}/
    For release builds: uploads to aijia/v{version}/

.PARAMETER Version
    Version number (e.g. 0.5.22)

.PARAMETER ReleaseType
    beta or release (default: beta)

.PARAMETER CertThumbprint
    Certificate thumbprint for signtool. If not provided, uses env SIGN_CERT_THUMBPRINT.

.PARAMETER TauriSigningKey
    Tauri updater private key (base64). If not provided, uses env TAURI_SIGNING_PRIVATE_KEY.

.PARAMETER TauriSigningPassword
    Tauri updater key password. If not provided, uses env TAURI_SIGNING_PRIVATE_KEY_PASSWORD.

.PARAMETER SkipDownload
    Skip download from OSS (use local file at WorkDir)

.EXAMPLE
    .\scripts\sign-windows.ps1 -Version 0.5.22 -ReleaseType beta
    .\scripts\sign-windows.ps1 -Version 0.5.22 -ReleaseType release
#>

param(
    [Parameter(Mandatory=$true)]
    [string]$Version,

    [ValidateSet("beta", "release")]
    [string]$ReleaseType = "beta",

    [string]$CertThumbprint = $env:SIGN_CERT_THUMBPRINT,

    [string]$TauriSigningKey = $env:TAURI_SIGNING_PRIVATE_KEY,

    [string]$TauriSigningPassword = $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD,

    [switch]$SkipDownload
)

$ErrorActionPreference = "Stop"

# --- Configuration ---
$OssBucket = "lotus-releases"
$OssEndpoint = "https://oss-cn-beijing.aliyuncs.com"
$OssPrefix = "aijia"
$ExeFilename = "AIjia_${Version}_x64-setup.exe"
$SigFilename = "${ExeFilename}.sig"

$WorkDir = Join-Path $PSScriptRoot ".." "signing-workspace"
if (-not (Test-Path $WorkDir)) { New-Item -ItemType Directory -Path $WorkDir | Out-Null }
$WorkDir = Resolve-Path $WorkDir

$ExePath = Join-Path $WorkDir $ExeFilename
$SigPath = Join-Path $WorkDir $SigFilename

# --- Validation ---
if (-not $CertThumbprint) {
    Write-Error "Certificate thumbprint required. Set -CertThumbprint or env SIGN_CERT_THUMBPRINT"
}
if (-not $TauriSigningKey) {
    Write-Error "Tauri signing key required. Set -TauriSigningKey or env TAURI_SIGNING_PRIVATE_KEY"
}

# --- Step 1: Download unsigned exe from OSS staging ---
if (-not $SkipDownload) {
    Write-Host "`n=== Step 1: Download unsigned exe from OSS ===" -ForegroundColor Cyan

    $OssKeyId = $env:OSS_ACCESS_KEY_ID
    $OssKeySecret = $env:OSS_ACCESS_KEY_SECRET
    if (-not $OssKeyId -or -not $OssKeySecret) {
        Write-Error "OSS_ACCESS_KEY_ID and OSS_ACCESS_KEY_SECRET required"
    }

    $StagingKey = "$OssPrefix/staging/$ReleaseType/v$Version/$($ExeFilename -replace '\.exe$', '.unsigned.exe')"

    # Use Python + oss2 for download (consistent with other scripts)
    $downloadScript = @"
import oss2, os, sys
auth = oss2.Auth(os.environ['OSS_ACCESS_KEY_ID'], os.environ['OSS_ACCESS_KEY_SECRET'])
bucket = oss2.Bucket(auth, '$OssEndpoint', '$OssBucket')
key = '$StagingKey'
local = r'$ExePath'
print(f'Downloading {key} ...')
bucket.get_object_to_file(key, local)
size = os.path.getsize(local) / 1024 / 1024
print(f'Downloaded: {size:.1f}MB')
"@
    python -c $downloadScript
    if ($LASTEXITCODE -ne 0) { throw "Download failed" }
} else {
    Write-Host "`n=== Step 1: Using local file (skip download) ===" -ForegroundColor Cyan
    if (-not (Test-Path $ExePath)) {
        Write-Error "File not found: $ExePath"
    }
}

# --- Step 2: Code sign with signtool ---
Write-Host "`n=== Step 2: Windows Authenticode signing ===" -ForegroundColor Cyan

# Find signtool
$SignTool = Get-ChildItem "C:\Program Files (x86)\Windows Kits\10\bin" -Recurse -Filter "signtool.exe" |
    Sort-Object { $_.Directory.Name } -Descending |
    Select-Object -First 1 -ExpandProperty FullName

if (-not $SignTool) {
    # Try PATH
    $SignTool = Get-Command signtool.exe -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Source
}
if (-not $SignTool) {
    Write-Error "signtool.exe not found. Install Windows SDK."
}

Write-Host "Using signtool: $SignTool"
Write-Host "Signing: $ExePath"
Write-Host "Certificate: $CertThumbprint"

& $SignTool sign /sha1 $CertThumbprint /tr http://timestamp.digicert.com /td sha256 /fd sha256 "$ExePath"
if ($LASTEXITCODE -ne 0) { throw "Code signing failed" }

# Verify signature
& $SignTool verify /pa "$ExePath"
if ($LASTEXITCODE -ne 0) { throw "Signature verification failed" }

Write-Host "Code signing successful!" -ForegroundColor Green

# --- Step 3: Regenerate Tauri updater .sig ---
Write-Host "`n=== Step 3: Regenerate Tauri updater signature ===" -ForegroundColor Cyan

# Use tauri CLI signer (npx or cargo)
$env:TAURI_SIGNING_PRIVATE_KEY = $TauriSigningKey
if ($TauriSigningPassword) {
    $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = $TauriSigningPassword
}

# Try multiple methods to run tauri signer
$signed = $false

# Method 1: cargo-tauri (if Rust toolchain installed)
$tauriCli = Get-Command "cargo-tauri" -ErrorAction SilentlyContinue
if ($tauriCli) {
    Write-Host "Using cargo-tauri signer"
    & cargo-tauri signer sign "$ExePath"
    if ($LASTEXITCODE -eq 0) { $signed = $true }
}

# Method 2: npx (if node_modules available in project)
if (-not $signed) {
    $projectDir = Join-Path $PSScriptRoot ".."
    $npxTauri = Join-Path $projectDir "node_modules" ".bin" "tauri.cmd"
    if (Test-Path $npxTauri) {
        Write-Host "Using local node_modules tauri CLI"
        Push-Location $projectDir
        & $npxTauri signer sign "$ExePath"
        if ($LASTEXITCODE -eq 0) { $signed = $true }
        Pop-Location
    }
}

# Method 3: npx global (will download if needed)
if (-not $signed) {
    $npx = Get-Command "npx" -ErrorAction SilentlyContinue
    if ($npx) {
        Write-Host "Using npx to download @tauri-apps/cli (may take a moment)..."
        $projectDir = Join-Path $PSScriptRoot ".."
        Push-Location $projectDir
        npx @tauri-apps/cli signer sign "$ExePath"
        if ($LASTEXITCODE -eq 0) { $signed = $true }
        Pop-Location
    }
}

if (-not $signed) {
    Write-Error @"
Tauri signer not available. Install one of:
  1. cargo install tauri-cli (Rust)
  2. Run 'pnpm install' in project root (Node.js)
  3. Install Node.js (npx will auto-download @tauri-apps/cli)
"@
}

# The signer creates {file}.sig next to the file
if (-not (Test-Path $SigPath)) {
    Write-Error ".sig file not created at: $SigPath"
}

Write-Host "Updater signature regenerated!" -ForegroundColor Green

# --- Step 4: Upload signed artifacts to OSS ---
Write-Host "`n=== Step 4: Upload signed artifacts to OSS ===" -ForegroundColor Cyan

if ($ReleaseType -eq "beta") {
    $FinalPrefix = "$OssPrefix/beta/v$Version"
    # OSS-side filename gets a -beta suffix so users can tell beta vs.
    # release apart in Downloads. Local working filename is unchanged.
    $UploadExeName = "AIjia_${Version}-beta_x64-setup.exe"
    $UploadSigName = "${UploadExeName}.sig"
} else {
    $FinalPrefix = "$OssPrefix/v$Version"
    $UploadExeName = $ExeFilename
    $UploadSigName = $SigFilename
}

$uploadScript = @"
import oss2, os, sys

auth = oss2.Auth(os.environ['OSS_ACCESS_KEY_ID'], os.environ['OSS_ACCESS_KEY_SECRET'])
bucket = oss2.Bucket(auth, '$OssEndpoint', '$OssBucket')

exe_local = r'$ExePath'
sig_local = r'$SigPath'
exe_key = '$FinalPrefix/$UploadExeName'
sig_key = '$FinalPrefix/$UploadSigName'

print(f'Uploading signed exe: {exe_key}')
oss2.resumable_upload(bucket, exe_key, exe_local,
    multipart_threshold=10*1024*1024, part_size=5*1024*1024, num_threads=4)

print(f'Uploading signature: {sig_key}')
bucket.put_object_from_file(sig_key, sig_local)

# For release builds, also update the latest symlink
release_type = '$ReleaseType'
if release_type == 'release':
    latest_key = '$OssPrefix/latest/windows-x64'
    bucket.copy_object('$OssBucket', exe_key, latest_key, headers={
        'x-oss-metadata-directive': 'REPLACE',
        'Content-Type': 'application/octet-stream',
        'Content-Disposition': f'attachment; filename="$ExeFilename"',
    })
    print(f'  -> latest: {latest_key}')

print(f'\n[ok] Windows v$Version ({release_type}) signed and uploaded!')
"@

python -c $uploadScript
if ($LASTEXITCODE -ne 0) { throw "Upload failed" }

# --- Done ---
Write-Host "`n$('='*60)" -ForegroundColor Green
Write-Host "  DONE! Windows v$Version ($ReleaseType) signed and uploaded." -ForegroundColor Green
Write-Host "$('='*60)" -ForegroundColor Green

if ($ReleaseType -eq "release") {
    Write-Host "`nNext step: Run 'Finalize Release' workflow on GitHub"
    Write-Host "  -> Actions -> Finalize Release -> Run workflow -> version: $Version"
} elseif ($ReleaseType -eq "beta") {
    Write-Host "`nBeta available at:"
    Write-Host "  https://lotus.renlijia.com/$OssPrefix/beta/v$Version/$ExeFilename"
}
