# ADR-005: P0–P3 修复落地（失败闭环）

## Status
Accepted

## Context
按 ADR-003 审计结果全量修复，原则：**默认失败闭环**——默认不给能力，显式开启且最小权限。

## Decision
- **P0 Critical**：C1 `exec` 仅 `shell` 特性编译 + 运行时 `GANYU_ALLOW_SHELL=1` 双层；
  C2 插件默认关 + `vetted`/白名单/程序名校验；C3/C4 `resolve_sandboxed` 文件沙箱
  （拒绝对路径/穿越/符号链接逃逸，根默认 `.ganyu_workspace`）；C5 `ssrf_guard` 四重校验 + 禁重定向。
- **P1 High**：H1 `crypto` 特性记忆 AES-256-GCM（SHA-256 派生密钥 + 原子写）；
  H2 `Mdl::detect_injection`（堆叠/注释截断/危险 DDL）；H3 `sandbox` 特性 Landlock
  （目标特定依赖仅 Linux 拉取，`pre_exec` 施加于子进程）。
- **P2 Medium**：M1 OpenViking 真代理 OV_BASE + 本地降级；M2 `RateLimiter` 令牌桶；
  M3 `Tool::side_effecting` 副作用不盲目重试；M4 tokio 异步 + 并发写；
  M5 网关出口 `sanitize_model_output`；M6 `parse_tool_call` 支持 JSON 原生函数调用。
- **P3 Low**：L1 `secret` 特性 `zeroize::Zeroizing` API key；L2 网关 `hot_reload`。

## Consequences
- 易：默认攻击面归零；能力按需组合（`hardened` 一键生产加固）。
- 难：默认失去 exec/插件/直读系统文件（需显式开启）；部分能力需特性编译。

## 验证
六档构建（default/network/crypto,secret/shell/sandbox/hardened）全过；
测试默认 36 / crypto 24 / network 23 全绿；PoC 复跑确认 exec 未注册、file_read 沙箱外失败；
selftest 9/9。
