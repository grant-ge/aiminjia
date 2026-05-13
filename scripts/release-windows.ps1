# AIjia Windows release - one-shot end-to-end script.
#
# Downloads unsigned exe from OSS staging, Authenticode-signs it via signtool,
# generates the Tauri updater .sig, uploads everything to the public OSS path,
# and cleans up staging. Zero Python dependency - uses Node (ali-oss) for OSS.
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
#   - Node.js (npm/npx)
#   - signtool.exe in PATH or under Windows SDK
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

# Force PS console + IO to UTF-8 so we don't depend on Windows code page.
try {
    [Console]::OutputEncoding = [System.Text.Encoding]::UTF8
    $OutputEncoding = [System.Text.Encoding]::UTF8
} catch {}

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$RepoRoot  = Split-Path -Parent $ScriptDir
Set-Location $RepoRoot

$ExeName  = "AIjia_${Version}_x64-setup.exe"
$WorkDir  = Join-Path $RepoRoot 'build\windows-unsigned'
$ExePath  = Join-Path $WorkDir $ExeName
$SigPath  = "$ExePath.sig"
$TauriKey = Join-Path $HOME '.tauri\aijia.key'

$StagingBase = "https://lotus.renlijia.com/aijia/staging/unsigned/v$Version"

# -- helpers --------------------------------------------------------------
function Write-Section($title) {
    Write-Host ''
    Write-Host "=== $title ===" -ForegroundColor Cyan
}
function Write-Ok($msg)   { Write-Host "  [OK]   $msg" -ForegroundColor Green }
function Write-Warn($msg) { Write-Host "  [WARN] $msg" -ForegroundColor Yellow }
function Write-Err($msg)  { Write-Host "  [FAIL] $msg" -ForegroundColor Red }

# Find a real signtool.exe (Windows SDK). Prefer the one in PATH.
function Get-Signtool {
    $cmd = Get-Command signtool.exe -ErrorAction SilentlyContinue
    if ($cmd) { return $cmd.Source }
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
    throw 'signtool.exe not found - install the Windows SDK or add it to PATH'
}

# -- credential storage (Windows Credential Manager) ----------------------
# Uses cmdkey.exe (no extra modules needed). Each value stored as a generic
# credential under target name AIjia.<key>.
function Save-Credential {
    param([string]$Name, [string]$Value)
    cmdkey /generic:"AIjia.$Name" /user:aijia /pass:"$Value" | Out-Null
}

# Win32 CredRead wrapper. Build the C# source as a string array and join,
# avoiding here-strings which are parser-fragile under PowerShell 5.1
# (especially around CRLF/LF line endings introduced by git autocrlf).
$credSource = @(
    'using System;',
    'using System.Runtime.InteropServices;',
    'public static class AIjiaCred {',
    '    [DllImport("Advapi32.dll", SetLastError=true, CharSet=CharSet.Unicode)]',
    '    public static extern bool CredRead(string target, int type, int reservedFlag, out IntPtr CredentialPtr);',
    '    [DllImport("Advapi32.dll", SetLastError=true)]',
    '    public static extern void CredFree([In] IntPtr cred);',
    '    [StructLayout(LayoutKind.Sequential, CharSet=CharSet.Unicode)]',
    '    public struct CREDENTIAL {',
    '        public uint Flags;',
    '        public uint Type;',
    '        public string TargetName;',
    '        public string Comment;',
    '        public System.Runtime.InteropServices.ComTypes.FILETIME LastWritten;',
    '        public uint CredentialBlobSize;',
    '        public IntPtr CredentialBlob;',
    '        public uint Persist;',
    '        public uint AttributeCount;',
    '        public IntPtr Attributes;',
    '        public string TargetAlias;',
    '        public string UserName;',
    '    }',
    '    public static string Read(string target) {',
    '        IntPtr ptr;',
    '        if (!CredRead(target, 1, 0, out ptr)) return null;',
    '        try {',
    '            CREDENTIAL cred = (CREDENTIAL)Marshal.PtrToStructure(ptr, typeof(CREDENTIAL));',
    '            return Marshal.PtrToStringUni(cred.CredentialBlob, (int)(cred.CredentialBlobSize / 2));',
    '        } finally {',
    '            CredFree(ptr);',
    '        }',
    '    }',
    '}'
) -join "`n"
if (-not ('AIjiaCred' -as [type])) {
    Add-Type -TypeDefinition $credSource -ErrorAction Stop
}

function Load-Credential {
    param([string]$Name)
    return [AIjiaCred]::Read("AIjia.$Name")
}

function Get-OrPrompt {
    param([string]$Name, [string]$Prompt, [switch]$Secret)
    if (-not $Reconfigure) {
        $existing = Load-Credential -Name $Name
        if ($existing) { return (Sanitize-Input $existing) }
    }
    # NOTE: -AsSecureString hangs on long pasted values (the host stops
    # echoing after the first '*'). The value ends up in Credential Manager
    # via cmdkey (plain-text on disk anyway) so SecureString brings no real
    # security here. Use plain Read-Host and mask the echo by hand if needed.
    $value = Read-Host -Prompt $Prompt
    $value = Sanitize-Input $value
    Save-Credential -Name $Name -Value $value
    return $value
}

# Drop all whitespace + control + non-printable chars. Clipboard pastes
# often add zero-width / BOM / trailing CR which break consumers like
# signtool (Invalid SHA1 hash format).
function Sanitize-Input {
    param([string]$s)
    if ($null -eq $s) { return '' }
    $sb = New-Object System.Text.StringBuilder
    foreach ($ch in $s.ToCharArray()) {
        $code = [int]$ch
        # Keep only printable ASCII (0x21-0x7E).
        if ($code -ge 0x21 -and $code -le 0x7E) { [void]$sb.Append($ch) }
    }
    return $sb.ToString()
}

