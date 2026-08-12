# ADR-004：取长补短 —— 设计基色 × 2026 开源边界 × ganyu-agent 现状

> 配套 `ADR-003-defect-vulnerability-audit.md`（缺陷/漏洞清单）。
> 目的：把设计文档 `agent-fusion-architecture.md` 的「基色」、2026 主流开源 agent 的**已验证安全/生产能力边界**、与 ganyu-agent 当前实现三方对照，给出可落地的「取长补短」改造清单。
> 调研方法：WebSearch 核实各框架最新版本与安全事件（CVE 编号、CVSS、补丁版本、来源 URL 见文末）。

---

## 1. 设计「基色」回顾（agent-fusion-architecture.md 的承诺）

四源融合（Pi / OpenClaw / Hermes / Prime-smolagents），Hermes v0.20 底座，模型路由网关（ai-router + OmniRoute 4 层級联）。关键安全基色：

| 基色 | 设计承诺 | 落地状态（见 ADR-003） |
|---|---|---|
| 执行沙箱 | 沙箱默认 Docker，代码执行隔离 | ❌ 未实现（C1/H3） |
| 隐私硬路由 | 敏感会话强制走本地 Ollama，不出网 | ❌ 未实现（无敏感分类） |
| 密钥加密 | 密钥 AES-256-GCM 加密落盘 | ❌ 明文 JSON（H1） |
| webhooks HMAC | 回调签名校验 | ❌ 无 webhook 服务 |
| Token 压缩 | RTK + Caveman 15–95% | ❌ 未实现 |
| 自愈网关 | 级联 / 熔断 / lkgp | ✅ 已实现（扎实） |
| A2A / 通道 | OpenClaw 通道生态 | ❌ 单进程 |
| OpenViking | 记忆代理 + 自愈兜底 | ❌ 空实现（M1） |

**结论**：ganyu-agent 把「架构/抽象/自愈」做到了设计承诺的水位，但「执行安全」整层低于设计承诺。这不是单点 bug，而是执行层缺位。

---

## 2. 2026 开源 agent 框架：能力边界与安全落地（已核实）

| 框架 | 最新版本（核实） | 沙箱/执行隔离 | 工具调用安全 | 记忆 | 路由/自愈 | 生产成熟度 | 关键安全事件 |
|---|---|---|---|---|---|---|---|
| **smolagents** (HF) | **1.26.0**（2026-05-29） | 默认 `LocalPythonExecutor` **非安全边界**；官方建议接沙箱 | 文本协议，靠沙箱兜底 | 无持久层 | 无内置路由 | 轻量原型/教学主流 | **CVE-2025-5120** 沙箱逃逸（CVSS 9.9，1.17.0 修） |
| **LangGraph** (LangChain) | **1.0.10+**（checkpoint-sqlite ≥3.0.1） | 无执行沙箱（编排层） | 无工具鉴权 | **checkpointer 持久层** | 有状态图、重试 | 4600万+ 月下载，企业主力 | **CVE-2025-67644**(SQLi 7.3)+**CVE-2026-28277**(msgpack RCE 7.4)+**CVE-2026-27022**(Redis) 链式 RCE |
| **CrewAI** | VU#221883（已修，移除 CodeInterpreter） | 曾 Docker 沙箱，**失败时静默降级**到不安全 SandboxPython | 路径/URL 校验后补（PR #5310/#5315） | 无 | 无 | 多 agent 编排流行 | **CVE-2026-2275**(ctypes RCE 8.6)/**-2287**(静默降级)/**-2285**(路径遍历)/**-2286**(SSRF) |
| **AutoGen / AG2** (MS) | AG2 社区续维护（MS 转维护态） | **Docker 沙箱 + Studio 0.4.8 gVisor/seccomp** | 代码执行内置 | 无 | 多 agent 对话 | 研究/实验主流 | 沙箱实践较稳 |
| **OpenHands** (原 OpenDevin) | **v0.21.0**（MIT） | **每会话独立 Docker 容器，默认隔离** | 容器内执行，宿主机隔离 | 会话级 | 事件流、可中断 | issue→PR 主流 | 默认沙箱，口碑好 |
| **Agno** (原 Phidata) | **v2.7.4** | **Superserve 基于 Firecracker 微 VM** 沙箱 | 100+ 工具集成 | PostgreSQL/ClickHouse，RBAC+审计 | 运行时+控制面 | 平台级自托管 | 隔离较强 |
| **Google ADK** | < **2.5.0** 受影响 | 无内建沙箱 | **工具确认未鉴权** | 会话历史 | 多 agent | 云原生 | **CVE-2026-18236** 持续伪造（CVSS 9.3，CWE-863） |
| **Pydantic AI** | 2024末起，最快增长 | 无内建沙箱 | **类型安全工具 I/O + 校验**（结构化输出） | 无 | 图/持久化集成 | 企业类型安全首选 | 靠 schema 校验减面 |
| **Letta** (MemGPT) | 记忆优先 | 无内建沙箱 | 内存块读写 | **分层持久记忆**（隐私/留存风险） | 有状态 | 长程对话 | 记忆即安全面 |

