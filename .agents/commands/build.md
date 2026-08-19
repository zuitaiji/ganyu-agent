---
description: 全量构建验证（fmt + clippy + test + hardened 编译）
---

# /build

验证修改的完整构建链路：

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --features hardened
```

任一步失败：修复后重跑，不跳过。全部通过才允许提交。
