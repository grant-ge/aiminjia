# Build src-tauri/resources/runtime/win32-x64/ from upstream sources.
#
# Inputs: scripts/runtime-sources.json
# Output: src-tauri/resources/runtime/win32-x64/{node,python,uv}/ + bundled-version.json
#
# Usage:
#   .\scripts\prepare-bundled-runtime.ps1
#   $env:FORCE=1; .\scripts\prepare-bundled-runtime.ps1

$ErrorActionPreference = "Stop"

$ScriptDir   = Split-Path -Parent $MyInvocation.MyCommand.Path
$ProjectDir  = Split-Path -Parent $ScriptDir
$Sources     = Join-Path $ScriptDir "runtime-sources.json"
$CacheDir    = if ($env:RUNTIME_PREP_CACHE) { $env:RUNTIME_PREP_CACHE } else { Join-Path $ProjectDir ".runtime-cache" }
$Plat        = "win32-x64"
$OutDir      = Join-Path $ProjectDir "src-tauri\resources\runtime\$Plat"

$src = Get-Content $Sources -Raw | ConvertFrom-Json
$BundleVersion = $src.bundleVersion

if ((Test-Path "$OutDir\bundled-version.json") -and ($env:FORCE -ne "1")) {
    $existing = (Get-Content "$OutDir\bundled-version.json" -Raw | ConvertFrom-Json).bundleVersion
    if ($existing -eq $BundleVersion) {
        Write-Host "[prepare-runtime] $Plat already at $BundleVersion, skipping (FORCE=1 to override)"
        exit 0
    }
}

New-Item -ItemType Directory -Force -Path $CacheDir, $OutDir | Out-Null

function Fetch-File {
    param([string]$Url, [string]$Sha, [string]$Name)
    $cached = Join-Path $CacheDir $Name
    if (Test-Path $cached) {
        $got = (Get-FileHash $cached -Algorithm SHA256).Hash.ToLower()
        if ($got -eq $Sha.ToLower()) {
            Write-Host "[cache-hit] $Name"
            return $cached
        }
        Write-Host "[cache-stale] $Name (got $got, expected $Sha) -- refetching"
        Remove-Item $cached
    }
    Write-Host "[download] $Url"
    Invoke-WebRequest -Uri $Url -OutFile $cached -UseBasicParsing
    $got = (Get-FileHash $cached -Algorithm SHA256).Hash.ToLower()
    if ($Sha -eq "AUTO-FILL-AT-FIRST-RUN-AND-COMMIT") {
        Write-Host "[NOTE] computed sha256 for ${Name}: $got"
        Write-Host "       Edit scripts/runtime-sources.json to pin this value, then re-run."
    } elseif ($got -ne $Sha.ToLower()) {
        throw "sha256 mismatch for $Name (got $got, expected $Sha)"
    }
    return $cached
}

function Extract-Zip {
    param([string]$Zip, [string]$Dest)
    if (Test-Path $Dest) { Remove-Item -Recurse -Force $Dest }
    New-Item -ItemType Directory -Force -Path $Dest | Out-Null
    Expand-Archive -Path $Zip -DestinationPath $Dest -Force
}

# Node
$nodeUrl = $src.node.platforms.$Plat.url
$nodeSha = $src.node.platforms.$Plat.sha256
$nodeZip = Fetch-File $nodeUrl $nodeSha "node-$Plat.zip"
$nodeTmp = Join-Path $env:TEMP "node-extract-$([Guid]::NewGuid().ToString('N'))"
Extract-Zip $nodeZip $nodeTmp
# zip contains node-v20.18.0-win-x64/ as top-level
$nodeRoot = Get-ChildItem $nodeTmp -Directory | Select-Object -First 1
if (Test-Path "$OutDir\node") { Remove-Item -Recurse -Force "$OutDir\node" }
Move-Item $nodeRoot.FullName "$OutDir\node"
Remove-Item -Recurse -Force $nodeTmp
if (-not (Test-Path "$OutDir\node\node.exe")) { throw "node.exe missing after extract" }
if (-not (Test-Path "$OutDir\node\npm.cmd")) { throw "npm.cmd missing after extract" }
if (-not (Test-Path "$OutDir\node\npx.cmd")) { throw "npx.cmd missing after extract" }

# Python (embeddable zip — flat layout, no nested dir)
$pyUrl = $src.python.platforms.$Plat.url
$pySha = $src.python.platforms.$Plat.sha256
$pyZip = Fetch-File $pyUrl $pySha "python-$Plat.zip"
if (Test-Path "$OutDir\python") { Remove-Item -Recurse -Force "$OutDir\python" }
Extract-Zip $pyZip "$OutDir\python"
if (-not (Test-Path "$OutDir\python\python.exe")) { throw "python.exe missing after extract" }
# Enable site-packages: embeddable disables site by default; patch python312._pth
$pthPath = Get-ChildItem "$OutDir\python" -Filter "python*._pth" | Select-Object -First 1
if ($pthPath) {
    $content = Get-Content $pthPath.FullName
    if ($content -notcontains "import site") {
        Add-Content -Path $pthPath.FullName -Value "`r`nimport site`r`nLib\site-packages"
    }
}
New-Item -ItemType Directory -Force -Path "$OutDir\python\Lib\site-packages" | Out-Null

# uv
$uvUrl = $src.uv.platforms.$Plat.url
$uvSha = $src.uv.platforms.$Plat.sha256
$uvZip = Fetch-File $uvUrl $uvSha "uv-$Plat.zip"
$uvTmp = Join-Path $env:TEMP "uv-extract-$([Guid]::NewGuid().ToString('N'))"
try {
    Extract-Zip $uvZip $uvTmp
    if (Test-Path "$OutDir\uv") { Remove-Item -Recurse -Force "$OutDir\uv" }
    New-Item -ItemType Directory -Force -Path "$OutDir\uv" | Out-Null
    Get-ChildItem $uvTmp -Recurse -Include "uv.exe","uvx.exe" | ForEach-Object {
        Copy-Item $_.FullName "$OutDir\uv\$($_.Name)"
    }
    if (-not (Test-Path "$OutDir\uv\uv.exe"))  { throw "uv.exe missing after extract" }
    if (-not (Test-Path "$OutDir\uv\uvx.exe")) { throw "uvx.exe missing after extract" }
} finally {
    if (Test-Path $uvTmp) { Remove-Item -Recurse -Force $uvTmp }
}

# Empty node_modules placeholder (RuntimeLayout::node_modules() returns "node/node_modules")
New-Item -ItemType Directory -Force -Path "$OutDir\node\node_modules" | Out-Null

# bundled-version.json
@{
    bundleVersion = $BundleVersion
    platform      = $Plat
    node          = $src.node.version
    python        = $src.python.version
    uv            = $src.uv.version
} | ConvertTo-Json | Set-Content "$OutDir\bundled-version.json"

Write-Host "[prepare-runtime] OK: $OutDir at $BundleVersion"
