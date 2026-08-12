# ganyu-agent 使用指南（CLI）

> 面向用户。安装见 [install.md](install.md)，配置见 [config-guide.md](config-guide.md)。

## 约定

- 每次交互打印 `session: <uuid>`；`--session <uuid>` 续接（Prime 式会话树）。
- 工具调用：`@tool arg` 脚本（每行一个，参数=同行剩余）或 JSON `{"tool":"x","args":"y"}`。

## 子命令

| 命令 | 作用 |
|------|------|
| `selftest` | 自检（9 项） |
| `tools` / `modes` | 列出工具技能 / 七大范式 |
| `run "<脚本>"` | ReAct 多步推理，打印轨迹 |
| `agent "任务" --mode <范式>` | 以指定范式编排（single/react/plan/multi/router/blackboard/graph） |
| `skill <名> <参数>` | 直接调用技能（summarize/troubleshoot/kb_query） |
| `sag "问题"` | 知识分析（默认 `examples/sample_mdl.json`） |
| `chat` | stdin 对话（默认） |

## 示例

```bash
ganyu-agent selftest
ganyu-agent run "@calc (1+2)*3"                          # → 9
ganyu-agent run $'@file_write a.txt\nhello'              # 沙箱内写（首行路径+内容）
ganyu-agent run $'@remember city\n杭州' && ganyu-agent run "@recall city"
ganyu-agent agent "总结报告" --mode multi
ganyu-agent sag "上月华东区利润最高的三个产品"
GANYU_AUDIT=1 ganyu-agent run "@calc 2+3"                # 审计 JSON 到 stderr
GANYU_ALLOW_SHELL=1 ganyu-agent run "@exec echo hi"      # exec（需 shell 特性）
```

## 常见问题

| 现象 | 说明 |
|------|------|
| `exec` Forbidden | 默认失败闭环：需 `shell` 特性 + `GANYU_ALLOW_SHELL=1` |
| 文件读不到 | 文件工具仅限沙箱根（默认 `.ganyu_workspace`）内相对路径 |
| 输出"本地兜底" | 无真模型；`--features network` + `OPENAI_API_*` 接入 |
| 插件未加载 | 需 `GANYU_ALLOW_PLUGINS=1` + `vetted:true` + 程序白名单 |
