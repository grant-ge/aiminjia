# Setup Playwright runtime for AI小家 (Windows)
# Downloads Node.js and installs Playwright with Chromium

$ErrorActionPreference = "Stop"

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$ProjectDir = Split-Path -Parent $ScriptDir
$RuntimeDir = Join-Path $ProjectDir "src-tauri\playwright-runtime"
$NodeVersion = "20.18.0"

Write-Host "=== Setting up Playwright runtime (Windows) ==="

$NodeDir = Join-Path $RuntimeDir "node"
$NodeUrl = "https://nodejs.org/dist/v${NodeVersion}/node-v${NodeVersion}-win-x64.zip"

# Download Node.js if not present
if (-not (Test-Path (Join-Path $NodeDir "node.exe"))) {
    Write-Host "Downloading Node.js v${NodeVersion} for Windows x64..."
    New-Item -ItemType Directory -Force -Path $NodeDir | Out-Null
    $ZipPath = Join-Path $env:TEMP "node-win-x64.zip"
    Invoke-WebRequest -Uri $NodeUrl -OutFile $ZipPath -UseBasicParsing

    # Extract
    $ExtractDir = Join-Path $env:TEMP "node-extract"
    if (Test-Path $ExtractDir) { Remove-Item -Recurse -Force $ExtractDir }
    Expand-Archive -Path $ZipPath -DestinationPath $ExtractDir

    # Move contents (strip top-level dir)
    $InnerDir = Get-ChildItem $ExtractDir | Select-Object -First 1
    Get-ChildItem $InnerDir.FullName | Move-Item -Destination $NodeDir -Force

    Remove-Item -Recurse -Force $ExtractDir
    Remove-Item -Force $ZipPath

    Write-Host "Node.js installed: $(& (Join-Path $NodeDir 'node.exe') --version)"
} else {
    Write-Host "Node.js already installed: $(& (Join-Path $NodeDir 'node.exe') --version)"
}

$NodeExe = Join-Path $NodeDir "node.exe"
$NpmCmd = Join-Path $NodeDir "npm.cmd"

# Install npm dependencies
Write-Host "Installing npm dependencies..."
Push-Location $RuntimeDir
$ErrorActionPreference = "Continue"
& $NpmCmd install --omit=dev 2>&1 | ForEach-Object { Write-Host $_ }
$exitCode = $LASTEXITCODE
$ErrorActionPreference = "Stop"
Pop-Location
if ($exitCode -ne 0) { throw "npm install failed (exit $exitCode)" }

# Install Playwright Chromium browser
Write-Host "Installing Playwright Chromium..."
$env:PLAYWRIGHT_BROWSERS_PATH = Join-Path $RuntimeDir "browsers"
$ErrorActionPreference = "Continue"
& (Join-Path $NodeDir "npx.cmd") --yes playwright install chromium 2>&1 | ForEach-Object { Write-Host $_ }
$exitCode = $LASTEXITCODE
$ErrorActionPreference = "Stop"
if ($exitCode -ne 0) { throw "playwright install failed (exit $exitCode)" }

Write-Host ""
Write-Host "=== Playwright runtime ready ==="
Write-Host "Node: $NodeExe"
Write-Host "Browsers: $(Join-Path $RuntimeDir 'browsers')"
Write-Host "Entry: $(Join-Path $RuntimeDir 'browser.js')"
