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
    Invoke-WebRequest -Uri $NodeUrl -OutFile $ZipPath

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
$npmResult = & $NpmCmd install --production 2>&1
$npmResult | Select-Object -Last 5
if ($LASTEXITCODE -ne 0) { Pop-Location; throw "npm install failed (exit $LASTEXITCODE)" }
Pop-Location

# Install Playwright Chromium browser
Write-Host "Installing Playwright Chromium..."
$env:PLAYWRIGHT_BROWSERS_PATH = Join-Path $RuntimeDir "browsers"
$pwResult = & (Join-Path $NodeDir "npx.cmd") playwright install chromium 2>&1
$pwResult | Select-Object -Last 5
if ($LASTEXITCODE -ne 0) { throw "playwright install failed (exit $LASTEXITCODE)" }

Write-Host ""
Write-Host "=== Playwright runtime ready ==="
Write-Host "Node: $NodeExe"
Write-Host "Browsers: $(Join-Path $RuntimeDir 'browsers')"
Write-Host "Entry: $(Join-Path $RuntimeDir 'browser.js')"
