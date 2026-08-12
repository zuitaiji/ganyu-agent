# ADR-007: 安装与分发策略

## Status
Accepted

## Context
让用户「一条命令安装」，同时避免供应链风险（对标 ClawHavoc 恶意技能事件与
Hermes「先审查再跑」的告诫）。

## Decision
- **安装内核 = `cargo install --locked --root <prefix>`**：复用 Rust 生态信任链与锁文件可复现性；
  不引入未签名二进制分发（无 CI/签名基础设施时二进制反而制造信任问题）。
- **一键脚本引导**（install.sh / install.ps1）：检测 cargo → 定位/克隆源码 →
  `CARGO_TARGET_DIR` 持久缓存构建（幂等升级）→ selftest 自检 → `ganyu` 别名 → PATH 提示。
- **特性默认空**（离线零依赖快装），`hardened` 为生产推荐显式选择。
- **供应链安全**：脚本最小面（只做检测/clone/cargo install）；远程要求 HTTPS 直链；
  未来发布二进制须附带 `.sha256` 并在脚本中校验。

## Consequences
- 易：一条命令安装/升级，PATH/自检/别名开箱即用；开发者仍可用 `cargo install --git`。
- 难：首次安装需编译（默认 ~1-3 分钟，hardened 更久）；无版本化发布管道（后续 CI 补）。

## 验证
install.sh `bash -n` + mock 全流程 dry-run 通过；install.ps1 真实执行
`-Features hardened -Dev` 成功（1m03s 零警告，selftest 通过，别名/PATH 就绪）；
hardened 二进制能力检验 12 项全过。
