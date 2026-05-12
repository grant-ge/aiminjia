# Setup Windows self-hosted GitHub Actions runner for AIjia
# Run this script once on a new Windows build machine (as Administrator).
#
# Prerequisites:
#   - Windows 10/11 x64
#   - Code signing certificate imported into local cert store
#   - Internet access (direct or system-level proxy configured)
#
# Usage: powershell -ExecutionPolicy Bypass -File scripts/setup-runner-windows.ps1

$ErrorActionPreference = "Stop"

Write-Host "=== AIjia Windows Runner Setup ===" -ForegroundColor Cyan
Write-Host ""

# ── 1. Check admin ──
$isAdmin = ([Security.Principal.WindowsPrincipal] [Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $isAdmin) {
    Write-Host "WARNING: Not running as Administrator. Some steps may fail." -ForegroundColor Yellow
}

# ── 2. Node.js ──
Write-Host "[1/6] Checking Node.js..." -ForegroundColor Green
if (Get-Command node -ErrorAction SilentlyContinue) {
    $nodeVer = node --version
    Write-Host "  Node.js $nodeVer found"
} else {
    Write-Host "  Node.js not found. Install from https://nodejs.org/ (v20 LTS recommended)" -ForegroundColor Yellow
}

# ── 3. pnpm ──
Write-Host "[2/6] Checking pnpm..." -ForegroundColor Green
if (Get-Command pnpm -ErrorAction SilentlyContinue) {
    $pnpmVer = pnpm --version
    Write-Host "  pnpm $pnpmVer found"
} else {
    Write-Host "  Installing pnpm..."
    npm install -g pnpm@9
}

# ── 4. Rust ──
Write-Host "[3/6] Checking Rust..." -ForegroundColor Green
if (Get-Command cargo -ErrorAction SilentlyContinue) {
    $rustVer = rustc --version
    Write-Host "  $rustVer found"
} else {
    Write-Host "  Rust not found. Installing..."
    Invoke-WebRequest -Uri "https://win.rustup.rs/x86_64" -OutFile "$env:TEMP\rustup-init.exe" -UseBasicParsing
    & "$env:TEMP\rustup-init.exe" -y --default-toolchain stable
    $env:Path += ";$env:USERPROFILE\.cargo\bin"
    Write-Host "  Rust installed: $(rustc --version)"
}

# ── 5. Python + oss2 ──
Write-Host "[4/6] Checking Python..." -ForegroundColor Green
if (Get-Command python -ErrorAction SilentlyContinue) {
    $pyVer = python --version
    Write-Host "  $pyVer found"
    Write-Host "  Installing oss2..."
    python -m pip install --disable-pip-version-check oss2
} else {
    Write-Host "  Python not found. Install from https://www.python.org/ (v3.10+ recommended)" -ForegroundColor Yellow
}

# ── 6. Windows SDK (signtool) ──
Write-Host "[5/6] Checking Windows SDK (signtool)..." -ForegroundColor Green
$signtool = Get-ChildItem "C:\Program Files (x86)\Windows Kits\10\bin" -Recurse -Filter "signtool.exe" -ErrorAction SilentlyContinue |
    Sort-Object { $_.Directory.Name } -Descending |
    Select-Object -First 1
if ($signtool) {
    Write-Host "  signtool found: $($signtool.FullName)"
} else {
    Write-Host "  signtool.exe not found. Install Windows SDK from Visual Studio Installer." -ForegroundColor Yellow
}

# ── 7. Code signing certificate ──
Write-Host "[6/6] Checking code signing certificate..." -ForegroundColor Green
$certs = Get-ChildItem -Path Cert:\CurrentUser\My -CodeSigningCert -ErrorAction SilentlyContinue
if ($certs) {
    foreach ($cert in $certs) {
        Write-Host "  Found: $($cert.Subject)" -ForegroundColor Green
        Write-Host "  Thumbprint: $($cert.Thumbprint)"
        Write-Host "  Expires: $($cert.NotAfter)"
    }
} else {
    $certs = Get-ChildItem -Path Cert:\LocalMachine\My -CodeSigningCert -ErrorAction SilentlyContinue
    if ($certs) {
        foreach ($cert in $certs) {
            Write-Host "  Found (LocalMachine): $($cert.Subject)" -ForegroundColor Green
            Write-Host "  Thumbprint: $($cert.Thumbprint)"
        }
    } else {
        Write-Host "  No code signing certificate found in cert store." -ForegroundColor Yellow
    }
}

# ── Summary ──
Write-Host ""
Write-Host "=== Environment Variables to Set ===" -ForegroundColor Cyan
Write-Host "Set these as system environment variables (persistent across reboots):"
Write-Host ""
Write-Host '  [System.Environment]::SetEnvironmentVariable("SIGN_CERT_THUMBPRINT", "<cert-thumbprint>", "Machine")'
Write-Host '  [System.Environment]::SetEnvironmentVariable("TAURI_SIGNING_PRIVATE_KEY", "<base64-key>", "Machine")'
Write-Host '  [System.Environment]::SetEnvironmentVariable("TAURI_SIGNING_PRIVATE_KEY_PASSWORD", "<password>", "Machine")'
Write-Host '  [System.Environment]::SetEnvironmentVariable("OSS_ACCESS_KEY_ID", "<ak>", "Machine")'
Write-Host '  [System.Environment]::SetEnvironmentVariable("OSS_ACCESS_KEY_SECRET", "<sk>", "Machine")'
Write-Host ""
Write-Host "=== GitHub Actions Runner ===" -ForegroundColor Cyan
Write-Host "Register at: GitHub repo -> Settings -> Actions -> Runners -> New self-hosted runner"
Write-Host "Labels: Windows, X64"
Write-Host "Install as service: .\svc.cmd install && .\svc.cmd start"
Write-Host ""
Write-Host "=== Network ===" -ForegroundColor Cyan
Write-Host "If behind a firewall, configure system-level proxy (e.g., Clash/V2Ray)."
Write-Host "CI workflow does NOT set proxy — the machine must be able to reach:"
Write-Host "  - github.com (git clone)"
Write-Host "  - registry.npmjs.org (pnpm install)"
Write-Host "  - nodejs.org (Playwright Node.js runtime)"
Write-Host "  - cdn.playwright.dev / storage.googleapis.com (Playwright Chromium)"
Write-Host "  - static.rust-lang.org (rustup)"
Write-Host "  - ai.renlijia.com (changelog)"
Write-Host ""
Write-Host "Setup complete!" -ForegroundColor Green
