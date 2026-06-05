<#
.SYNOPSIS
  下载 LongMemEval 数据集到本地（仓库内 tools/longmemeval/data/，已 gitignore）。

.DESCRIPTION
  从国内镜像 hf-mirror.com 拉取 longmemeval-cleaned 数据集。
  默认下 oracle(小) + s(264MB)。加 -IncludeM 再下 m(超大)。

.EXAMPLE
  pwsh tools/longmemeval/download.ps1
  pwsh tools/longmemeval/download.ps1 -IncludeM
#>
param(
    [switch]$IncludeM
)

$ErrorActionPreference = "Stop"
$base = "https://hf-mirror.com/datasets/xiaowu0162/longmemeval-cleaned/resolve/main"
$dataDir = Join-Path $PSScriptRoot "data"
New-Item -ItemType Directory -Force -Path $dataDir | Out-Null

$files = @("longmemeval_oracle.json", "longmemeval_s_cleaned.json")
if ($IncludeM) { $files += "longmemeval_m_cleaned.json" }

foreach ($f in $files) {
    $out = Join-Path $dataDir $f
    if (Test-Path $out) {
        Write-Host "已存在，跳过: $f" -ForegroundColor Yellow
        continue
    }
    Write-Host "下载 $f ..." -ForegroundColor Cyan
    # curl.exe 跟随 308 重定向；Invoke-WebRequest 在 PS5.1 下不跟 308。
    curl.exe -L -s -o $out "$base/$f"
    $mb = [math]::Round((Get-Item $out).Length / 1MB, 2)
    Write-Host "完成: $f ($mb MB)" -ForegroundColor Green
}

Write-Host "`n数据已就位于: $dataDir" -ForegroundColor Green
Write-Host "运行评测前，设置 `$env:LME_DATA 指向其中一个 json（见 README.md）。"
