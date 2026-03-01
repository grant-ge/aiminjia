# Download python-build-standalone + install pip deps into src-tauri\python-runtime\
# Usage: pwsh scripts/setup-python.ps1
$ErrorActionPreference = "Stop"

$PYTHON_VERSION = "3.12.8"
$STANDALONE_TAG = "20250106"
$TARGET_DIR = "src-tauri\python-runtime"
$REQUIREMENTS = "src-tauri\requirements.txt"
$TRIPLE = "x86_64-pc-windows-msvc"
$PYTHON_BIN = "$TARGET_DIR\python.exe"

$FILENAME = "cpython-${PYTHON_VERSION}+${STANDALONE_TAG}-${TRIPLE}-install_only_stripped.tar.gz"
$URL = "https://github.com/astral-sh/python-build-standalone/releases/download/${STANDALONE_TAG}/${FILENAME}"

# ─── Skip if already set up ───────────────────────────────────────
if (Test-Path $PYTHON_BIN) {
    $existingVer = & $PYTHON_BIN --version 2>&1
    if ($existingVer -match $PYTHON_VERSION) {
        Write-Host "Python $PYTHON_VERSION already exists at $PYTHON_BIN, skipping download."
        Write-Host "To force re-download, delete $TARGET_DIR\ and re-run."
        Write-Host "Installing pip dependencies..."
        & $PYTHON_BIN -m pip install -r $REQUIREMENTS --only-binary :all: --no-cache-dir -q
        Write-Host "Done."
        exit 0
    }
}

# ─── Download ──────────────────────────────────────────────────────
Write-Host "Downloading Python $PYTHON_VERSION for $TRIPLE..."
Write-Host "URL: $URL"

$tmpDir = New-TemporaryFile | ForEach-Object { Remove-Item $_; New-Item -ItemType Directory -Path $_ }
$archive = Join-Path $tmpDir $FILENAME

try {
    $ProgressPreference = 'SilentlyContinue'
    # Retry up to 3 times — GitHub releases can be flaky on CI runners
    $maxRetries = 3
    for ($attempt = 1; $attempt -le $maxRetries; $attempt++) {
        try {
            Write-Host "  Attempt $attempt of $maxRetries..."
            Invoke-WebRequest -Uri $URL -OutFile $archive -UseBasicParsing
            # Verify file is not truncated (Python standalone is >20MB)
            $fileSize = (Get-Item $archive).Length
            if ($fileSize -lt 20MB) {
                throw "Downloaded file too small ($([math]::Round($fileSize/1MB, 1)) MB) — likely truncated"
            }
            Write-Host "  Downloaded $([math]::Round($fileSize/1MB, 1)) MB"
            break
        } catch {
            if ($attempt -eq $maxRetries) {
                throw "Download failed after $maxRetries attempts: $_"
            }
            Write-Host "  Attempt $attempt failed: $_ — retrying in 5s..."
            Start-Sleep -Seconds 5
        }
    }
} catch {
    Write-Error "Download failed: $_"
    exit 1
}

# ─── Extract ───────────────────────────────────────────────────────
Write-Host "Extracting to $TARGET_DIR\..."
if (Test-Path $TARGET_DIR) {
    Remove-Item -Recurse -Force $TARGET_DIR
}
# Ensure the target is fully removed before rename
Start-Sleep -Milliseconds 500

$parentDir = Split-Path $TARGET_DIR -Parent
# tar.exe is built into Windows 10+ — use full path to avoid Git Bash's /usr/bin/tar
$tarExe = "$env:SystemRoot\System32\tar.exe"
if (-not (Test-Path $tarExe)) { $tarExe = "tar" }
& $tarExe xzf $archive -C $parentDir
if ($LASTEXITCODE -ne 0) {
    Write-Error "tar extraction failed (exit code $LASTEXITCODE). Archive may be corrupted."
    exit 1
}

# python-build-standalone archives contain a top-level `python\` directory
$extractedDir = Join-Path $parentDir "python"
if (-not (Test-Path $extractedDir)) {
    Write-Error "Expected directory '$extractedDir' not found after extraction. Archive format may have changed."
    exit 1
}
# Move-Item works when target doesn't exist; Rename-Item fails if target path looks like device name
Move-Item -Path $extractedDir -Destination $TARGET_DIR -Force

Write-Host "Python binary: $PYTHON_BIN"
& $PYTHON_BIN --version

# ─── Install pip dependencies ──────────────────────────────────────
Write-Host "Installing pip dependencies from $REQUIREMENTS..."
# --only-binary :all: — force pre-built wheels, never compile C extensions
& $PYTHON_BIN -m pip install -r $REQUIREMENTS --only-binary :all: --no-cache-dir -q

# ─── Slim down ─────────────────────────────────────────────────────
Write-Host "Removing unnecessary files to reduce bundle size..."

$libDir = Join-Path $TARGET_DIR "Lib"

# Remove test directories
Get-ChildItem -Path $TARGET_DIR -Recurse -Directory -Filter "test" -ErrorAction SilentlyContinue | Remove-Item -Recurse -Force -ErrorAction SilentlyContinue
Get-ChildItem -Path $TARGET_DIR -Recurse -Directory -Filter "tests" -ErrorAction SilentlyContinue | Remove-Item -Recurse -Force -ErrorAction SilentlyContinue

# Remove __pycache__
Get-ChildItem -Path $TARGET_DIR -Recurse -Directory -Filter "__pycache__" -ErrorAction SilentlyContinue | Remove-Item -Recurse -Force -ErrorAction SilentlyContinue

# Remove .pyc files
Get-ChildItem -Path $TARGET_DIR -Recurse -Filter "*.pyc" -ErrorAction SilentlyContinue | Remove-Item -Force -ErrorAction SilentlyContinue

# Remove unused stdlib modules
foreach ($dir in @("tkinter", "idlelib", "turtle", "turtledemo", "ensurepip", "lib2to3", "distutils")) {
    $path = Join-Path $libDir $dir
    if (Test-Path $path) {
        Remove-Item -Recurse -Force $path -ErrorAction SilentlyContinue
    }
}

# Remove pip (not needed at runtime)
$pipDir = Join-Path $libDir "site-packages\pip"
if (Test-Path $pipDir) {
    Remove-Item -Recurse -Force $pipDir -ErrorAction SilentlyContinue
}

# Remove .dist-info directories
Get-ChildItem -Path $TARGET_DIR -Recurse -Directory -Filter "*.dist-info" -ErrorAction SilentlyContinue | Remove-Item -Recurse -Force -ErrorAction SilentlyContinue

# ─── Cleanup temp files ───────────────────────────────────────────
Remove-Item -Recurse -Force $tmpDir -ErrorAction SilentlyContinue

# ─── Summary ───────────────────────────────────────────────────────
$size = (Get-ChildItem -Recurse $TARGET_DIR | Measure-Object -Property Length -Sum).Sum / 1MB
$sizeStr = "{0:N0} MB" -f $size

Write-Host ""
Write-Host "Setup complete!"
Write-Host "  Python: $PYTHON_BIN"
Write-Host "  Size:   $sizeStr"
& $PYTHON_BIN --version
