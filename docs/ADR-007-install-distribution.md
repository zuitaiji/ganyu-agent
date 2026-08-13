# ADR-007: 安装与分发策略

## Status
Accepted（v2：2026-08-13 更新——CI 发布管道落地，安装内核改为免编译下载）

## Context
让用户「一条命令安装」，同时避免供应链风险（对标 ClawHavoc 恶意技能事件与
Hermes「先审查再跑」的告诫）。v1 因无 CI 基础设施，选择 cargo 编译内核；
v2 落地 GitHub Actions 发布管道后，改为下载预编译二进制（Hermes 式即开即用）。

## Decision（v2）
- **安装内核 = GitHub Releases 预编译二进制下载**（默认免编译）：
  - CI（`.github/workflows/release.yml`）：tag 推送 → 三平台（win/linux/macos）`--features hardened`
    构建 → 资产**统一 tar.gz** → 发布 Release；
  - 安装脚本（install.sh / install.ps1）默认走：GitHub API 查最新 release →
    按平台选资产 → 下载 → `tar -xzf` 解压到 `<prefix>/bin` → selftest 自检 →
    `ganyu` 别名 → PATH 提示；幂等（重复执行覆盖升级，不动 config.toml 与记忆文件）；
  - `--features`（ps1 `-Features`）显式指定时**回退 cargo 编译**（本地源码优先，否则 clone），供定制特性。
- **安装位置**：独立目录（sh 默认 `~/.local`，ps1 默认 `~\.ganyu`），删目录即卸载，零污染。
- **自更新**：`ganyu update` 子命令从 GitHub Releases 下载当前平台最新资产覆盖 `~/.ganyu/bin`。
- **供应链安全**：脚本最小面（只做 API 查询/下载/解压）；release 资产来自仓库自身 CI；
  未来可追加 `.sha256` 校验。

## Consequences
- 易：一条命令安装/升级，免编译（秒级），PATH/自检/别名开箱即用；开发者仍可用
  `cargo install --git` 或 `--features` 定制编译。
- 难：发布依赖 CI + tag 流程（`git tag vX.Y.Z && git push --tags`）；未发布 tag 时
  `update` 会提示先打 tag。

## 验证
- install.sh `bash -n` + install.ps1 PowerShell Parser 语法检查通过；
- CI 三平台构建全绿，Release v0.1.0/v0.1.1 资产（2.8–3.3MB）发布成功；
- 端到端实测：下载 → 解压 → `doctor` ✅（network/crypto/secret/shell 全开）；
  `setup` 写入 config.toml ✅；`model` 切换 ✅。
- 踩坑记录：Windows Compress-Archive 正斜杠路径 PathNotFound → 资产统一 tar.gz；
  Git Bash GNU tar 将 `D:\` 误判为远程主机 → Archive 步骤用 cygpath 转换。
