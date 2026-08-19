---
description: 运行测试（默认特性 + 全特性 + 指定模块）
---

# /test

```bash
cargo test                        # 默认特性（crypto+secret）
cargo test --features hardened    # 全特性（含 network/shell）
cargo test <模块名>               # 单模块，如 cargo test memory
```

注意：改 env 的测试（`GANYU_MEM_KEY`）依赖静态锁串行化，不要单独并行跑多个测试进程。
