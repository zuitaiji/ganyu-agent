#!/usr/bin/env bash
# ============================================================================
# ganyu-agent 一键安装脚本 v2（Linux / macOS / Git-Bash）
#
# 用法（Hermes 式，一条命令）：
#   curl -fsSL https://raw.githubusercontent.com/zuitaiji/ganyu-agent/main/install.sh | bash
#
# 行为：
#   - 默认【免编译】：从 GitHub Releases 下载预编译二进制（hardened 特性），
#     装到独立目录 $PREFIX（默认 ~/.local），零 Rust 依赖，删目录即卸载。
#   - 指定 --features 时回退【源码编译】：本地有仓库用本地源码，否则 clone。
#   - 幂等：重复执行覆盖升级二进制，不动 config.toml 与记忆文件。
#   - 自带 selftest 自检 + 别名 ganyu + PATH 提示。
# ============================================================================
set -euo pipefail

# ---- 可配置项 -----------------------------------------------------------------
GANYU_VERSION="${GANYU_VERSION:-latest}"      # release 版本：latest 或 v0.1.0
PREFIX="${PREFIX:-$HOME/.local}"
GANYU_FEATURES="${GANYU_FEATURES:-}"          # 指定后走 cargo 编译
GANYU_REPO="${GANYU_REPO:-https://github.com/zuitaiji/ganyu-agent}"
GANYU_BRANCH="${GANYU_BRANCH:-main}"
CREATE_ALIAS="${GANYU_CREATE_ALIAS:-1}"

# 官方发布公钥（ed25519 原始 32 字节 hex）。与 scripts/sign-release.py 签名端、
# src/main.rs 的 verify_update_signature 验签端共用同一把钥匙。
# 轮换流程见 docs/update-signing.md §3。
GANYU_OFFICIAL_PUBKEY="${GANYU_UPDATE_PUBKEY:-241db1db27d3c19c58df6a35de52a158080e310bdeb57c50ddca8e5c647b9ba4}"

# ---- 简易参数解析 -------------------------------------------------------------
while [[ $# -gt 0 ]]; do
  case "$1" in
    --version)   GANYU_VERSION="$2"; shift 2 ;;
    --features)  GANYU_FEATURES="$2"; shift 2 ;;
    --prefix)    PREFIX="$2"; shift 2 ;;
    --branch)    GANYU_BRANCH="$2"; shift 2 ;;
    --repo)      GANYU_REPO="$2"; shift 2 ;;
    --no-alias)  CREATE_ALIAS=0; shift ;;
    -h|--help)
      echo "用法: bash install.sh [--version vX] [--features <f1,f2>] [--prefix <dir>] [--no-alias]"
      echo "  默认免编译下载 release；--features 走 cargo 编译。特性示例: hardened | network | crypto,secret"
      exit 0 ;;
    *) echo "未知参数: $1"; exit 2 ;;
  esac
done

BIN_DIR="$PREFIX/bin"
BIN="$BIN_DIR/ganyu-agent"

# 平台 → release 资产名
detect_asset() {
  local os arch
  os="$(uname -s)"
  arch="$(uname -m)"
  case "$os" in
    Linux*)  echo "ganyu-agent-linux-$([ "$arch" = "aarch64" ] && echo arm64 || echo x86_64).tar.gz" ;;
    Darwin*) echo "ganyu-agent-macos-$([ "$arch" = "arm64" ] && echo arm64 || echo x86_64).tar.gz" ;;
    MINGW*|MSYS*|CYGWIN*) echo "ganyu-agent-windows-x86_64.tar.gz" ;;
    *) echo "不支持的平台: $os/$arch" >&2; exit 1 ;;
  esac
}

