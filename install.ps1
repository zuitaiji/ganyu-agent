# ============================================================================
# ganyu-agent 一键安装脚本 v2（Windows PowerShell 5.1+）
#
# 用法（Hermes 式，一条命令）：
#   iex (irm https://raw.githubusercontent.com/zuitaiji/ganyu-agent/main/install.ps1)
#
# 行为：
#   - 默认【免编译】：从 GitHub Releases 下载预编译二进制（hardened 特性），
#     装到独立目录 $Prefix（默认 ~/.ganyu），零 Rust 依赖，删目录即卸载。
#   - 指定 -Features 时回退【源码编译】：本地有仓库用本地源码，否则 clone，
#     适合要定制特性的开发者。
#   - 幂等：重复执行覆盖升级二进制，不动 ~/.ganyu/config.toml 与记忆文件。
#   - 自带 selftest 自检 + 别名 ganyu.exe + PATH 提示。
# ============================================================================
[CmdletBinding()]
param(
  [string]$Version = "latest",     # release 版本：latest 或 v0.1.0
  [string]$Prefix = "",            # 安装前缀，默认 $HOME\.ganyu
  [string]$Features = "",          # 指定后走 cargo 编译（如 "hardened"）
  [string]$Repo = "https://github.com/zuitaiji/ganyu-agent",
  [string]$Branch = "main",
  [switch]$Dev,                    # 源码编译时用 dev profile（快，未优化）
  [switch]$NoAlias
)

$ErrorActionPreference = "Stop"
if (-not $Prefix) { $Prefix = Join-Path $HOME ".ganyu" }
$binDir = Join-Path $Prefix "bin"
$binPath = Join-Path $binDir "ganyu-agent.exe"

# ---- 平台检测 → release 资产名 ----------------------------------------------
function Get-AssetName {
  $arch = $env:PROCESSOR_ARCHITECTURE
  if ($arch -eq "ARM64") {
    return "ganyu-agent-windows-arm64.zip"
  }
  return "ganyu-agent-windows-x86_64.zip"
}

# ---- 路径一：免编译下载（默认） ----------------------------------------------
if (-not $Features) {
  $asset = Get-AssetName
  Write-Host "[install] 免编译安装（下载预编译 hardened 二进制）..."
  Write-Host "[install]   release: $Version / asset: $asset"

  # 解析 release 资产下载地址（公开仓库，无需 token）
  $apiUrl = "https://api.github.com/repos/zuitaiji/ganyu-agent/releases/$Version"
  $release = Invoke-RestMethod -Uri $apiUrl -Headers @{ "User-Agent" = "ganyu-install" }
  $assetObj = $release.assets | Where-Object { $_.name -eq $asset }
  if (-not $assetObj) {
    $avail = ($release.assets | ForEach-Object { $_.name }) -join ", "
    Write-Error "release $Version 中未找到资产 $asset。可用资产: $avail"
  }
  $downloadUrl = $assetObj.browser_download_url

  New-Item -ItemType Directory -Path $binDir -Force | Out-Null
  $zipPath = Join-Path ([IO.Path]::GetTempPath()) $asset
  Write-Host "[install] 下载 $downloadUrl"
  Invoke-WebRequest -Uri $downloadUrl -OutFile $zipPath -UseBasicParsing
  Expand-Archive -Path $zipPath -DestinationPath $binDir -Force
  Remove-Item $zipPath -Force -ErrorAction SilentlyContinue

  # 资产内可能直接是 exe（zip 内为 ganyu-agent.exe）；兜底处理
  if (-not (Test-Path $binPath)) {
    $inner = Get-ChildItem -Path $binDir -Filter "ganyu-agent*" -File | Select-Object -First 1
    if ($inner) { Copy-Item $inner.FullName $binPath -Force }
  }
  if (-not (Test-Path $binPath)) { Write-Error "安装失败：$binPath 不存在" }
}
else {
  # ---- 路径二：源码编译（指定 -Features） ------------------------------------
  $cargo = Get-Command cargo -ErrorAction SilentlyContinue
  if (-not $cargo) {
    Write-Host "[install] 未检测到 cargo。免编译安装无需 cargo；" -ForegroundColor Yellow
    Write-Host "          如需 -Features 定制编译，请先安装 Rust：https://rustup.rs" -ForegroundColor Yellow
    exit 1
  }
  $src = $PSScriptRoot
  $isRepo = (Test-Path (Join-Path $src "Cargo.toml")) -and (Test-Path (Join-Path $src "src\main.rs"))
  if (-not $isRepo) {
    $tmp = Join-Path ([IO.Path]::GetTempPath()) "ganyu-agent-src"
    if (Test-Path $tmp) { Remove-Item $tmp -Recurse -Force -ErrorAction SilentlyContinue }
    Write-Host "[install] 克隆 $Repo@$Branch ..."
    git clone --depth 1 --branch $Branch "$Repo.git" $tmp | Out-Null
    if ($LASTEXITCODE -ne 0) { Write-Error "git clone 失败"; exit 1 }
    $src = $tmp
  }
  $featArgs = @("--features", $Features)
  if ($Dev) { $featArgs += "--debug" }
  if (-not $env:GANYU_CARGO_TARGET_DIR) {
    $env:GANYU_CARGO_TARGET_DIR = Join-Path $Prefix "target"
  }
  New-Item -ItemType Directory -Path $env:GANYU_CARGO_TARGET_DIR -Force | Out-Null
  Write-Host "[install] cargo install --path '$src' --root '$Prefix' $(if ($Dev) {'[dev]'} else {'[release]'})"
  & cargo install --path $src --root $Prefix --locked @featArgs
  if ($LASTEXITCODE -ne 0) { Write-Error "cargo install 失败"; exit 1 }
}

# ---- 自检 --------------------------------------------------------------------
Write-Host "[install] 自检: $binPath selftest"
$prevEAP = $ErrorActionPreference
$ErrorActionPreference = "Continue"
& $binPath selftest 2>&1 | Out-Null
$selftestExit = $LASTEXITCODE
$ErrorActionPreference = $prevEAP
if ($selftestExit -eq 0) {
  Write-Host "[install] selftest 通过" -ForegroundColor Green
} else {
  Write-Host "[install] 警告：selftest 未通过（exit=$selftestExit），请反馈" -ForegroundColor Yellow
}

# ---- 别名 --------------------------------------------------------------------
if (-not $NoAlias) {
  $aliasPath = Join-Path $binDir "ganyu.exe"
  Copy-Item $binPath $aliasPath -Force -ErrorAction SilentlyContinue
  Write-Host "[install] 已创建别名: $aliasPath"
}

# ---- PATH 提示 ---------------------------------------------------------------
$inPath = ($env:Path -split ";") -contains $binDir
if (-not $inPath) {
  Write-Host ""
  Write-Host "[install] 请把以下目录加入 PATH（当前会话）：" -ForegroundColor Cyan
  Write-Host ('          $env:Path = "' + $binDir + ';$env:Path"')
  Write-Host ('          永久生效：setx PATH "' + $binDir + ';%PATH%"')
}

Write-Host ""
Write-Host "[install] 安装完成。快速体验：" -ForegroundColor Green
Write-Host "          ganyu-agent selftest"
Write-Host "          ganyu-agent doctor"
Write-Host "          配置模型（交互式向导，推荐）：ganyu-agent setup"
Write-Host "          直接对话：ganyu-agent chat   （或 ganyu）"
Write-Host "          升级：ganyu-agent update"
Write-Host "          查看/切换模型：ganyu-agent model"
Write-Host "          接 Telegram：ganyu-agent gateway start"
