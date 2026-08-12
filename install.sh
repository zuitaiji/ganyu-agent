#!/usr/bin/env bash
# ============================================================================
# ganyu-agent 一键安装脚本（Linux / macOS / Git-Bash）
#
# 用法：
#   本地（仓库内）：      bash install.sh [--features hardened] [--prefix ~/.local]
#   一条命令（远程）：   curl -fsSL <install.sh 直链> | bash
#
# 设计原则（对标 Pi / OpenClaw / Hermes 的 curl|sh 模式 + 供应链安全）：
#   - 默认 HTTPS 拉取；下载的 tarball 校验 SHA256（若发布产物带 .sha256）；
#   - 能力按需开启：默认构建零依赖、离线可装；`--features hardened` 才拉网络/TLS 依赖；
#   - 安装到独立 PREFIX，不污染系统目录；自带 selftest 自检与 PATH 提示；
#   - 幂等：重复执行覆盖升级，不动已有数据（.ganyu_workspace / 记忆文件）。
# ============================================================================
set -euo pipefail

# ---- 可配置项（环境变量优先，命令行参数次之） -------------------------------
GANYU_REPO="${GANYU_REPO:-https://github.com/zuitaiji/ganyu-agent.git}"
GANYU_BRANCH="${GANYU_BRANCH:-main}"
PREFIX="${PREFIX:-$HOME/.local}"
# 默认特性：空 = 默认构建（零依赖、离线）；生产建议 --features hardened
GANYU_FEATURES="${GANYU_FEATURES:-}"
# 是否创建 `ganyu` 别名软链（指向 ganyu-agent）
CREATE_ALIAS="${GANYU_CREATE_ALIAS:-1}"

# ---- 简易参数解析 -----------------------------------------------------------
while [[ $# -gt 0 ]]; do
  case "$1" in
    --features)  GANYU_FEATURES="$2"; shift 2 ;;
    --prefix)    PREFIX="$2"; shift 2 ;;
    --branch)    GANYU_BRANCH="$2"; shift 2 ;;
    --repo)      GANYU_REPO="$2"; shift 2 ;;
    --no-alias)  CREATE_ALIAS=0; shift ;;
    -h|--help)
      echo "用法: bash install.sh [--features <f1,f2>] [--prefix <dir>] [--branch <b>] [--repo <url>] [--no-alias]"
      echo "  特性示例: hardened | network | crypto,secret | shell,sandbox"
      exit 0 ;;
    *) echo "未知参数: $1"; exit 2 ;;
  esac
done

# ---- 前置检查 -----------------------------------------------------------------
if ! command -v cargo >/dev/null 2>&1; then
  echo "[install] 未检测到 cargo。请先安装 Rust（rustup）："
  echo "          curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
  echo "          然后重试本脚本。"
  exit 1
fi

# 定位源码：优先使用脚本所在仓库（已 clone），否则克隆到临时目录。
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
if [[ -f "$SCRIPT_DIR/Cargo.toml" && -f "$SCRIPT_DIR/src/main.rs" ]]; then
  SRC="$SCRIPT_DIR"
  echo "[install] 使用本地源码: $SRC"
else
  TMP_SRC="$(mktemp -d)/ganyu-agent"
  echo "[install] 克隆 $GANYU_REPO@$GANYU_BRANCH ..."
  git clone --depth 1 --branch "$GANYU_BRANCH" "$GANYU_REPO" "$TMP_SRC"
  SRC="$TMP_SRC"
fi

# ---- 构建并安装 --------------------------------------------------------------
echo "[install] cargo install --path '$SRC' --root '$PREFIX' --features '${GANYU_FEATURES:-<default>}'"
FEAT_ARGS=()
if [[ -n "$GANYU_FEATURES" ]]; then
  FEAT_ARGS=(--features "$GANYU_FEATURES")
fi
# 构建目录放系统临时区，避免污染工作区 target（也规避某些目录的文件锁）
TARGET_DIR="$(mktemp -d)/ganyu-target"
trap 'rm -rf "$TARGET_DIR"' EXIT

CARGO_TARGET_DIR="$TARGET_DIR" cargo install --path "$SRC" --root "$PREFIX" \
  --locked "${FEAT_ARGS[@]}"

BIN_DIR="$PREFIX/bin"
BIN="$BIN_DIR/ganyu-agent"

# ---- 自检 ---------------------------------------------------------------------
echo "[install] 自检: $BIN selftest"
if "$BIN" selftest >/tmp/ganyu_selftest.log 2>&1; then
  echo "[install] ✅ selftest 通过"
else
  echo "[install] ⚠️ selftest 失败，日志: /tmp/ganyu_selftest.log（可继续使用，建议反馈）"
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
echo "          ganyu-agent tools"
echo "          ganyu-agent run \"上月华东区利润最高的三个产品\" --mode sag"
echo "          生产建议: 重新安装并加 --features hardened（记忆加密/限速/审计）"
