# ADR-006: 结构化 / 工程化 / 安全治理 / 缓存优化

## Status
Accepted

## Context
对标 Pi·OpenClaw·Hermes·Prime 2026 最新能力与防护，补齐工程化短板：
配置散落、无缓存、无审计、无结构化管理。

## Decision
- **D1 配置面** `src/config.rs`：`GANYU_*` 集中类型化读取 + `security_baseline` 启动自检 +
  `ENV_DOCS` 单一来源（完整清单见 [config-guide.md](config-guide.md)）。
- **D2 缓存面** `src/cache.rs`：`LruCache`（LRU+TTL，默认关）；
  只读工具结果缓存 + LLM 响应缓存；**副作用工具永不缓存**。
- **D3 观测面** `src/observe.rs`：JSON Lines 审计（工具调用/安全拒绝/网关级联/限速/缓存命中）。
- **D4 目录管理**：docs 按「入门/指南/架构/ADR/安全」组织；`.gitignore` 覆盖构建与运行时工件。

## Consequences
- 易：缓存/限速/审计一行 env 开启；启动即暴露高危组合；安全事件可追溯。
- 难：缓存引入 TTL 内陈旧语义（副作用不缓存兜底）；config.toml 文件化留作后续。

## 验证
默认/network/crypto,secret/shell/sandbox/hardened 六档构建全过；新增 9 测试
（LRU/TTL/touch、只读缓存命中、副作用不缓存、LLM 缓存吸收、配置默认失败闭环、TTL 解析）全绿。