**跨框架共性结论（Lyrie Research《When Prompts Become Shells》、Check Point、CERT/CC 一致）**：
1. **沙箱默认打开是 2026 的及格线**：OpenHands/AutoGen/Agno 默认隔离；smolagents/LangGraph 自己不跑代码但要求用户接沙箱。ganyu 当前 `exec` 无沙箱，**低于及格线**。
2. **「失败开放（fail-open）」是头号杀手**：CrewAI CVE-2026-2287（Docker 不可达静默降级到不安全沙箱）证明——降级不能悄悄发生，必须 fail-closed 并告警。ganyu 的 `exec` 等价于「永远无沙箱」，比 CrewAI 更糟。
3. **持久层即攻击面**：LangGraph checkpointer 链式 RCE 说明「记忆/状态存储」是与执行同等危险的结构面。ganyu 明文 JSON 记忆（H1）属同类风险，只是尚未联网暴露。
4. **工具鉴权必须在执行层强制**：Google ADK CVE-2026-18236（确认伪造）说明不能靠系统提示约束工具调用，要在执行层校验「工具是否注册/是否需要确认/参数是否匹配」。对应 ganyu M5。
5. **类型化工具 I/O 减面**：Pydantic AI 用 schema 校验工具入参/出参，ganyu 全量 `Value(String)` 缺少这层，路径/URL 校验只能事后补（C3/C4/C5）。

---

## 3. ganyu-agent 的长处（必须保留，且是相对开源的差异化优势）

- **统一抽象领先**：`Unit`/`RunContext`/`Workflow` 三层让 7 大范式零重复脚手架。多数开源框架是单范式（CrewAI 角色组、LangGraph 图、AutoGen 对话）——ganyu 的多范式统一是其架构亮点。
- **离线优先安全默认**：`LocalBackend`/`LocalReasoner`/`KeywordRouter` 零密钥可跑，避免了一上来就暴露联网面。
- **自愈网关扎实**：级联 fallback + 熔断器 + lkgp 粘路径（routing/mod.rs、heal/mod.rs），有测试覆盖，质量高于多数框架的路由实现。
- **安全求值样板已存在**：`calc` 用严格正则白名单 `[0-9+\-*/().\s]+`——这是 `exec` 应效仿的正确范式（sandbox + allowlist，而非裸 shell）。
- **SAG 模板路径防注入**：`template_sql` 用 `enum`/固定区列表（region 来自 `ZONES` 白名单、top_n 为 `usize`），模板路径无注入——只差 LLM 路径的参数化（H2）。
- **ReAct 步数上限** `MAX_STEPS=8` 防死循环。
- **会话 UUID 贯通** Agent/SAG/记忆提交，可观测锚点清晰。

---

## 4. 取长补短：把开源成熟实践与设计基色落到 ganyu（改造清单）

> 优先级对齐 ADR-003 的 P0–P3。每条给出「学谁 / 怎么做」。

### P0 — 阻断发布级（对应 C1/C2/C3/C4/C5）
- **[学 OpenHands/AutoGen] 执行默认隔离 + fail-closed**：`exec`/`CommandTool` 一律经沙箱代理（Docker 微 VM / Landlock+seccomp / Firecracker）。沙箱不可用时**拒绝执行并高优告警**，绝不降级到宿主机（吸取 CrewAI CVE-2026-2287 教训）。
- **[学 CrewAI 修复] 默认关闭危险能力**：`exec` 默认禁用，需 `--allow-exec` 显式开启；`discover("plugins")` 默认关闭，插件清单需签名/白名单（修 C2）。
- **[学 CrewAI CVE-2026-2285] 文件工具加 sandbox root + 路径规范化**：`file_read`/`file_write` 限定根目录，拒绝 `..`/绝对路径/symlink 逃逸（修 C3/C4）。
- **[学 CrewAI CVE-2026-2286] web_fetch 加 SSRF 防护**：仅 http(s)，解析 IP 后拒绝 `127/8`、`10/8`、`172.16/12`、`192.168/16`、`169.254/16`、`::1`，禁重定向到内网（修 C5）。

