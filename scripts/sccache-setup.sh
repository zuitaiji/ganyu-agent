#!/usr/bin/env bash
# sccache 接入脚本（构建缓存方案 C）：本地编译缓存，跨 target 目录/特性共享产物。
# 用法：
#   bash scripts/sccache-setup.sh          # 检测 + 启用（当前 shell）
#   source scripts/sccache-setup.sh        # 导出让当前 shell 生效
set -e

if command -v sccache >/dev/null 2>&1; then
  echo "[sccache] 已安装: $(sccache --version | head -1)"
else
  echo "[sccache] 未安装。请安装后重试："
  echo "          Windows: 下载 https://github.com/mozilla/sccache/releases 的 x86_64-pc-windows-msvc 包，解压加入 PATH"
  echo "          macOS:   brew install sccache"
  echo "          Linux:   cargo install sccache 或 发行版包"
  return 1 2>/dev/null || exit 1
fi

export RUSTC_WRAPPER="$(command -v sccache)"
echo "[sccache] 已启用: RUSTC_WRAPPER=$RUSTC_WRAPPER"
echo "          后续 cargo 构建将自动去重（多特性/多 target 目录共享）。"
echo "          查看命中: sccache --show-stats"
