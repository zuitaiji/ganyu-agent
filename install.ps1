# ============================================================================
# ganyu-agent 一键安装脚本 v2（Windows PowerShell 5.1+）
#
# 用法（Hermes 式，一条命令）：
#   iex (irm https://raw.githubusercontent.com/zuitaiji/ganyu-agent/main/install.ps1)
#
# 行为：
#   - 默认【免编译】：从 GitHub Releases 下载预编译二进制（hardened 特性），
#     装到独立目录 $Prefix（默认 ~/.ganyu），零 Rust 依赖，删目录即卸载。
#   - 指定 GANYU_FEATURES 时回退【源码编译】：本地有仓库用本地源码，否则 clone，
#     适合要定制特性的开发者。
#   - 幂等：重复执行覆盖升级二进制，不动 ~/.ganyu/config.toml 与记忆文件。
#   - 自带 selftest 自检 + 别名 ganyu.exe + PATH 提示。
#
# 参数（iex (irm ...) 单行执行不支持脚本级 param()，故用环境变量）：
#   GANYU_VERSION    release 版本（默认 latest）
#   GANYU_PREFIX     安装前缀（默认 %USERPROFILE%\.ganyu）
#   GANYU_FEATURES   指定后走 cargo 编译（如 "hardened"）
#   GANYU_REPO / GANYU_BRANCH   源码编译时的仓库与分支
#   GANYU_DEV=1      源码编译用 dev profile（快，未优化）
#   GANYU_NOALIAS=1  跳过别名创建
# 例：$env:GANYU_FEATURES="hardened"; iex (irm https://raw.githubusercontent.com/zuitaiji/ganyu-agent/main/install.ps1)
# ============================================================================
$ErrorActionPreference = "Stop"

# ---- 参数：环境变量注入（兼容 iex 单行执行） -------------------------------
$Version  = if ($env:GANYU_VERSION)  { $env:GANYU_VERSION }  else { "latest" }
$Prefix   = if ($env:GANYU_PREFIX)   { $env:GANYU_PREFIX }   else { "" }
$Features = if ($env:GANYU_FEATURES) { $env:GANYU_FEATURES } else { "" }
$Repo     = if ($env:GANYU_REPO)     { $env:GANYU_REPO }     else { "https://github.com/zuitaiji/ganyu-agent" }
$Branch   = if ($env:GANYU_BRANCH)   { $env:GANYU_BRANCH }   else { "main" }
$Dev      = $env:GANYU_DEV -eq "1"
$NoAlias  = $env:GANYU_NOALIAS -eq "1"

# 官方发布公钥（ed25519 原始 32 字节 hex）。与 scripts/sign-release.py、src/main.rs
# 验签端共用；可用 GANYU_UPDATE_PUBKEY 覆盖。轮换见 docs/update-signing.md §3。
$OfficialPubKey = if ($env:GANYU_UPDATE_PUBKEY) { $env:GANYU_UPDATE_PUBKEY } else { "241db1db27d3c19c58df6a35de52a158080e310bdeb57c50ddca8e5c647b9ba4" }

if (-not $Prefix) { $Prefix = Join-Path $HOME ".ganyu" }
$binDir = Join-Path $Prefix "bin"
$binPath = Join-Path $binDir "ganyu-agent.exe"

# ---- 平台检测 → release 资产名 ----------------------------------------------
function Get-AssetName {
  $arch = $env:PROCESSOR_ARCHITECTURE
  if ($arch -eq "ARM64") {
    return "ganyu-agent-windows-arm64.tar.gz"
  }
  return "ganyu-agent-windows-x86_64.tar.gz"
}

