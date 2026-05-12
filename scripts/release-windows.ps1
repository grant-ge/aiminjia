# AIjia Windows release — one-shot end-to-end script.
#
# Downloads unsigned exe from OSS staging, Authenticode-signs it via signtool,
# generates the Tauri updater .sig, uploads everything to the public OSS path,
# and cleans up staging. Zero Python dependency — uses Node (ali-oss) for OSS.
#
# Credentials are stored in Windows Credential Manager after the first run,
# so subsequent runs only need -Version and -Type.
#
# Usage:
#   .\scripts\release-windows.ps1 -Version 0.5.23-beta.1 -Type beta
#   .\scripts\release-windows.ps1 -Version 0.5.23 -Type release
#
# First-time setup will prompt for:
#   - Authenticode cert SHA1 thumbprint
#   - OSS access key id + secret
#   - Tauri signing key password
#
# Override stored creds: -Reconfigure
# Skip cleanup of staging:  -KeepStaging
#
# Prereqs:
#   - Node.js (npm/npx) — already required for the project
#   - signtool.exe in PATH (Windows SDK)
#   - EV hardware token plugged in
#   - $HOME\.tauri\aijia.key  (the Tauri Ed25519 private key)

param(
    [Parameter(Mandatory=$true)][string]$Version,
    [Parameter(Mandatory=$true)][ValidateSet('beta','release')][string]$Type,
    [switch]$Reconfigure,
    [switch]$KeepStaging,
    [string]$TimestampUrl = 'http://time.certum.pl'
)

$ErrorActionPreference = 'Stop'

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$RepoRoot  = Split-Path -Parent $ScriptDir
Set-Location $RepoRoot

$ExeName  = "AIjia_${Version}_x64-setup.exe"
$WorkDir  = Join-Path $RepoRoot 'build\windows-unsigned'
$ExePath  = Join-Path $WorkDir $ExeName
$SigPath  = "$ExePath.sig"
$TauriKey = Join-Path $HOME '.tauri\aijia.key'

$StagingBase = "https://lotus.renlijia.com/aijia/staging/unsigned/v$Version"

# ── helpers ──────────────────────────────────────────────────────────────
function Write-Section($title) {
    Write-Host ''
    Write-Host "═══ $title ═══" -ForegroundColor Cyan
}
function Write-Ok($msg)   { Write-Host "  ✓ $msg" -ForegroundColor Green }
function Write-Warn($msg) { Write-Host "  ! $msg" -ForegroundColor Yellow }
function Write-Err($msg)  { Write-Host "  ✗ $msg" -ForegroundColor Red }

# Find a real python.exe — but we don't actually need it here.
# Find a real signtool.exe (Windows SDK). Prefer the one in PATH.
function Get-Signtool {
    $cmd = Get-Command signtool.exe -ErrorAction SilentlyContinue
    if ($cmd) { return $cmd.Source }
    # Fall back to common SDK locations (newest version first).
    $sdkRoots = @(
        'C:\Program Files (x86)\Windows Kits\10\bin',
        'C:\Program Files\Windows Kits\10\bin'
    )
    foreach ($root in $sdkRoots) {
        if (-not (Test-Path $root)) { continue }
        $candidates = Get-ChildItem $root -Directory -ErrorAction SilentlyContinue |
            Where-Object { $_.Name -match '^10\.' } |
            Sort-Object Name -Descending |
            ForEach-Object { Join-Path $_.FullName 'x64\signtool.exe' } |
            Where-Object { Test-Path $_ }
        if ($candidates.Count -gt 0) { return $candidates[0] }
    }
    throw 'signtool.exe not found — install the Windows SDK or add it to PATH'
}

