[CmdletBinding()]
param(
    [string]$InstallDir = (Join-Path $env:USERPROFILE ".aijia\bin")
)

$ErrorActionPreference = "Stop"

$RepoRoot = Split-Path -Parent $PSScriptRoot
$TauriDir = Join-Path $RepoRoot "src-tauri"

Push-Location $TauriDir
try {
    cargo build --release --bin aijia-cli
} finally {
    Pop-Location
}

$ExePath = Join-Path $TauriDir "target\release\aijia-cli.exe"
$ShortExePath = Join-Path $InstallDir "aijia.exe"
New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
Copy-Item -LiteralPath $ExePath -Destination $ShortExePath -Force

$UserPath = [Environment]::GetEnvironmentVariable("Path", "User")
$PathParts = @()
if (-not [string]::IsNullOrWhiteSpace($UserPath)) {
    $PathParts = $UserPath -split ';' | Where-Object { -not [string]::IsNullOrWhiteSpace($_) }
}
$AlreadyInPath = $PathParts | Where-Object { $_.TrimEnd('\') -ieq $InstallDir.TrimEnd('\') }
if (-not $AlreadyInPath) {
    $NewUserPath = if ([string]::IsNullOrWhiteSpace($UserPath)) {
        $InstallDir
    } else {
        "$UserPath;$InstallDir"
    }
    [Environment]::SetEnvironmentVariable("Path", $NewUserPath, "User")
}
if (($env:Path -split ';' | Where-Object { $_.TrimEnd('\') -ieq $InstallDir.TrimEnd('\') }).Count -eq 0) {
    $env:Path = "$InstallDir;$env:Path"
}

Write-Host "Built $ExePath"
Write-Host "Installed $ShortExePath"
Write-Host 'Open a new shell to use: aijia -p "1+1"'
