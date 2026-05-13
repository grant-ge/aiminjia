# Setup Windows signing machine for AIjia
#
# This machine only does: Authenticode signing + Tauri updater signing + OSS upload.
# Building is done on GitHub-hosted runners.
#
# Usage: powershell -ExecutionPolicy Bypass -File scripts\setup-runner-windows.ps1

$ErrorActionPreference = "Stop"

Write-Host "=== AIjia Windows Signing Machine Setup ===" -ForegroundColor Cyan
Write-Host ""

# ── 1. Node.js (for tauri signer) ──
Write-Host "[1/4] Checking Node.js..." -ForegroundColor Green
if (Get-Command node -ErrorAction SilentlyContinue) {
    $nodeVer = node --version
    Write-Host "  Node.js $nodeVer found"
} else {
    Write-Host "  Node.js not found. Install from https://nodejs.org/ (v20 LTS)" -ForegroundColor Yellow
}

# ── 2. Tauri CLI (for updater signature) ──
Write-Host "[2/4] Checking Tauri CLI..." -ForegroundColor Green
if (Get-Command tauri -ErrorAction SilentlyContinue) {
    $tauriVer = tauri --version 2>$null
    Write-Host "  tauri-cli $tauriVer found"
} else {
    Write-Host "  Tauri CLI not found. Installing globally..."
    if (Get-Command npm -ErrorAction SilentlyContinue) {
        npm install -g @tauri-apps/cli
        Write-Host "  Installed. Fallback: npx @tauri-apps/cli@latest also works." -ForegroundColor Green
    } else {
        Write-Host "  Cannot install — Node.js/npm not available. Install Node.js first." -ForegroundColor Yellow
    }
}

# ── 3. Python + oss2 (for OSS upload) ──
Write-Host "[3/4] Checking Python + oss2..." -ForegroundColor Green
if (Get-Command python -ErrorAction SilentlyContinue) {
    $pyVer = python --version
    Write-Host "  $pyVer found"
    Write-Host "  Installing/upgrading oss2..."
    python -m pip install --disable-pip-version-check --upgrade oss2
} else {
    Write-Host "  Python not found. Install from https://www.python.org/ (v3.10+)" -ForegroundColor Yellow
}

# ── 4. Code signing certificate ──
Write-Host "[4/4] Checking code signing setup..." -ForegroundColor Green
Write-Host "  EV certificate signing is done via SimpleSign app."
Write-Host "  Ensure SimpleSign is installed and the hardware token (USB) is connected."

# ── Summary ──
Write-Host ""
Write-Host "=== Environment Variables Required ===" -ForegroundColor Cyan
Write-Host "Set these as user env vars (persistent across reboots):"
Write-Host ""
Write-Host '  [System.Environment]::SetEnvironmentVariable("TAURI_SIGNING_PRIVATE_KEY", "<base64-key>", "User")'
Write-Host '  [System.Environment]::SetEnvironmentVariable("TAURI_SIGNING_PRIVATE_KEY_PASSWORD", "<password>", "User")'
Write-Host '  [System.Environment]::SetEnvironmentVariable("OSS_ACCESS_KEY_ID", "<ak>", "User")'
Write-Host '  [System.Environment]::SetEnvironmentVariable("OSS_ACCESS_KEY_SECRET", "<sk>", "User")'
Write-Host ""
Write-Host "=== Usage ===" -ForegroundColor Cyan
Write-Host "After CI build completes on GitHub:"
Write-Host "  1. Download 'windows-installers' artifact from GitHub Actions"
Write-Host "  2. Sign the exe with SimpleSign"
Write-Host "  3. Run: .\scripts\sign-and-upload-windows.ps1 <version> <release|beta>"
Write-Host ""
Write-Host "=== Network ===" -ForegroundColor Cyan
Write-Host "Machine needs access to:"
Write-Host "  - registry.npmjs.org (npx tauri-cli fallback)"
Write-Host "  - oss-cn-beijing.aliyuncs.com (OSS upload)"
Write-Host ""
Write-Host "Setup complete!" -ForegroundColor Green
