---
description: 环境诊断（本地二进制 + 配置 + 特性面）
---

# /doctor

```bash
cargo run -- doctor    # 源码诊断
ganyu doctor           # 已安装二进制诊断（~/.ganyu/bin）
```

检查项：配置完整性（~/.ganyu/config.toml）、特性门控、网关端点、能力面。
若配置缺失：`ganyu setup` 交互式配置（API key 掩码输入，不回显）。