# -- 0. Sanity: tauri key file present ------------------------------------
if (-not (Test-Path $TauriKey)) {
    Write-Err "Tauri signing key not found at: $TauriKey"
    Write-Host ''
    Write-Host "Create it with the value from the macOS machine (~/.tauri/aijia.key)." -ForegroundColor Yellow
    Write-Host "Example:" -ForegroundColor Yellow
    Write-Host '  New-Item -ItemType Directory -Force -Path "$HOME\.tauri" | Out-Null' -ForegroundColor Yellow
    Write-Host '  Set-Content "$HOME\.tauri\aijia.key" -Value "<base64-from-mac>" -NoNewline -Encoding ASCII' -ForegroundColor Yellow
    exit 1
}

Write-Section "AIjia Windows $Version ($Type) - release pipeline"

# -- 1. Load (or prompt for) credentials ----------------------------------
Write-Section "Step 1/5: Load credentials"
$Thumbprint    = Get-OrPrompt -Name 'Thumbprint'      -Prompt 'Authenticode cert SHA1 thumbprint'
$OssKeyId      = Get-OrPrompt -Name 'OssKeyId'        -Prompt 'OSS_ACCESS_KEY_ID'
$OssKeySecret  = Get-OrPrompt -Name 'OssKeySecret'    -Prompt 'OSS_ACCESS_KEY_SECRET'         -Secret
$TauriKeyPwd   = Get-OrPrompt -Name 'TauriKeyPwd'     -Prompt 'TAURI_SIGNING_PRIVATE_KEY_PASSWORD' -Secret
$thumbPrefix = if ($Thumbprint.Length -ge 8) { $Thumbprint.Substring(0,8) } else { $Thumbprint }
$ossPrefix   = if ($OssKeyId.Length -ge 6) { $OssKeyId.Substring(0,6) } else { $OssKeyId }
Write-Ok "thumbprint=$thumbPrefix... oss key id=$ossPrefix..."

# -- 2. Download unsigned artifacts ---------------------------------------
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
# We always regenerate .sig from the SIGNED exe - drop the staging copy now.
Remove-Item $SigPath -ErrorAction SilentlyContinue

# -- 3. Authenticode sign with signtool -----------------------------------
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
if (-not $auth.TimeStamperCertificate) { Write-Warn 'No timestamp present - signature will expire with cert!' }
$subjectCn = ($auth.SignerCertificate.Subject -replace '^CN=([^,]*).*', '$1')
Write-Ok "signed by $subjectCn"

# -- 4. Generate Tauri updater .sig ---------------------------------------
Write-Section "Step 4/5: Generate Tauri updater signature"
# tauri-cli signer sign supports -f <path> to read key from a file.
# This avoids passing the long base64 key through PowerShell -> npx ->
# child env, which previously got truncated/whitespace-injected.
# (The wrong flag '-k' takes a key STRING and tries to parse a path
# as base64, hence the "Invalid symbol" errors we hit before.)
$env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = $TauriKeyPwd
$tauriCliPkg = '@tauri-apps/cli@latest'
& npx --yes $tauriCliPkg signer sign -f $TauriKey $ExePath
$signerExit = $LASTEXITCODE
Remove-Item Env:\TAURI_SIGNING_PRIVATE_KEY_PASSWORD -ErrorAction SilentlyContinue
if ($signerExit -ne 0) { throw "tauri signer failed (exit $signerExit)" }
if (-not (Test-Path $SigPath)) { throw ".sig not created at $SigPath" }
$sigBytes = (Get-Item $SigPath).Length
Write-Ok "sig: $SigPath ($sigBytes bytes)"

# -- 5. Upload to OSS via Node --------------------------------------------
Write-Section "Step 5/5: Upload to OSS (Node + ali-oss)"
$env:OSS_ACCESS_KEY_ID = $OssKeyId
$env:OSS_ACCESS_KEY_SECRET = $OssKeySecret
$uploadScript = Join-Path $ScriptDir 'ci-upload-windows.mjs'
& node $uploadScript $Version $Type $ExePath
$uploadExit = $LASTEXITCODE
Remove-Item Env:\OSS_ACCESS_KEY_ID -ErrorAction SilentlyContinue
Remove-Item Env:\OSS_ACCESS_KEY_SECRET -ErrorAction SilentlyContinue
if ($uploadExit -ne 0) { throw "OSS upload failed (exit $uploadExit)" }

# -- 6. Cleanup staging ---------------------------------------------------
if (-not $KeepStaging) {
    Write-Section "Cleanup: remove staging from OSS"
    $env:OSS_ACCESS_KEY_ID = $OssKeyId
    $env:OSS_ACCESS_KEY_SECRET = $OssKeySecret
    $cleanupScript = Join-Path $ScriptDir 'ci-cleanup-staging.mjs'
    & node $cleanupScript $Version
    Remove-Item Env:\OSS_ACCESS_KEY_ID -ErrorAction SilentlyContinue
    Remove-Item Env:\OSS_ACCESS_KEY_SECRET -ErrorAction SilentlyContinue
}

Write-Host ''
Write-Host "================================================================" -ForegroundColor Green
Write-Host " [OK] Windows v$Version ($Type) released" -ForegroundColor Green
Write-Host "================================================================" -ForegroundColor Green
$publicPrefix = if ($Type -eq 'beta') { 'beta/' } else { '' }
Write-Host "  Download: https://lotus.renlijia.com/aijia/${publicPrefix}v$Version/AIjia_${Version}_x64-setup.exe"
