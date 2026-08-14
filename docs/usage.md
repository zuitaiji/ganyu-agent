# ganyu-agent 使用指南（CLI）

> 面向用户。安装见 [install.md](install.md)，配置见 [config-guide.md](config-guide.md)。

## 即开即用（三种入口）

| 入口 | 操作 |
|------|------|
| **双击启动** | Windows：双击 `C:\Users\Administrator\.ganyu\start-ganyu.bat`，直接进入对话 |
| **命令对话** | 新开终端：`ganyu` 或 `ganyu-agent chat`（无参数默认即 chat） |
| **单次调用** | `ganyu-agent run "你好"` |

首次使用只需两步：装好后 `ganyu setup` 配一次模型（见下），之后以上任一入口即开即用；
启动时会显示已连接模型名；配错或缺配置时 `ganyu-agent doctor` 直接指出问题。

## 配置模型（交互式向导，推荐）

```bash
ganyu setup          # 依次问 base_url / api_key / model，回车沿用当前值
```

也支持参数模式（脚本/CI 用）：

```bash
ganyu setup --base_url https://api.openai.com/v1 --api_key sk-... --model gpt-4o
```

等价于手写配置文件（已设置的 `OPENAI_API_BASE/KEY/MODEL` 环境变量仍优先于文件）：

```toml
# ~/.ganyu/config.toml
[model]
base_url = "https://apihub.agnes-ai.com/v1"   # OpenAI 兼容端点
api_key = "sk-..."                             # 你的 key
model = "agnes-2.5-flash"                      # 模型 id
```

## 约定

- 每次交互打印 `session: <uuid>`；`--session <uuid>` 续接（Prime 式会话树）。
- 工具调用：`@tool arg` 脚本（每行一个，参数=同行剩余）或 JSON `{"tool":"x","args":"y"}`。

## 子命令

| 命令 | 作用 |
|------|------|
| `chat` | **交互式 REPL**（终端内多轮对话，上下文延续；管道输入=单次） |
| `setup` | **交互式配置模型**（base_url/api_key/model → `~/.ganyu/config.toml`） |
| `model` | **查看当前模型**；`model <新id>` 切换（写配置） |
| `models` | **查询网关可用模型列表**（`GET /v1/models`，基于已配置端点） |
| `update` | **从 GitHub Releases 自更新**到最新预编译二进制（覆盖 `~/.ganyu/bin`） |
| `gateway setup <token>` / `gateway start` | **Telegram 消息平台网关**：存 token / 长轮询收发消息 |
| `run "<脚本>"` | ReAct 多步推理，打印轨迹 |
| `agent "任务" --mode <范式>` | 以指定范式编排（single/react/plan/multi/router/blackboard/graph） |
| `sag "问题"` | 知识分析（默认 `examples/sample_mdl.json`） |
| `skill <名> <参数>` | 直接调用技能（summarize/troubleshoot/kb_query） |
| `selftest` / `tools` / `modes` | 自检 / 列工具 / 列范式 |
| `doctor` | 环境诊断：编译特性 / 配置文件 / 模型配置 / 网关后端 / 能力面 |

## 示例

```bash
ganyu setup                                       # 交互式配置模型
ganyu model gpt-4o                                 # 切换模型
ganyu update                                       # 升级到最新 release
ganyu gateway setup 123456:ABC... && ganyu gateway start   # 接 Telegram
ganyu chat                                         # 交互对话（接真模型）
ganyu run "@calc (1+2)*3"                          # → 9（离线可用）
ganyu run $'@file_write a.txt\nhello'              # 沙箱内写
ganyu run $'@remember city\n杭州' && ganyu run "@recall city"
ganyu agent "总结报告" --mode multi                # 多范式（接真模型语义完整）
ganyu sag "上月华东区利润最高的三个产品"
GANYU_AUDIT=1 ganyu run "@calc 2+3"                # 审计 JSON 到 stderr
GANYU_ALLOW_SHELL=1 ganyu run "@exec echo hi"      # exec（需 shell 特性）
```

## 常见问题

| 现象 | 说明 |
|------|------|
| 输出"本地兜底" | 未接模型：`ganyu setup` 配置（或 `OPENAI_API_BASE/KEY/MODEL`）且以 `network` 特性构建 |
| 请求 400 / 模型名错误 | 精确设置 `OPENAI_MODEL`（如 `agnes-2.5-flash`）；网关拒绝不认识的模型 id |
| 网络不通 | 走本地代理：设置 `HTTP_PROXY`/`HTTPS_PROXY` 环境变量（reqwest system-proxy 自动读取） |
| `exec` Forbidden | 默认失败闭环：需 `shell` 特性 + `GANYU_ALLOW_SHELL=1` |
| 文件读不到 | 文件工具仅限沙箱根（默认 `.ganyu_workspace`）内相对路径 |
| 插件未加载 | 需 `GANYU_ALLOW_PLUGINS=1` + `vetted:true` + 程序白名单 |
| `update` 找不到资产 | 该版本未发布 release（先 `git tag vX.Y.Z && git push --tags`），或稍后重试 |
| `update` 报 sha256 校验失败 | 资产被篡改或下载损坏，立即停止并重新 update（校验文件缺失时仅警告不阻断） |
| `gateway` 未配置 | 先 `ganyu gateway setup <bot_token>`；token 在 [BotFather](https://t.me/BotFather) 创建 |