# ── credential storage (Windows Credential Manager) ──────────────────────
# Uses cmdkey.exe (no extra modules needed). Each value stored as a generic
# credential under target name AIjia.<key>.
function Save-Credential {
    param([string]$Name, [string]$Value)
    cmdkey /generic:"AIjia.$Name" /user:aijia /pass:"$Value" | Out-Null
}
function Load-Credential {
    param([string]$Name)
    # cmdkey doesn't expose the password — use Win32 CredRead via .NET instead.
    Add-Type -Namespace AIjiaCred -Name Native -MemberDefinition @'
[DllImport("Advapi32.dll", SetLastError=true, CharSet=CharSet.Unicode)]
public static extern bool CredRead(string target, int type, int reservedFlag, out IntPtr CredentialPtr);
[DllImport("Advapi32.dll", SetLastError=true)]
public static extern void CredFree([In] IntPtr cred);
[StructLayout(LayoutKind.Sequential, CharSet=CharSet.Unicode)]
public struct CREDENTIAL {
    public uint Flags; public uint Type; public string TargetName; public string Comment;
    public System.Runtime.InteropServices.ComTypes.FILETIME LastWritten;
    public uint CredentialBlobSize; public IntPtr CredentialBlob;
    public uint Persist; public uint AttributeCount; public IntPtr Attributes;
    public string TargetAlias; public string UserName;
}
'@ -ErrorAction SilentlyContinue

    $ptr = [IntPtr]::Zero
    if (-not [AIjiaCred.Native]::CredRead("AIjia.$Name", 1, 0, [ref]$ptr)) { return $null }
    try {
        $cred = [System.Runtime.InteropServices.Marshal]::PtrToStructure($ptr, [type][AIjiaCred.Native+CREDENTIAL])
        return [System.Runtime.InteropServices.Marshal]::PtrToStringUni($cred.CredentialBlob, $cred.CredentialBlobSize / 2)
    } finally {
        [AIjiaCred.Native]::CredFree($ptr)
    }
}

function Get-OrPrompt {
    param([string]$Name, [string]$Prompt, [switch]$Secret)
    if (-not $Reconfigure) {
        $existing = Load-Credential -Name $Name
        if ($existing) { return $existing }
    }
    if ($Secret) {
        $sec = Read-Host -Prompt $Prompt -AsSecureString
        $bstr = [System.Runtime.InteropServices.Marshal]::SecureStringToBSTR($sec)
        try { $value = [System.Runtime.InteropServices.Marshal]::PtrToStringBSTR($bstr) }
        finally { [System.Runtime.InteropServices.Marshal]::ZeroFreeBSTR($bstr) }
    } else {
        $value = Read-Host -Prompt $Prompt
    }
    Save-Credential -Name $Name -Value $value
    return $value
}

# ── 0. Sanity: tauri key file present ────────────────────────────────────
if (-not (Test-Path $TauriKey)) {
    Write-Err "Tauri signing key not found at: $TauriKey"
    Write-Host ''
    Write-Host "Create it with the value from the macOS machine (~/.tauri/aijia.key)." -ForegroundColor Yellow
    Write-Host "Example:" -ForegroundColor Yellow
    Write-Host '  New-Item -ItemType Directory -Force -Path "$HOME\.tauri" | Out-Null' -ForegroundColor Yellow
    Write-Host '  Set-Content "$HOME\.tauri\aijia.key" -Value "<base64-from-mac>" -NoNewline -Encoding ASCII' -ForegroundColor Yellow
    exit 1
}

Write-Section "AIjia Windows $Version ($Type) — release pipeline"

# ── 1. Load (or prompt for) credentials ──────────────────────────────────
Write-Section "Step 1/5: Load credentials"
$Thumbprint    = Get-OrPrompt -Name 'Thumbprint'      -Prompt 'Authenticode cert SHA1 thumbprint'
$OssKeyId      = Get-OrPrompt -Name 'OssKeyId'        -Prompt 'OSS_ACCESS_KEY_ID'
$OssKeySecret  = Get-OrPrompt -Name 'OssKeySecret'    -Prompt 'OSS_ACCESS_KEY_SECRET'         -Secret
$TauriKeyPwd   = Get-OrPrompt -Name 'TauriKeyPwd'     -Prompt 'TAURI_SIGNING_PRIVATE_KEY_PASSWORD' -Secret
Write-Ok "thumbprint=$($Thumbprint.Substring(0,8))…  oss key id=$($OssKeyId.Substring(0,6))…"

# ── 2. Download unsigned artifacts ───────────────────────────────────────
Write-Section "Step 2/5: Download unsigned artifacts from OSS staging"
New-Item -ItemType Directory -Force -Path $WorkDir | Out-Null
$exeUrl = "$StagingBase/$ExeName"
$sigUrl = "$StagingBase/$ExeName.sig"
Write-Host "  $exeUrl"
Invoke-WebRequest -Uri $exeUrl -OutFile $ExePath -UseBasicParsing
Write-Host "  $sigUrl"
Invoke-WebRequest -Uri $sigUrl -OutFile $SigPath -UseBasicParsing
$exeSize = (Get-Item $ExePath).Length
Write-Ok ("exe downloaded: {0:N0} bytes" -f $exeSize)
# We always regenerate .sig from the SIGNED exe — drop the staging copy now.
Remove-Item $SigPath -ErrorAction SilentlyContinue