# ---- ed25519 签名校验（R-1 信任锚点） ------------------------------------------
# 依赖 python3(cryptography) 或 openssl；二者皆无则拒绝安装（fail-closed）。
function Test-Ed25519Signature {
  param($FilePath, $SigPath, $PubHex)
  if (-not (Test-Path $SigPath) -or (Get-Item $SigPath).Length -eq 0) {
    Write-Error ".sig 文件为空或缺失"
    return $false
  }
  # 1) python3 + cryptography
  $py = Get-Command python3 -ErrorAction SilentlyContinue
  if ($py) {
    $vf = Join-Path ([IO.Path]::GetTempPath()) "ganyu_verify_$(Get-Random).py"
    @"
import sys, binascii
pub = binascii.unhexlify(sys.argv[1])
msg = open(sys.argv[2], 'rb').read()
sig = open(sys.argv[3], 'rb').read()
if len(sig) != 64:
    sys.stderr.write('sig length != 64\n'); sys.exit(1)
try:
    from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PublicKey
    Ed25519PublicKey.from_public_bytes(pub).verify(sig, msg)
except ImportError:
    sys.exit(2)
except Exception as e:
    sys.stderr.write('ed25519 verify failed: %s\n' % e); sys.exit(1)
"@ | Set-Content -Path $vf -Encoding UTF8
    & python3 $vf $PubHex $FilePath $SigPath
    $rc = $LASTEXITCODE
    Remove-Item $vf -Force -ErrorAction SilentlyContinue
    if ($rc -eq 0) { return $true }
    if ($rc -ne 2) { return $false }   # 真失败（非依赖缺失）
  }
  # 2) openssl
  $ossl = Get-Command openssl -ErrorAction SilentlyContinue
  if ($ossl) {
    $bytes = @()
    for ($i = 0; $i -lt $PubHex.Length; $i += 2) { $bytes += [Convert]::ToByte($PubHex.Substring($i, 2), 16) }
    $der = [Convert]::ToBase64String(([byte[]]@(0x30, 0x2a, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x03, 0x21, 0x00) + $bytes))
    $pemPath = Join-Path ([IO.Path]::GetTempPath()) "ganyu_pub_$(Get-Random).pem"
    "-----BEGIN PUBLIC KEY-----`n$der`n-----END PUBLIC KEY-----" | Set-Content -Path $pemPath -Encoding ASCII
    & openssl pkeyutl -verify -pubin -rawin -in $FilePath -sigfile $SigPath -inkey $pemPath 2>$null
    $rc = $LASTEXITCODE
    Remove-Item $pemPath -Force -ErrorAction SilentlyContinue
    return ($rc -eq 0)
  }
  Write-Error "无 python3(cryptography) 也无 openssl，无法校验 ed25519 签名，拒绝安装"
  return $false
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

  # 供应链校验：下载配套 .sha256 并对比（release 资产由 CI 生成）
  $shaPath = Join-Path ([IO.Path]::GetTempPath()) "$asset.sha256"
  try {
    Invoke-WebRequest -Uri "$downloadUrl.sha256" -OutFile $shaPath -UseBasicParsing
    $expected = ((Get-Content $shaPath -Raw).Trim() -split '\s+')[0]
    $actual = (Get-FileHash -Path $zipPath -Algorithm SHA256).Hash.ToLower()
    if ($actual -ne $expected) {
      Remove-Item $zipPath, $shaPath -Force -ErrorAction SilentlyContinue
      Write-Error "sha256 校验失败：资产可能被篡改！`n期望 $expected`n实际 $actual"
    }
    Write-Host "[install] sha256 校验通过" -ForegroundColor Green
  } catch {
    Write-Host "[install] 警告：未获取到 sha256 校验文件（跳过校验）" -ForegroundColor Yellow
  }
  Remove-Item $shaPath -Force -ErrorAction SilentlyContinue

  # 供应链强校验（R-1）：下载配套 .sig 并用官方公钥验签，缺失或失败均拒绝安装。
  $sigPath = Join-Path ([IO.Path]::GetTempPath()) "$asset.sig"
  Write-Host "[install] 校验 ed25519 签名（R-1）" -ForegroundColor Green
  try {
    Invoke-WebRequest -Uri "$downloadUrl.sig" -OutFile $sigPath -UseBasicParsing
    if (Test-Ed25519Signature -FilePath $zipPath -SigPath $sigPath -PubHex $OfficialPubKey) {
      Write-Host "[install] ed25519 签名校验通过" -ForegroundColor Green
    } else {
      Remove-Item $zipPath, $sigPath -Force -ErrorAction SilentlyContinue
      Write-Error "ed25519 签名校验失败：资产来源不可信，已拒绝安装！"
    }
  } catch {
    Remove-Item $zipPath -Force -ErrorAction SilentlyContinue
    Write-Error "未获取到 .sig 签名文件（发布不完整），拒绝安装"
  }
  Remove-Item $sigPath -Force -ErrorAction SilentlyContinue

  # Windows 10 1803+ 自带 bsdtar；tar.gz 统一解压（比 Expand-Archive 更稳）。
  & tar -xzf $zipPath -C $binDir
  if ($LASTEXITCODE -ne 0) { Write-Error "解压失败：tar -xzf $zipPath -C $binDir" }
  Remove-Item $zipPath -Force -ErrorAction SilentlyContinue

  # 资产内文件名统一为 ganyu-agent（无扩展名）；Windows 对齐为 ganyu-agent.exe。
  # 升级场景（已有旧 exe）同样需要同步，不能只看 Test-Path。
  $unpacked = Join-Path $binDir "ganyu-agent"
  if (Test-Path $unpacked) {
    if (Test-Path $binPath) { Remove-Item $binPath -Force -ErrorAction SilentlyContinue }
    if (-not (Test-Path $binPath)) { Rename-Item $unpacked $binPath -Force -ErrorAction SilentlyContinue }
    if (-not (Test-Path $binPath)) { Copy-Item $unpacked $binPath -Force -ErrorAction SilentlyContinue }
  }
  if (-not (Test-Path $binPath)) {
    Write-Error "安装失败：$binPath 无法写入。ganyu 正在运行？请先退出所有 ganyu/ganyu-agent 会话后重试。"
  }
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
  # 构建缓存统一目录（幂等升级增量编译）；可用 GANYU_CARGO_TARGET_DIR 覆盖。
  if (-not $env:GANYU_CARGO_TARGET_DIR) {
    $env:GANYU_CARGO_TARGET_DIR = Join-Path $Prefix ".build-cache"
  }
  New-Item -ItemType Directory -Path $env:GANYU_CARGO_TARGET_DIR -Force | Out-Null
  # 锁自愈：清除残留的 cargo 构建锁（写拦截/中断可能遗留），避免下次构建卡死。
  foreach ($lf in @(
    "$env:GANYU_CARGO_TARGET_DIR\.cargo-build-lock",
    "$env:GANYU_CARGO_TARGET_DIR\.cargo-lock",
    "$env:GANYU_CARGO_TARGET_DIR\release\.cargo-build-lock",
    "$env:GANYU_CARGO_TARGET_DIR\release\.cargo-lock"
  )) { if (Test-Path $lf) { Remove-Item $lf -Force -ErrorAction SilentlyContinue } }
  Write-Host "[install] 构建缓存: $env:GANYU_CARGO_TARGET_DIR（升级将增量编译）"
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

# ---- PATH 注册（用户级，装完即用；幂等） ------------------------------------
$userPath = [Environment]::GetEnvironmentVariable("Path", "User")
if ($userPath -notlike "*$binDir*") {
  $newPath = if ($userPath) { "$userPath;$binDir" } else { $binDir }
  [Environment]::SetEnvironmentVariable("Path", $newPath, "User")
  Write-Host "[install] 已将 $binDir 写入用户级 PATH（新终端生效）" -ForegroundColor Green
} else {
  Write-Host "[install] $binDir 已在用户级 PATH"
}
# 当前会话同步：本会话立即可用 ganyu，无需重启终端
if ($env:Path -notlike "*$binDir*") { $env:Path = "$binDir;$env:Path" }

Write-Host ""
Write-Host "[install] 安装完成。快速体验：" -ForegroundColor Green
Write-Host "          ganyu-agent selftest"
Write-Host "          ganyu-agent doctor"
Write-Host "          配置模型（交互式向导，推荐）：ganyu-agent setup"
Write-Host "          直接对话：ganyu-agent chat   （或 ganyu）"
Write-Host "          升级：ganyu-agent update"
Write-Host "          查看/切换模型：ganyu-agent model"
Write-Host "          接 Telegram：ganyu-agent gateway start"
