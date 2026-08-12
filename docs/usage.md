# ganyu-agent 使用指南（CLI）

> 面向用户。安装见 [install.md](install.md)，配置见 [config-guide.md](config-guide.md)。

## 即开即用（三种入口）

| 入口 | 操作 |
|------|------|
| **双击启动** | Windows：双击 `C:\Users\Administrator\.ganyu\start-ganyu.bat`，直接进入对话 |
| **命令对话** | 新开终端：`ganyu-agent chat`（或 `ganyu-agent`，默认即 chat） |
| **单次调用** | `ganyu-agent run "你好"` |

首次使用只需两步：装好后写一次 `~/.ganyu/config.toml`（见下），之后以上任一入口即开即用；
启动时会显示已连接模型名；配错或缺配置时 `ganyu-agent doctor` 直接指出问题。

## 一站式配置（写一次，永久生效）

```toml
# ~/.ganyu/config.toml
[model]
base_url = "https://apihub.agnes-ai.com/v1"   # OpenAI 兼容端点
api_key = "sk-..."                             # 你的 key
model = "agnes-2.5-flash"                      # 模型 id
```
> 已设置的 `OPENAI_API_BASE/KEY/MODEL` 环境变量优先于配置文件（CI/容器友好）。

## 约定

- 每次交互打印 `session: <uuid>`；`--session <uuid>` 续接（Prime 式会话树）。
- 工具调用：`@tool arg` 脚本（每行一个，参数=同行剩余）或 JSON `{"tool":"x","args":"y"}`。

## 子命令

| 命令 | 作用 |
|------|------|
| `chat` | **交互式 REPL**（终端内多轮对话，上下文延续；管道输入=单次） |
| `run "<脚本>"` | ReAct 多步推理，打印轨迹 |
| `agent "任务" --mode <范式>` | 以指定范式编排（single/react/plan/multi/router/blackboard/graph） |
| `sag "问题"` | 知识分析（默认 `examples/sample_mdl.json`） |
| `skill <名> <参数>` | 直接调用技能（summarize/troubleshoot/kb_query） |
| `selftest` / `tools` / `modes` | 自检 / 列工具 / 列范式 |
| `doctor` | 环境诊断：编译特性 / 配置文件 / 模型配置 / 网关后端 / 能力面 |

## 示例

```bash
ganyu-agent chat                                   # 交互对话（接真模型）
ganyu-agent run "@calc (1+2)*3"                    # → 9（离线可用）
ganyu-agent run $'@file_write a.txt\nhello'        # 沙箱内写
ganyu-agent run $'@remember city\n杭州' && ganyu-agent run "@recall city"
ganyu-agent agent "总结报告" --mode multi          # 多范式（接真模型语义完整）
ganyu-agent sag "上月华东区利润最高的三个产品"
GANYU_AUDIT=1 ganyu-agent run "@calc 2+3"          # 审计 JSON 到 stderr
GANYU_ALLOW_SHELL=1 ganyu-agent run "@exec echo hi" # exec（需 shell 特性）
```

## 常见问题

| 现象 | 说明 |
|------|------|
| 输出"本地兜底" | 未接模型：配置 `~/.ganyu/config.toml`（或 `OPENAI_API_BASE/KEY/MODEL`）且以 `network` 特性构建 |
| 请求 400 / 模型名错误 | 精确设置 `OPENAI_MODEL`（如 `agnes-2.5-flash`）；网关拒绝不认识的模型 id |
| 网络不通 | 走本地代理：设置 `HTTP_PROXY`/`HTTPS_PROXY` 环境变量（reqwest system-proxy 自动读取） |
| `exec` Forbidden | 默认失败闭环：需 `shell` 特性 + `GANYU_ALLOW_SHELL=1` |
| 文件读不到 | 文件工具仅限沙箱根（默认 `.ganyu_workspace`）内相对路径 |
| 插件未加载 | 需 `GANYU_ALLOW_PLUGINS=1` + `vetted:true` + 程序白名单 |