# ---- ed25519 签名校验（R-1 信任锚点） ------------------------------------------
# 依赖 python3(cryptography) 或 openssl；二者皆无则拒绝安装（fail-closed）。
# $1=资产文件 $2=.sig 文件 $3=公钥 hex
verify_ed25519() {
  local file="$1" sig="$2" pub="$3" vf rc
  if [[ ! -s "$sig" ]]; then echo "[install] ❌ .sig 文件为空或缺失" >&2; return 1; fi
  if command -v python3 >/dev/null 2>&1; then
    vf="$(mktemp)"
    cat >"$vf" <<'PYEOF'
import sys, binascii
pub = binascii.unhexlify(sys.argv[1])
msg = open(sys.argv[2], 'rb').read()
sig = open(sys.argv[3], 'rb').read()
if len(sig) != 64:
    sys.stderr.write("sig length != 64\n"); sys.exit(1)
try:
    from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PublicKey
    Ed25519PublicKey.from_public_bytes(pub).verify(sig, msg)
except ImportError:
    sys.exit(2)   # cryptography 缺失，交由 openssl 兜底
except Exception as e:
    sys.stderr.write("ed25519 verify failed: %s\n" % e); sys.exit(1)
PYEOF
    python3 "$vf" "$pub" "$file" "$sig"; rc=$?
    rm -f "$vf"
    if [[ $rc -eq 0 ]]; then return 0; fi
    if [[ $rc -ne 2 ]]; then return 1; fi   # 真失败（非依赖缺失）
  fi
  if command -v openssl >/dev/null 2>&1; then
    # 原始 32 字节公钥 → SubjectPublicKeyInfo DER → PEM
    local der pem
    der="$(printf '\x30\x2a\x30\x05\x06\x03\x2b\x65\x70\x03\x21\x00' | cat - <(printf '%s' "$pub" | xxd -r -p) | base64 -w0)"
    pem="-----BEGIN PUBLIC KEY-----
$der
-----END PUBLIC KEY-----"
    if printf '%s\n' "$pem" | openssl pkeyutl -verify -pubin -rawin -in "$file" -sigfile "$sig" >/dev/null 2>&1; then
      return 0
    fi
    return 1
  fi
  echo "[install] ❌ 无 python3(cryptography) 也无 openssl，无法校验 ed25519 签名，拒绝安装" >&2
  echo "[install]   请安装 python3+cryptography 或 openssl 后重试，或手动核对签名。" >&2
  return 1
}

# ---- 路径一：免编译下载（默认） ------------------------------------------------
if [[ -z "$GANYU_FEATURES" ]]; then
  ASSET="$(detect_asset)"
  echo "[install] 免编译安装（下载预编译 hardened 二进制）..."
  echo "[install]   release: $GANYU_VERSION / asset: $ASSET"

  API_URL="https://api.github.com/repos/zuitaiji/ganyu-agent/releases/$GANYU_VERSION"
  if ! command -v curl >/dev/null 2>&1; then
    echo "[install] 需要 curl 下载 release"; exit 1
  fi
  DL_URL="$(curl -fsSL -H 'User-Agent: ganyu-install' "$API_URL" \
    | grep -o "https://[^\"]*/$ASSET" | head -1)"
  if [[ -z "$DL_URL" ]]; then
    echo "[install] release $GANYU_VERSION 中未找到资产 $ASSET" >&2
    curl -fsSL -H 'User-Agent: ganyu-install' "$API_URL" \
      | grep -o '"name": "[^"]*"' | sed 's/"name": //' | head -10 >&2
    exit 1
  fi

  mkdir -p "$BIN_DIR"
  TMP="$(mktemp -d)"
  trap 'rm -rf "$TMP"' EXIT
  echo "[install] 下载 $DL_URL"
  curl -fsSL "$DL_URL" -o "$TMP/$ASSET"

  # 供应链校验：下载配套 .sha256 并对比（release 资产由 CI 生成）
  # macOS 无 sha256sum（仅自带 shasum -a 256），两者输出格式一致（"hash  filename"）。
  echo "[install] 校验 sha256"
  if command -v sha256sum >/dev/null 2>&1; then
    CHECK="sha256sum -c"
  else
    CHECK="shasum -a 256 -c"
  fi
  if curl -fsSL "$DL_URL.sha256" -o "$TMP/$ASSET.sha256" 2>/dev/null; then
    if (cd "$TMP" && $CHECK "$ASSET.sha256") >/dev/null 2>&1; then
      echo "[install] ✅ sha256 校验通过"
    else
      echo "[install] ❌ sha256 校验失败：资产可能被篡改！" >&2
      exit 1
    fi
  else
    echo "[install] ⚠️ 未获取到 sha256 校验文件（跳过校验）"
  fi

  # 供应链强校验（R-1）：下载配套 .sig 并用官方公钥验签，缺失或失败均拒绝安装。
  echo "[install] 校验 ed25519 签名（R-1）"
  if curl -fsSL "$DL_URL.sig" -o "$TMP/$ASSET.sig" 2>/dev/null && [[ -s "$TMP/$ASSET.sig" ]]; then
    if verify_ed25519 "$TMP/$ASSET" "$TMP/$ASSET.sig" "$GANYU_OFFICIAL_PUBKEY"; then
      echo "[install] ✅ ed25519 签名校验通过"
    else
      echo "[install] ❌ ed25519 签名校验失败：资产来源不可信，已拒绝安装！" >&2
      exit 1
    fi
  else
    echo "[install] ❌ 未获取到 .sig 签名文件（发布不完整），拒绝安装" >&2
    exit 1
  fi

  case "$ASSET" in
    *.zip)    (cd "$TMP" && unzip -o "$ASSET" >/dev/null) ;;
    *.tar.gz) (cd "$TMP" && tar -xzf "$ASSET") ;;
  esac
  # 资产内直接是 ganyu-agent 二进制
  if [[ ! -f "$BIN" ]]; then
    find "$TMP" -name "ganyu-agent" -type f -exec cp {} "$BIN" \; 2>/dev/null || true
  fi
  chmod +x "$BIN" 2>/dev/null || true
  if [[ ! -f "$BIN" ]]; then
    echo "[install] 安装失败：$BIN 不存在" >&2; exit 1
  fi

