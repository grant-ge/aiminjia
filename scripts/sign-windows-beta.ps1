# One-shot Windows beta/release helper.
#
# Downloads the unsigned x64 setup from the OSS staging area, pauses so you can
# Authenticode-sign the .exe with SimpleSign (GUI), then runs the standard
# sign-and-upload-windows.ps1 flow to generate the Tauri .sig and upload to
# the public OSS path.
#
# Usage (from anywhere; uses repo at $PSScriptRoot's parent):
#   .\scripts\sign-windows-beta.ps1 -Version 0.5.23-beta.1 -Type beta
#   .\scripts\sign-windows-beta.ps1 -Version 0.5.23 -Type release
#
# Prerequisites:
#   - $env:TAURI_SIGNING_PRIVATE_KEY  (contents of ~/.tauri/aijia.key)
#   - $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD
#   - $env:OSS_ACCESS_KEY_ID / OSS_ACCESS_KEY_SECRET
#   - Python with `oss2` for the upload step (the inner script handles that)
#   - SimpleSign installed + EV hardware token plugged in
#
# This script does NOT call SimpleSign — sign the .exe in its GUI when prompted.

param(
    [Parameter(Mandatory=$true)][string]$Version,
    [Parameter(Mandatory=$true)][ValidateSet('beta','release')][string]$Type
)

$ErrorActionPreference = 'Stop'

# Resolve repo root (this script lives in <repo>/scripts/)
$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$RepoRoot  = Split-Path -Parent $ScriptDir
Set-Location $RepoRoot

$ExeName  = "AIjia_${Version}_x64-setup.exe"
$WorkDir  = Join-Path $RepoRoot 'build\windows-unsigned'
$ExePath  = Join-Path $WorkDir $ExeName
$SigPath  = "$ExePath.sig"

New-Item -ItemType Directory -Force -Path $WorkDir | Out-Null

$StagingBase = "https://lotus.renlijia.com/aijia/staging/unsigned/v$Version"
$ExeUrl = "$StagingBase/$ExeName"
$SigUrl = "$StagingBase/$ExeName.sig"

Write-Host "================================================================" -ForegroundColor Cyan
Write-Host " AIjia Windows $Version ($Type) — download + sign + upload" -ForegroundColor Cyan
Write-Host "================================================================" -ForegroundColor Cyan
Write-Host ""

# ── 0. Env sanity check ──
$missing = @()
foreach ($v in 'TAURI_SIGNING_PRIVATE_KEY','TAURI_SIGNING_PRIVATE_KEY_PASSWORD','OSS_ACCESS_KEY_ID','OSS_ACCESS_KEY_SECRET') {
    if (-not (Get-Item env:$v -ErrorAction SilentlyContinue)) { $missing += $v }
}
if ($missing.Count -gt 0) {
    Write-Host "ERROR: missing env vars: $($missing -join ', ')" -ForegroundColor Red
    Write-Host "Set them in this PowerShell session, e.g.:"
    Write-Host '  $env:TAURI_SIGNING_PRIVATE_KEY = Get-Content $HOME\.tauri\aijia.key -Raw'
    Write-Host '  $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = "Hello1199"'
    Write-Host '  $env:OSS_ACCESS_KEY_ID = "..."'
    Write-Host '  $env:OSS_ACCESS_KEY_SECRET = "..."'
    exit 1
}

# ── 1. Download unsigned exe + tauri sig from staging ──
Write-Host "=== Step 1: Download unsigned artifacts from OSS staging ===" -ForegroundColor Cyan
Write-Host "  $ExeUrl"
Invoke-WebRequest -Uri $ExeUrl -OutFile $ExePath -UseBasicParsing
Write-Host "  $SigUrl"
Invoke-WebRequest -Uri $SigUrl -OutFile $SigPath -UseBasicParsing

$exeInfo = Get-Item $ExePath
Write-Host ("  Downloaded: {0:N0} bytes" -f $exeInfo.Length) -ForegroundColor Green

# ── 2. Pause for SimpleSign GUI signing ──
Write-Host ""
Write-Host "=== Step 2: Sign the .exe with SimpleSign (manual) ===" -ForegroundColor Cyan
Write-Host "  File to sign:" -ForegroundColor Yellow
Write-Host "    $ExePath" -ForegroundColor Yellow
Write-Host ""
Write-Host "  Open SimpleSign, sign the file in-place, then come back here."
Write-Host ""
Read-Host "  Press ENTER after SimpleSign has finished signing"

# Quick verify
$sig = Get-AuthenticodeSignature $ExePath
if ($sig.Status -ne 'Valid') {
    Write-Host "  WARNING: Authenticode status is '$($sig.Status)' (expected 'Valid')." -ForegroundColor Yellow
    $confirm = Read-Host "  Continue anyway? (y/N)"
    if ($confirm -ne 'y') { exit 1 }
} else {
    Write-Host "  Authenticode OK: $($sig.SignerCertificate.Subject)" -ForegroundColor Green
}

# Drop the staging .sig — the inner script regenerates it from the SIGNED exe.
Remove-Item $SigPath -ErrorAction SilentlyContinue

# ── 3. Hand off to standard sign-and-upload-windows.ps1 ──
Write-Host ""
Write-Host "=== Step 3: Generate Tauri updater .sig + upload to OSS ===" -ForegroundColor Cyan
& (Join-Path $ScriptDir 'sign-and-upload-windows.ps1') $Version $Type $ExePath
if ($LASTEXITCODE -ne 0) {
    Write-Host "sign-and-upload-windows.ps1 failed with exit $LASTEXITCODE" -ForegroundColor Red
    exit $LASTEXITCODE
}

# ── 4. Cleanup staging files on OSS (optional, only after success) ──
Write-Host ""
Write-Host "=== Step 4: Remove staging artifacts from OSS ===" -ForegroundColor Cyan
$cleanup = Read-Host "  Delete aijia/staging/unsigned/v$Version/* from OSS now? (Y/n)"
if ($cleanup -ne 'n' -and $cleanup -ne 'N') {
    $env:PYTHONUTF8 = '1'
    python -m pip install --disable-pip-version-check oss2 2>$null
    python -c @"
import os, oss2, sys
auth = oss2.Auth(os.environ['OSS_ACCESS_KEY_ID'], os.environ['OSS_ACCESS_KEY_SECRET'])
bucket = oss2.Bucket(auth, 'https://oss-cn-beijing.aliyuncs.com', 'lotus-releases')
prefix = 'aijia/staging/unsigned/v$Version/'
for o in bucket.list_objects(prefix=prefix).object_list:
    print(f'  delete {o.key}')
    bucket.delete_object(o.key)
"@
}

Write-Host ""
Write-Host "================================================================" -ForegroundColor Green
Write-Host " Windows v$Version ($Type) signed and uploaded." -ForegroundColor Green
Write-Host "================================================================" -ForegroundColor Green
