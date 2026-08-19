# .agents/hooks/ — 开发钩子

> 当前仓库未启用 git hooks（单人项目 + CI 兜底）。本目录为范本，启用时按需挂接。

## 建议启用

### pre-commit（质量门控）

```bash
#!/bin/bash
# .git/hooks/pre-commit 或通过 husky/pre-commit 框架
set -e
cargo fmt --check        || { echo "fmt 失败"; exit 1; }
cargo clippy --all-targets -- -D warnings || { echo "clippy 失败"; exit 1; }
# 快速 smoke：避免提交密钥/运行时数据
if git diff --cached --name-only | grep -qE '\.ganyu_.*\.json$|\.ganyu_workspace/|\.env$'; then
  echo "⚠️ 检测到运行时数据/密钥文件被暂存，已阻止"; exit 1
fi
```

### pre-push（全量验证，可选）

```bash
#!/bin/bash
set -e
cargo test
```

## 约定

- 钩子脚本只做门控（拒绝坏结果），不自动改写代码
- 钩子逻辑与 `.agents/rules/` 保持一致，避免双规范漂移
- CI（release.yml）是最终权威：本地钩子失败 ≠ 跳过，CI 仍会全量验证