# ---- 路径二：源码编译（指定 --features） ----------------------------------------
else
  if ! command -v cargo >/dev/null 2>&1; then
    echo "[install] 未检测到 cargo。免编译安装无需 cargo；" >&2
    echo "          --features 定制编译需先装 Rust: https://rustup.rs" >&2
    exit 1
  fi
  SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
  if [[ -f "$SCRIPT_DIR/Cargo.toml" && -f "$SCRIPT_DIR/src/main.rs" ]]; then
    SRC="$SCRIPT_DIR"
  else
    TMP_SRC="$(mktemp -d)/ganyu-agent"
    echo "[install] 克隆 $GANYU_REPO@$GANYU_BRANCH ..."
    git clone --depth 1 --branch "$GANYU_BRANCH" "$GANYU_REPO.git" "$TMP_SRC"
    SRC="$TMP_SRC"
  fi
  # 构建缓存统一目录（幂等升级增量编译）；可用 GANYU_CARGO_TARGET_DIR 覆盖。
  if [[ -z "${GANYU_CARGO_TARGET_DIR:-}" ]]; then
    TARGET_DIR="$HOME/.ganyu/.build-cache"
  else
    TARGET_DIR="$GANYU_CARGO_TARGET_DIR"
  fi
  mkdir -p "$TARGET_DIR"
  # 锁自愈：清除残留的 cargo 构建锁（写拦截/中断可能遗留），避免下次构建卡死。
  rm -f "$TARGET_DIR/.cargo-build-lock" "$TARGET_DIR/.cargo-lock" \
        "$TARGET_DIR/release/.cargo-build-lock" "$TARGET_DIR/release/.cargo-lock" 2>/dev/null || true
  echo "[install] 构建缓存: $TARGET_DIR（升级将增量编译）"
  CARGO_TARGET_DIR="$TARGET_DIR" cargo install --path "$SRC" --root "$PREFIX" \
    --locked --features "$GANYU_FEATURES"
  chmod +x "$BIN"
fi

# ---- 自检 ---------------------------------------------------------------------
echo "[install] 自检: $BIN selftest"
if "$BIN" selftest >/tmp/ganyu_selftest.log 2>&1; then
  echo "[install] ✅ selftest 通过"
else
  echo "[install] ⚠️ selftest 失败（exit=$?），日志: /tmp/ganyu_selftest.log"
fi

# ---- 可选别名 -----------------------------------------------------------------
if [[ "$CREATE_ALIAS" == "1" ]]; then
  ln -sf "$BIN" "$BIN_DIR/ganyu" 2>/dev/null || true
  echo "[install] 已创建别名: $BIN_DIR/ganyu -> ganyu-agent"
fi

# ---- PATH 提示 -----------------------------------------------------------------
case ":${PATH}:" in
  *":${BIN_DIR}:"*) ;;
  *) echo "[install] 请把以下目录加入 PATH 后重新打开终端："
     echo "          export PATH=\"$BIN_DIR:\$PATH\"" ;;
esac

echo ""
echo "[install] ✅ 安装完成。快速体验："
echo "          ganyu-agent selftest"
echo "          ganyu-agent doctor"
echo "          配置模型（交互式向导，推荐）：ganyu-agent setup"
echo "          直接对话：ganyu-agent chat   （或 ganyu）"
echo "          升级：ganyu-agent update"
echo "          查看/切换模型：ganyu-agent model"
echo "          接 Telegram：ganyu-agent gateway start"
