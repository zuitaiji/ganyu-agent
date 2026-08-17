# ganyu-agent 构建缓存优化方案

> 状态：**执行中**（2026-08-17 已确认 A+B+C，B2 变体）· 决策人：老大

## 执行记录

| 项 | 内容 | 状态 |
|----|------|------|
| A. 统一缓存目录 | install.sh/install.ps1 → `$HOME/.ganyu/.build-cache`（GANYU_CARGO_TARGET_DIR 可覆盖）+ 锁自愈 | ✅ 已改（BOM/语法校验过） |
| B. Cargo 优化 | `.cargo/config.toml`（incremental+jobs=8）+ `[profile.release]` B2（thin LTO / cgu=4 / strip / panic=abort） | ✅ 已加（全量构建验证中） |
| C. sccache | `sccache 0.8.2` 已装（~/.cargo/bin）+ `scripts/sccache-setup.sh` | ✅ 已装（命中验证随构建） |

## 1. 现状与问题

| # | 问题 | 现状 | 影响 |
|---|------|------|------|
| P1 | install.sh 构建无缓存 | `mktemp` 临时目录，装完即弃 | 每次升级**全量重编**（release 18–28min） |
| P2 | install.ps1 缓存目录易被锁 | `$Prefix\target`（~/.ganyu/target）曾被写拦截代理锁死（`.cargo-build-lock` 删不掉） | 安装/构建中断 |
| P3 | 无统一构建入口 | 手动构建依赖临时目录（%TEMP%），语义不明确 | 混乱、不可复现 |
| P4 | release 无 profile 优化 | Cargo 默认（无 strip、无 LTO、codegen-units 16） | 二进制体积大（~7MB→可 -30%） |
| P5 | CI 缓存已有 | `.github/workflows/release.yml` 用 `Swatinem/rust-cache@v2` | ✅ 无需改动 |

## 2. 方案

### A. 统一本地构建缓存（解决 P1/P2/P3）——建议执行
- **统一缓存目录**：`$HOME/.ganyu/.build-cache`（避开旧 target 锁区）
  - `install.sh`：`CARGO_TARGET_DIR` 由 `mktemp` 改为该目录（幂等，升级增量编译）
  - `install.ps1`：`GANYU_CARGO_TARGET_DIR` 默认由 `$Prefix\target` 改为该目录
  - 均支持 `GANYU_CARGO_TARGET_DIR` 环境变量覆盖
- **锁自愈**：两个脚本构建前检测 `.cargo-build-lock` / `.cargo-lock` 残留并清除
- 效果：全量编译只需一次，之后升级/多特性构建增量（秒~分钟级）

### B. Cargo 配置与 profile 优化（解决 P4）——建议执行
- 新增 `.cargo/config.toml`：
  ```toml
  [build]
  incremental = true   # 增量编译
  jobs = 8             # 并行度（可按机器核数调整）
  ```
- `Cargo.toml` 新增 `[profile.release]`（两个变体二选一）：
  - **B2（推荐：小体积）**：`lto = "thin"` · `codegen-units = 4` · `strip = true` · `panic = "abort"`
    - 体积约 -30~40%；thin LTO 增量代价小；`panic=abort` 已验证（源码无 `catch_unwind`）
  - B1（快构建）：保持默认 opt，仅加 `strip = true`

### C. sccache 分布式/本地编译缓存（解决多特性去重）——可选
- `RUSTC_WRAPPER=sccache` + 本地 server；default/network/hardened 等特性共享 deps 编译产物
- 成本：新增系统依赖（sccache）；首次无收益，多次构建后收益明显
- 适合：多特性频繁切换构建的开发者

## 3. 验证记录（待构建完成补录）

| 步骤 | 内容 | 验证 |
|------|------|------|
| 1 | 修改 install.sh / install.ps1（统一缓存目录 + 锁自愈） | `bash -n` / 脚本 dry-run |
| 2 | 新增 `.cargo/config.toml` + `Cargo.toml` profile（B1 或 B2） | 增量构建耗时对比 |
| 3 | 全量构建 + selftest + doctor + 真实对话回归 | 功能无退化 |
| 4 | （可选 C）sccache 接入与文档 | 二次构建命中率 |
| 5 | 更新 docs（install/config-guide 缓存说明）+ 提交推送 | 仓库同步 |

1. 执行范围：**A+B**（推荐）？还是 A 先行？
2. profile 变体：**B2 小体积**（推荐）还是 B1 快构建？
3. 是否启用 C（sccache）？
4. 缓存目录 `~/.ganyu/.build-cache` 是否接受（或指定其它路径）？
