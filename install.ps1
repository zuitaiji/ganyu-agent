# ============================================================================
# ganyu-agent 一键安装脚本（Windows PowerShell 5.1+）
#
# 用法：
#   本地（仓库内）：      .\install.ps1 -Features hardened -Prefix "$HOME\.ganyu"
#   一条命令（远程）：   iex (irm <install.ps1 直链>)
#
# 与 install.sh 相同的设计原则：默认零依赖构建、能力按需开启、独立 PREFIX、
# 自带 selftest 自检与 PATH 提示、幂等升级不动数据。
# ============================================================================
[CmdletBinding()]
param(
  [string]$Features = "",          # 特性组合，如 "hardened" / "network" / "crypto,secret"
  [string]$Prefix = "",            # 安装前缀，默认 $HOME\.ganyu
  [string]$Repo = "https://github.com/zuitaiji/ganyu-agent.git",
  [string]$Branch = "main",
  [switch]$Dev,                    # 用 dev profile 构建（快，未优化；验证/CI 用）
  [switch]$NoAlias
)

$ErrorActionPreference = "Stop"

if (-not $Prefix) { $Prefix = Join-Path $HOME ".ganyu" }
$binDir = Join-Path $Prefix "bin"
$binPath = Join-Path $binDir "ganyu-agent.exe"

# ---- 前置检查：cargo ----------------------------------------------------------
$cargo = Get-Command cargo -ErrorAction SilentlyContinue
if (-not $cargo) {
  Write-Host "[install] 未检测到 cargo。请先安装 Rust（rustup）：" -ForegroundColor Yellow
  Write-Host "          在 https://rustup.rs 下载 rustup-init.exe 并安装。"
  Write-Host "          然后重新打开终端重试。"
  exit 1
}

# ---- 定位源码 ---------------------------------------------------------------
$src = $PSScriptRoot
$isRepo = (Test-Path (Join-Path $src "Cargo.toml")) -and (Test-Path (Join-Path $src "src\main.rs"))
if (-not $isRepo) {
  $tmp = Join-Path ([IO.Path]::GetTempPath()) "ganyu-agent-src"
  if (Test-Path $tmp) { Remove-Item $tmp -Recurse -Force -ErrorAction SilentlyContinue }
  Write-Host "[install] 克隆 $Repo@$Branch ..."
  git clone --depth 1 --branch $Branch $Repo $tmp | Out-Null
  if ($LASTEXITCODE -ne 0) { Write-Error "git clone 失败"; exit 1 }
  $src = $tmp
}

# ---- 构建并安装 -------------------------------------------------------------
$featArgs = @()
if ($Features) { $featArgs = @("--features", $Features) }
if ($Dev) { $featArgs += "--debug" }
$featLabel = if ($Features) { $Features } else { "<default>" }
Write-Host "[install] cargo install --path '$src' --root '$Prefix' --features '$featLabel' $(if ($Dev) {'[dev profile]'} else {'[release]'})"

# 构建目录：默认持久缓存在 $Prefix\target（幂等升级时增量编译），
# 可用环境变量 GANYU_CARGO_TARGET_DIR 覆盖（如 CI 指定缓存目录）。
if (-not $env:GANYU_CARGO_TARGET_DIR) {
  $env:GANYU_CARGO_TARGET_DIR = Join-Path $Prefix "target"
}
New-Item -ItemType Directory -Path $env:GANYU_CARGO_TARGET_DIR -Force | Out-Null

& cargo install --path $src --root $Prefix --locked @featArgs
if ($LASTEXITCODE -ne 0) { Write-Error "cargo install 失败"; exit 1 }

# ---- 自检 --------------------------------------------------------------------
# selftest 依赖仓库内 examples/sample_mdl.json，需切到源码目录执行；
# 且 native 命令非零退出不应被 $ErrorActionPreference=Stop 当作致命错误。
Write-Host "[install] 自检: $binPath selftest"
$prevEAP = $ErrorActionPreference
$ErrorActionPreference = "Continue"
Push-Location $src
& $binPath selftest 2>&1 | Out-Null
$selftestExit = $LASTEXITCODE
Pop-Location
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
Write-Host "          ganyu-agent tools"
Write-Host "          ganyu-agent doctor"
Write-Host "          开箱即用（配置模型）：编辑 ~/.ganyu/config.toml，写入"
Write-Host '            [model]'
Write-Host '            base_url = "https://api.openai.com/v1"'
Write-Host '            api_key = "sk-..."'
Write-Host '            model = "你的模型id"'
Write-Host "          然后直接对话：ganyu-agent chat"
Write-Host "          生产建议: 重新安装并加 -Features hardened（记忆加密/限速/审计）"
