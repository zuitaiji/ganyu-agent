# Release 规则（ganyu-agent）

> 发布 = 可复现 + 可验证 + 可回滚。每次发布必须走完整闭环。

## 流程

```
1. 功能/修复完成，CI 全绿（cargo test 三平台）
2. git commit + push origin main
3. gh workflow run release.yml（手动触发主分支构建）
4. gh run watch <run-id> --exit-status   ← 三平台必须全绿
5. git tag vX.Y.Z && git push origin vX.Y.Z  ← tag 触发 release 流水线
6. 等待 release 生成，核对资产（3 平台 tar.gz + .sha256，≥6 资产）
7. 穷尽式 review：README/SECURITY/CHANGELOG 同步；安全面 F-01~F-14 复查
8. 汇报：做了什么 / 验证证据 / 剩余风险
```

## 版本规则

- 语义化版本：`fix` → patch（v0.1.x），`feat` → minor（v0.x.0）
- tag 必须与 Cargo.toml 版本对齐（本项目 tag 号与 crate version 独立管理，以 tag 为准）
- 旧 release 缺 `.sha256` 时 update 降级警告——**新发布必须带 sha256**

## 发布后强制 review（F 系列清单）

- **F-01~F-14**：安全面全项复查（密钥/沙箱/审计/供应链/执行面）
- 文档同步：README（命令/特性矩阵）、docs/（ADR/config-guide/install）
- 测试统计：记录三平台测试数，新增测试必须在发布说明中列出
- 残留风险：P2 必须修复后发布；P3 记录在案并说明

## 回滚

- 版本回退：重新 tag 上一版本并触发 CI（不删旧 release）
- 二进制损坏/密钥问题：立即 unpublish 或标记 draft，通知用户走 `ganyu update` 修复
