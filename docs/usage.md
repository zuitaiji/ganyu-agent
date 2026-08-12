# ganyu-agent 使用指南（CLI）

> 面向用户：安装（见 [install.md](install.md)）后，如何用 CLI 完成各类任务。
> 假设二进制为 `ganyu-agent`（或别名 `ganyu`），已加入 PATH。

## 全局约定

- 每个交互都会打印 `session: <uuid>`；用 `--session <uuid>` 可续接该会话（记忆中有轨迹则注入）。
- 工具调用两种语法：
  - **`@tool arg` 脚本**：`run`/`chat` 里逐行执行，每行一个工具（参数取同行剩余部分）；
  - **JSON 原生函数调用**（M6）：`{"tool":"x","args":"y"}` 或 OpenAI `function_call` 风格。
- 环境变量（`GANYU_*`）见 [SECURITY.md](../SECURITY.md) 与 `src/config.rs`。

## 子命令速查

| 子命令 | 作用 |
|--------|------|
| `selftest` | 内置自检（9 项：重试/SAG/记忆/工具/技能/网关） |
| `tools` | 列出全部工具与特性技能 |
| `modes` | 列出七大 agent 范式 |
| `run "<脚本>"` | 跑完整 ReAct 推理循环（多步工具调用），打印轨迹与作答 |
| `agent "任务" --mode <范式>` | 以指定范式编排执行 |
| `skill <名> <参数>` | 直接调用特性技能（如 `summarize path`） |
| `sag "问题"` | 跑知识/分析面 SAG 管道（默认 `examples/sample_mdl.json`） |
| `chat` | 从 stdin 读一行，经推理循环响应（默认子命令） |
| `--session <uuid>` | 续接指定会话（`run`/`chat`/`agent` 通用） |

## 示例

### 1. 自检与环境
```bash
ganyu-agent selftest          # 期望输出: selftest: 9 passed, 0 failed
ganyu-agent tools             # 列出 echo/calc/file_*/exec/remember/recall/rag_search/skill:*
ganyu-agent modes             # single/react/plan/multi/router/blackboard/graph
```

### 2. ReAct 推理循环（工具调用）
```bash
ganyu-agent run "@calc (1+2)*3"            # → 9
ganyu-agent run "@echo hello"              # 回显
ganyu-agent run $'@file_write a.txt\nhello world'   # 写入沙箱内 a.txt（首行路径，空行后内容）
ganyu-agent run "@file_read a.txt"         # 读回
ganyu-agent run $'@remember city\n杭州'     # 记忆写入（key=city）
ganyu-agent run "@recall city"             # 记忆读取
ganyu-agent run "@rag_search 杭州"          # 记忆检索
```
> 文件工具受沙箱约束（默认根 `.ganyu_workspace`）；`exec` 默认关闭（见安全章节）。

### 3. 多范式编排
```bash
ganyu-agent agent "总结报告并排查故障" --mode single
ganyu-agent agent "分三步完成：调研、草稿、复核" --mode plan
ganyu-agent agent "做一件事" --mode multi          # 规划者/执行者/复核者协作
ganyu-agent agent "帮我总结一下文档" --mode router  # 关键字路由到 Summarizer
ganyu-agent agent "写一份季度报告" --mode blackboard
ganyu-agent agent "主题X" --mode graph              # DAG 拓扑执行
```

### 4. 知识/分析（SAG）
```bash
ganyu-agent sag "上月华东区利润最高的三个产品"
# 输出: verdict: Pass / sql: SELECT ... / rows: [...]
```

### 5. 技能
```bash
ganyu-agent skill summarize path/to/file   # 读文件并离线摘要
ganyu-agent skill troubleshoot "报错信息"   # 检索记忆中的案例与沉淀
ganyu-agent skill kb_query "问题"           # 知识库检索 + 摘要
```

### 6. 会话续接（跨重启自进化）
```bash
ganyu-agent run "第一次对话"                # 记住 session: xxxx
ganyu-agent chat --session xxxx             # 续接：注入上次轨迹
```

### 7. 特性开关（示例）
```bash
# shell 工具（需 shell 特性编译 + 运行时放行）
GANYU_ALLOW_SHELL=1 ganyu-agent run "@exec echo hi"

# 记忆加密（crypto 特性 + 密钥）
GANYU_MEM_KEY='强口令' ganyu-agent run $'@remember secret\ndata'

# 审计日志 / 只读工具缓存 / 限速
GANYU_AUDIT=1 GANYU_TOOL_CACHE_TTL=30000 GANYU_RATE_PER_MIN=60 ganyu-agent run "@calc 2+3"

# 真实 LLM（network 特性）
OPENAI_API_BASE=https://api.openai.com/v1 OPENAI_API_KEY=sk-... \
  ganyu-agent run "用一句话介绍 ganyu"
```

## 常见问题

- **`exec` 提示 Forbidden**：exec 默认失败闭环，需 `shell` 特性编译 + `GANYU_ALLOW_SHELL=1`。
- **文件读不到**：文件工具只能访问沙箱根内相对路径（默认 `.ganyu_workspace`），
  如需放宽设 `GANYU_FS_ROOT`（生产建议保持默认或改用容器挂载）。
- **没有真模型输出是"本地兜底"**：默认构建无网络模型；接真实模型用 `--features network` + `OPENAI_API_*`。
- **插件 `upper` 未出现**：插件默认关闭；需 `GANYU_ALLOW_PLUGINS=1` 且清单带 `vetted:true`、
  程序名在 `GANYU_PLUGIN_ALLOW` 白名单。
