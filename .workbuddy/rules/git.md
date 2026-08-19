# Git 规则（ganyu-agent）

## 提交信息规范

```
<type>(<scope>): <一句话描述>

- 根因：...
- 修复：...
- 测试：...
```

- type：`feat` / `fix` / `docs` / `chore` / `refactor` / `test`
- scope：模块名（memory / routing / security / install / ci / test …）
- 必须说明"为什么"（根因/影响），不只写"做了什么"

## 提交纪律

- **一次提交一个逻辑变更**；代码与文档同步提交
- 运行时数据（`.ganyu_*.json`、`.ganyu_workspace/`、`*.tmp`）已被 .gitignore 排除，**勿手动 add**
- 不提交：密钥、测试临时文件、`target/` 产物
- 禁止破坏性命令：`git reset --hard`、`git clean -f`、`git push --force`（除非用户明确要求）

## 分支与发布

- 日常开发直接 `main`（单人项目惯例）；实验性改动可临时分支
- 发布流程见 `.workbuddy/rules/release.md`：
  ```
  commit + push → gh workflow run release.yml → CI 三平台全绿 → git tag vX.Y.Z → push tag → release 生成 → 穷尽 review
  ```

## 提交前自检

- [ ] `git status` 只含本次变更文件
- [ ] `git diff` 无密钥/调试残留
- [ ] 无无关文件混入（不 add 无关改动）