# ── 3. Authenticode sign with signtool ───────────────────────────────────
Write-Section "Step 3/5: Authenticode sign (signtool + EV token)"
$signtool = Get-Signtool
Write-Host "  signtool: $signtool"
Write-Host "  timestamp: $TimestampUrl"
& $signtool sign /v /fd sha256 /sha1 $Thumbprint /tr $TimestampUrl /td sha256 $ExePath
if ($LASTEXITCODE -ne 0) { throw "signtool sign failed (exit $LASTEXITCODE)" }
& $signtool verify /pa /v $ExePath | Out-Null
if ($LASTEXITCODE -ne 0) { throw "signtool verify failed (exit $LASTEXITCODE)" }
$auth = Get-AuthenticodeSignature $ExePath
if ($auth.Status -ne 'Valid') { throw "Authenticode status not Valid: $($auth.Status)" }
if (-not $auth.TimeStamperCertificate) { Write-Warn 'No timestamp present — signature will expire with cert!' }
Write-Ok "signed by $($auth.SignerCertificate.Subject -replace '^CN=([^,]*).*', '$1')"

# ── 4. Generate Tauri updater .sig ───────────────────────────────────────
Write-Section "Step 4/5: Generate Tauri updater signature"
# Pass key file path + password directly — avoids env-var quoting bugs.
$env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = $TauriKeyPwd
$cliArgs = @('--yes', '@tauri-apps/cli@latest', 'signer', 'sign', '-k', $TauriKey, $ExePath)
& npx @cliArgs
$signerExit = $LASTEXITCODE
Remove-Item Env:\TAURI_SIGNING_PRIVATE_KEY_PASSWORD -ErrorAction SilentlyContinue
if ($signerExit -ne 0) { throw "tauri signer failed (exit $signerExit)" }
if (-not (Test-Path $SigPath)) { throw ".sig not created at $SigPath" }
Write-Ok "sig: $SigPath ($((Get-Item $SigPath).Length) bytes)"

# ── 5. Upload to OSS via Node ────────────────────────────────────────────
Write-Section "Step 5/5: Upload to OSS (Node + ali-oss)"
$env:OSS_ACCESS_KEY_ID = $OssKeyId
$env:OSS_ACCESS_KEY_SECRET = $OssKeySecret
$uploadScript = Join-Path $ScriptDir 'ci-upload-windows.mjs'
& node $uploadScript $Version $Type $ExePath
$uploadExit = $LASTEXITCODE
Remove-Item Env:\OSS_ACCESS_KEY_ID -ErrorAction SilentlyContinue
Remove-Item Env:\OSS_ACCESS_KEY_SECRET -ErrorAction SilentlyContinue
if ($uploadExit -ne 0) { throw "OSS upload failed (exit $uploadExit)" }

# ── 6. Cleanup staging ───────────────────────────────────────────────────
if (-not $KeepStaging) {
    Write-Section "Cleanup: remove staging from OSS"
    $env:OSS_ACCESS_KEY_ID = $OssKeyId
    $env:OSS_ACCESS_KEY_SECRET = $OssKeySecret
    $cleanupArgs = @($Version)
    & node (Join-Path $ScriptDir 'ci-cleanup-staging.mjs') @cleanupArgs
    Remove-Item Env:\OSS_ACCESS_KEY_ID -ErrorAction SilentlyContinue
    Remove-Item Env:\OSS_ACCESS_KEY_SECRET -ErrorAction SilentlyContinue
}

Write-Host ''
Write-Host "════════════════════════════════════════════════════════════════" -ForegroundColor Green
Write-Host " ✓ Windows v$Version ($Type) released" -ForegroundColor Green
Write-Host "════════════════════════════════════════════════════════════════" -ForegroundColor Green
$publicPrefix = if ($Type -eq 'beta') { 'beta/' } else { '' }
Write-Host "  Download: https://lotus.renlijia.com/aijia/${publicPrefix}v$Version/AIjia_${Version}_x64-setup.exe"
