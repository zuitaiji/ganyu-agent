---
description: 走完整发布闭环（CI → tag → release → review）
---

# /release

严格遵循 `.workbuddy/rules/release.md`：

```bash
git commit -m "..." && git push origin main
gh workflow run release.yml
gh run watch <run-id> --exit-status      # 三平台全绿
git tag vX.Y.Z && git push origin vX.Y.Z
# 核对 release 资产 ≥6（3 平台 tar.gz + .sha256）
```

发布后：穷尽式 review（F-01~F-14）、README/docs 同步、汇报（做了什么/验证/剩余风险）。
