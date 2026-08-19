# AGENTS.md — ganyu-agent 开发入口

> 本文件是目录页，不是百科全书。渐进式披露：先读本文件 → 按需深入 `.workbuddy/rules/` 与 `docs/`。
> 项目：Rust AI agent 系统（会话 UUID + 统一字符串值 + 抽象层）。MIT。

## 启动工作流

1. `pwd` 确认在仓库根
2. 完整阅读本文件
3. 读 `.workbuddy/settings.json` 了解项目设置
4. 按需读规则：`.workbuddy/rules/{coding,security,git,release}.md`
5. 架构总览：`docs/architecture.md`；历史决策：`docs/ADR-*.md`
6. 最近提交：`git log --oneline -5`

## 构建与验证命令

| 动作 | 命令 |
|---|---|
| 全量测试 | `cargo test`（默认特性 crypto+secret） |
| 全特性编译 | `cargo build --features hardened` |
| 类型/lint | `cargo clippy --all-targets -- -D warnings` |
| 格式 | `cargo fmt --check` |
| 环境诊断 | `cargo run -- doctor` |
| 自检 | `cargo run -- selftest` |

**任何改动必须通过：fmt + clippy + test 三关才可提交。**

## 工作规则

- **一次一个功能**：不在一次提交里混多个无关改动
- **必须验证**：未跑上述验证命令前，不能声明完成
- **精准改动**：只碰任务相关文件；不重构没坏的东西；发现无关死代码只汇报不删除
- **测试先行**：修 bug 先写失败测试复现；新增逻辑补测试（`src/**` 内 `#[cfg(test)]`）
- **特性门控**：危险能力（network/shell/sandbox）需显式 `--features` 开启；默认特性 `crypto,secret` 不得降级
- **不提交**：运行时数据（`.ganyu_*.json`、`.ganyu_workspace/`）、密钥、测试临时文件
- **收拢约定**：agent 开发配置只放 `.workbuddy/`；`skills/`（顶层）是 ganyu 运行时技能库，**不是开发辅助技能，勿混用**
- **安全底线**：详见 `.workbuddy/rules/security.md`；绝不在代码/日志/提交中带出密钥

## 完成定义

- [ ] 目标行为已实现，且无无关改动
- [ ] `cargo fmt --check`、`cargo clippy -- -D warnings`、`cargo test` 全部通过
- [ ] 新增/修改逻辑有测试覆盖
- [ ] 提交信息遵循 `.workbuddy/rules/git.md` 规范

## 升级处理

- 架构不明确 → 读 `docs/architecture.md` 或 ADR，仍不明则问
- 测试持续失败 → 记录进度与现象，标记需人工审查，不盲目绕过
- 涉及发布 → 遵循 `.workbuddy/rules/release.md`（CI 全绿 → tag → release → 穷尽 review）
- 涉及安全面（密钥/沙箱/审计/执行面）→ 强制先读 `.workbuddy/rules/security.md`