### P1 — 高危级（对应 H1/H2/H3）
- **[学 LangGraph 教训] 记忆加密 + 参数化**：敏感命名空间 `viking://user/memory/*`、`.../sessions/*` 用 AES-256-GCM 加密落盘（密钥来自 env/KMS，绝不入库）；`LocalMemory::save` 改每 key 独立文件或 WAL，避免整文件重写竞态（修 H1/M4）。
- **[学 LangGraph SQLi] SAG 参数化 + 注入检测**：region/top_n 以 bind 参数传入，禁止字符串拼接；`validate_sql` 增加注入特征检测（多语句/注释/非白名单表/子查询越权）（修 H2）。
- **[对齐设计基色] 落地沙箱层**：先 Landlock+seccomp（Linux）兜底，再接 Docker 微 VM；`network` 构建默认开网络隔离（修 H3）。

### P2 — 中危级（对应 M1–M6）
- **[诚实] 清理 OpenViking 空实现**：实现真正远端代理（超时+失败降级）或删除该类型并改注释为「预留扩展点」（修 M1）。
- **[学自愈网关] 加速率/成本护栏**：网关增令牌桶速率限制 + 单次会话 token 预算；`exec`/网络工具调用次数上限（修 M2）。
- **[学 CrewAI 修复] 副作用工具不盲目重试**：工具标注 `side_effect`，有副作用者默认不重试（修 M3）。
- **[学 Pydantic AI] 类型化工具 I/O + 输出脱敏**：关键工具参数做 schema 校验；工具输出回流前截断长度 + 敏感内容（密钥模式）脱敏（修 M5/C3-C5）。
- **[学 Pydantic AI/OpenAI] 原生 function calling**：`OpenAiBackend` 支持 `tools`/`tool_calls`，结构化解析动作，替代脆弱的 `@tool` 文本协议（修 M6）。

### P3 — 低危级（对应 L1/L2）
- **[学机密管理] API Key 用 `secrecy::SecretString` + `zeroize`**（修 L1）。
- **[学 Agno 运行时] 网关后端热更新**：`RwLock<Vec>` 或消息通道，支持运行期增删后端（修 L2）。

---

## 5. 一句话总结

ganyu-agent 的**抽象与自愈已达生产级**，但**执行安全层停留在「便利优先」阶段**——默认构建即带 RCE/任意读写/SSRF 且无沙箱。2026 年开源共识是「**沙箱默认开 + fail-closed + 执行层鉴权 + 持久层加密**」。取长补短的方向明确：**保留统一抽象与自愈网关，把 OpenHands/AutoGen 的沙箱默认、CrewAI 修复后的 fail-closed 与路径/URL 校验、LangGraph 教训后的记忆加密、Pydantic AI 的类型化工具 I/O 落到 ganyu**，即可把设计文档的「基色」从承诺变为实现。

---

## 6. 来源 URL（核实依据）
- smolagents CVE-2025-5120：https://nvd.nist.gov/vuln/detail/CVE-2025-5120 ；版本线：https://deps.dev/advisory/osv/PYSEC-2026-542
- LangGraph checkpointer RCE 链：https://cyfar.ca/posts/from-sqli-to-rce-exploiting-langgraphs-checkpointer ；https://labs.cloudsecurityalliance.org/research/csa-research-note-langgraph-rce-chain-20260615-csa-styled
- CrewAI VU#221883：https://www.kb.cert.org/vuls/id/221883 ；链析：https://safeguard.sh/resources/blog/crewai-sandbox-escape-cve-chain-2026
- AutoGen/OpenHands 沙箱实践：https://www.docker.com/blog/comparing-sandboxing-approaches-ai-agents ；OpenHands v0.21.0：https://oss.iqrator.org?p=3938/
- Agno v2.7.4（Firecracker 沙箱）：https://cloud.tencent.com.cn/developer/article/2713159
- Google ADK CVE-2026-18236：https://nvd.nist.gov/vuln/detail/CVE-2026-18236
- Pydantic AI / 框架全景：https://builderai.tools/blog/agent-framework-landscape-2026-buyers-guide ；https://uvik.net/blog/agentic-ai-frameworks
