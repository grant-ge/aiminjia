# Setup DingTalk Workspace CLI (dws.exe) for AIjia on Windows.
# Downloads the Windows binary from GitHub releases and places it in src-tauri/resources/.

$ErrorActionPreference = "Stop"

$ScriptDir    = Split-Path -Parent $MyInvocation.MyCommand.Path
$ProjectDir   = Split-Path -Parent $ScriptDir
$ResourcesDir = Join-Path $ProjectDir "src-tauri\resources"
$Repo         = "DingTalk-Real-AI/dingtalk-workspace-cli"
$BinPath      = Join-Path $ResourcesDir "dws.exe"

Write-Host "=== Setting up DingTalk Workspace CLI (dws.exe) ==="

New-Item -ItemType Directory -Force -Path $ResourcesDir | Out-Null

if ((Test-Path $BinPath) -and ($env:DWS_FORCE_REINSTALL -ne "1")) {
    Write-Host "dws.exe already present, skipping download."
    exit 0
}

# Resolve version (env DWS_VERSION or latest from GitHub redirect)
$Version = $env:DWS_VERSION
if (-not $Version -or $Version -eq "latest") {
    $resp = Invoke-WebRequest -Uri "https://github.com/$Repo/releases/latest" -MaximumRedirection 0 -ErrorAction SilentlyContinue
    if ($resp.Headers.Location) {
        $Version = ($resp.Headers.Location -split '/tag/')[-1].Trim()
    }
    if (-not $Version) {
        # Fallback to API
        $api = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases/latest"
        $Version = $api.tag_name
    }
}

Write-Host "Downloading dws $Version (windows/amd64)..."

# Detect arch (only amd64 published as of v1.0.17; arm64 falls back to amd64)
$ArchiveName = "dws-windows-amd64.zip"
$DownloadUrl = "https://github.com/$Repo/releases/download/$Version/$ArchiveName"

$TempDir = Join-Path $env:TEMP ("dws-setup-" + [System.Guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Force -Path $TempDir | Out-Null

try {
    $ArchivePath = Join-Path $TempDir $ArchiveName
    Invoke-WebRequest -Uri $DownloadUrl -OutFile $ArchivePath -UseBasicParsing

    Expand-Archive -Path $ArchivePath -DestinationPath $TempDir -Force

    # Find dws.exe (may be at root or inside subdir)
    $found = Get-ChildItem -Path $TempDir -Filter "dws.exe" -Recurse | Select-Object -First 1
    if (-not $found) {
        throw "dws.exe not found inside $ArchiveName"
    }
    Copy-Item -Path $found.FullName -Destination $BinPath -Force

    Write-Host "dws.exe installed at $BinPath"
    & $BinPath --version
    if ($LASTEXITCODE -ne 0) { throw "dws.exe --version failed (exit $LASTEXITCODE)" }
} finally {
    Remove-Item -Recurse -Force -ErrorAction SilentlyContinue $TempDir
}
