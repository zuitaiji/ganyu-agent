# Coding 规则（ganyu-agent）

> 适用：所有 src/ 改动。目标：可验证、可追溯、不引入臆测抽象。

## Rust 风格基线

- **edition 2021**；遵循 `cargo fmt` 输出，禁止手工对抗格式
- 优先函数式/声明式；避免无必要 `class` 式结构；模块小而清晰
- 错误处理走 `thiserror`（项目已依赖），不用裸 `unwrap`/`expect`（测试除外）
- 异步统一 `tokio`；共享状态用 `Arc<Mutex<_>>`，与项目现有模式一致
- 注释解释**为什么**，不解释做了什么；公开函数/复杂逻辑加 doc comment

## 特性门控（项目铁律）

- `default = ["crypto", "secret"]`：**不得移除**（防"忘开加固就裸奔"，F-09）
- `network`/`shell`/`sandbox`：危险能力，仅显式 `--features` 开启
- 新增依赖：优先纯 Rust 轻量 crate；引入 TLS/C 编译依赖必须说明理由
- 条件编译：平台特定代码（如 landlock）必须 `cfg(target_os = ...)` 包裹，防破坏跨平台构建

## 改动纪律（karpathy-guidelines）

1. 明确假设与验收标准再动手
2. 精准修改：每行改动可追溯到用户目标
3. 避免臆测抽象：不为一次性代码做抽象、不做未被要求的可配置性
4. 匹配项目风格：跟随现有代码写法，即使个人偏好不同
5. 发现无关死代码 → 只汇报，不删除

## 测试要求

- 修 bug：先写失败测试复现 → 修复 → 测试转绿
- 新逻辑：`src/**` 内 `#[cfg(test)]` 模块补覆盖（项目惯例）
- **改 env 的测试必须串行**：全局 env（如 `GANYU_MEM_KEY`）并行会竞态，用静态锁串行化
- 平台敏感测试（crypto/shell/sandbox）注意 `#[cfg(feature=...)]` 门控

## 验证闭环

```
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```
三关全绿才可提交。CI 会跑全量（Linux/macOS/Windows 三平台），本地至少保证当前平台全绿。
