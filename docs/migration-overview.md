# ganyu-agent：Python → Rust 迁移（工具层）进度

## 已完成
- **审查结论**：项目中 Python 层共 30 个 `.py`（约 6952 行），均为工具链 / AI 技能辅助脚本，不进入运行时核心；安全扫描全绿（无 shell=True、无 eval/exec、无硬编码密钥、SSRF 防护在 Rust 核心）。
- **方案确认**：用户选择方案 A —— 全量 Rust 化，并入主 crate `src/tools/`。媒体编码走 Rust 封装 ffmpeg 子进程。
- **已落地文件**：
  - `Cargo.toml`：新增 `ed25519-dalek` 依赖与 `sign` feature（与 network 共享 ring）。
  - `src/release_sign.rs`：等价 `scripts/sign-release.py` + `scripts/seed_selfcheck.py`。子命令 gen/pub/sign/verify/seed-check；RFC 8032 Ed25519；内置测试向量断言；`#[cfg(not(feature="sign"))]` 返回 Forbidden。
  - `src/tools/mod.rs`：`run_tool` 分发 upper/diagram/git-diff/pr-diff（async）。
  - `src/tools/upper.rs`：等价 `plugins/upper.py`（stdin → 大写）。
  - `src/tools/diagram.rs`：纯 Rust 生成 SVG，等价 `docs/ai-arch/diagrams/gen_diagrams.py`（role_interaction.svg / upload_repo_init_lane.svg）。已清除此前编码损坏字符（𓀀 / ̄）。
  - `src/tools/git_diff.rs`：git-diff 本地（列表式 subprocess，无 shell）等价 get_diff.py；pr-diff 远程（GitHub/GitLab，token 取参数或环境变量，仅 network 特性）。
  - `src/lib.rs`：新增 `pub mod release_sign; pub mod tools;`。
  ️ - `src/main.rs`：在 `match cmd` 接入 "`release`" 与 "`tool`" 子命令。
- **待办 / 未解决**：
  - **编译验证被沙箱阻断**：本沙箱在 cargo 编译阶段会回收进程（exit 1、无 stderr），导致 `cargo build` 无法在本回合稳定产出可执行结果。已改用后台任务重试（task ukGf7y）。
  - 其余大模块迁移（video-clip、api-test-automation、code-analyzer/ddd）尚未开始。
  - Python 文件过渡期保留，待用户确认后删除。

## 下一步
- 等后台编译结果；若通过，继续迁移剩余大模块并更新 CI/README。
- 若编译报错，定位并修复（优先基于日志）。
