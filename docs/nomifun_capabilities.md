# nomifun 内置 agent 能力 · 全量赋能清单（盘点）

本文档盘点 ganyu-agent 当前的能力层，并把 **nomifun 平台全部内置 agent 能力（33 项）** 接入本项目的 agent 体系，使任意 agent（ReAct / 多智能体 / 路由 / Blackboard / Graph）都能「认识 + 调用」它们。

---

## 1. 盘点：ganyu-agent 原有能力层

| 层 | 现状 | 缺口 |
|----|------|------|
| 工具 `ToolRegistry` | `echo / calc / file_read / file_write / file_list / remember / recall / rag_search`（+ 可选 `exec`、`web_fetch`） | 仅本地/离线，无「领域方法论」 |
| 技能 `SkillBook` | 仅 3 个：`summarize` / `troubleshoot` / `kb_query` | 没有代码/测试/安全/视频/设计/架构等专家能力 |
| 意图路由 `match_intent` | 仅 11 条关键字规则，指向上述 3 技能 | 无法把自然语言意图路由到专家能力 |
| 网关 `Gateway` | LLM 后端 + 熔断/降级 | — |

**结论**：agent 缺「领域专家能力」。nomifun 平台的 33 项内置 agent 能力正好补齐这一环。

---

## 2. 赋能方式（代码落点）

- 新增模块 `src/ext/nomifun_caps.rs`：
  - `NOMIFUN_CAPS`：全量 33 项能力的目录（`name` / `description` / `keywords` / `guidance`）。
  - `register_nomifun_skills(book)`：把每项注册为 `Skill`，步骤为 `SkillStep::Call { tool: "nomifun_skill", arg: "cap=<name> {input}" }`。
  - `match_nomifun_intent(q)`：按关键词做意图路由。
  - `NomifunSkillTool`：派发工具 —— 离线返回能力的方法论 SOP；配置 `GANYU_NOMIFUN_GATEWAY` 时走真实桥接。
- 修改 `src/ext/mod.rs`：`pub mod nomifun_caps;` + `match_intent` 末尾追加 nomifun 路由。
- 修改 `src/main.rs`：注册 `NomifunSkillTool`，调用 `register_nomifun_skills`，并自然把 33 项包成 `skill:<name>` 工具。

效果：
1. **认识**：`tools` 列表 / `tools` 子命令现在包含 33 个 `skill:<name>`。
2. **路由**：`KeywordRouter` 等工作流经 `match_intent` 命中 nomifun 关键词后派发到对应 skill。
3. **调用**：`skill:<name>` 对 ReAct / 多智能体 / 路由 agent 可见，LLM 可直接点名调用；也支持 CLI `ganyu skill <name> <参数>`。
4. **执行**：默认离线返回该能力的方法论 SOP（agent 据此自行执行）；设 `GANYU_NOMIFUN_GATEWAY`（形如 `nomifun skill {cap} {input}`）后真实派发到 nomifun 平台，程序名受安全白名单约束。

---

## 3. 全量能力清单（33 项）

| # | 能力名 | 一句话能力 | 触发关键词（节选） |
|---|--------|-----------|--------------------|
| 1 | `agent-git-oracle` | 仓库分析与重构指南（AI 推理技术债/反模式） | git oracle / 技术债 / 架构反模式 / 仓库分析 |
| 2 | `ai-video-clipper` | 全自动 AI 视频剪辑 | ai 视频剪辑 / 自动剪辑 / 短视频生成 / 批量剪辑 |
| 3 | `ai-agentic-video-editor` | 全自主 agentic 视频编辑面 | agentic 视频 / 自动生成视频 / 视频编辑面 |
| 4 | `api-test-automation` | API 测试自动化（REST/GraphQL） | 接口测试 / api 测试 / 性能测试 / 契约测试 / mock |
| 5 | `bug-fixing-openclaw` | 零回归 bug 修复工作流 | 修复 bug / bug 修复 / 零回归 / 修 bug |
| 6 | `clean-code` | 务实编码规范 | clean code / 编码规范 / 命名规范 / 可读性 |
| 7 | `code-analyzer` | 深度代码分析（DDD 模式识别） | 代码分析 / 架构分析 / ddd 识别 / 技术债务评估 |
| 8 | `code-error-fixer` | 系统化代码错误诊断与修复 | 编译错误 / 运行时异常 / 类型错误 / 逻辑 bug |
| 9 | `code-refactoring` | 代码重构模式与技巧 | 重构 / refactor / 降低复杂度 / 可维护性 |
| 10 | `code-review-assistant` | 代码 Review 助手（中文报告） | 代码 review / code review / 审查报告 |
| 11 | `critical-code-reviewer` | 严苛对抗式代码审查 | 严苛审查 / critical review / 对抗式审查 |
| 12 | `debug-pro` | 系统化调试方法论 | 调试 / debug / 排错 / 诊断 |
| 13 | `design-to-code` | 从设计稿实现像素级 UI | 设计稿 / 切图 / 设计转代码 / 还原设计 / figma |
| 14 | `e2e-testing-patterns` | 可靠 E2E 测试（Playwright/Cypress） | e2e / 端到端测试 / playwright / cypress |
| 15 | `ffmpeg-video-editor` | 自然语言生成 FFmpeg 命令 | ffmpeg / 视频命令 / 转码 / 裁剪视频 / 压缩视频 |
| 16 | `frontend-design-pro` | 前端设计质量提升 | 前端设计 / 设计质量 / ui 审计 / polish |
| 17 | `log-analyzer` | 日志解析/检索/分析 | 日志 / log / 日志分析 / 堆栈分析 |
| 18 | `nexus-error-explain` | 解释错误信息并给修复 | 错误解释 / error explain / 报错含义 |
| 19 | `pr-reviewer` | GitHub PR 自动化审查 | pr 审查 / pr review / pull request / github pr |
| 20 | `prd-to-prototype` | 从需求到可交互原型 | prd / 产品原型 / 高保真原型 / 交互原型 |
| 21 | `project-code-standard` | 项目代码规范检查/执行/修复 | 代码规范 / lint / 规范检查 / eslint / pep8 |
| 22 | `remotion-video-toolkit` | Remotion+React 程序化视频 | remotion / react 视频 / 程序化视频 |
| 23 | `security-audit` | Clawdbot 部署安全审计 | 安全审计 / security audit / 漏洞扫描 / 凭证泄露 |
| 24 | `simplify` | 为清晰/一致/可维护性重构 | 简化 / simplify / 精简代码 |
| 25 | `superpowers-systematic-debugging` | 系统性调试（拒绝随机补丁） | 系统调试 / systematic debugging / 科学调试 |
| 26 | `superpowers-tdd` | 测试驱动开发 | tdd / 测试驱动 / 先写测试 / 红绿重构 |
| 27 | `system-architect` | 资深系统架构师 | 系统架构 / 架构设计 / system architect |
| 28 | `test-patterns` | 跨语言/框架测试编写与运行 | 测试模式 / 单元测试 / 集成测试 / test patterns |
| 29 | `ui-design` | 综合 UI 设计 | ui 设计 / 界面设计 / 视觉设计 / 可访问性 |
| 30 | `uncle-bob` | Uncle Bob 原则（SOLID/整洁架构） | uncle bob / solid / 整洁架构 / clean architecture |
| 31 | `video-clip-assistant` | 视频自动剪辑助手 | 视频剪辑助手 / 精彩片段 / 视频字幕 |
| 32 | `video-editor` | 视频剪辑操作 | 视频编辑 / 视频合并 / 视频转换 / 视频处理 |
| 33 | `wireframe` | 线框图与用户流 | 线框图 / wireframe / 用户流 / 页面布局 |

---

## 4. 用法

```bash
# 查看全部已接入能力（含 33 个 skill:<name>）
ganyu tools

# 直接调用某个 nomifun 能力（离线：返回方法论 SOP）
ganyu skill code-review-assistant "src/ext/nomifun_caps.rs"

# 多智能体 / 路由工作流里，自然语言命中关键词会自动路由到对应 skill
ganyu agent "帮我做一下这个仓库的安全审计" --mode multi
ganyu run "把这段 Figma 设计稿还原成 HTML"

# 真实桥接（可选）：把能力派发到 nomifun 平台执行
# GANYU_NOMIFUN_GATEWAY='nomifun skill {cap} {input}'
export GANYU_NOMIFUN_GATEWAY='nomifun skill {cap} {input}'
ganyu skill code-review-assistant "src/ext/mod.rs"
```

> 离线优先：未设置 `GANYU_NOMIFUN_GATEWAY` 时，`skill:<name>` 返回该能力的结构化方法论 SOP，agent 据此自行执行，零网络依赖。

---

## 5. 同步真实技能内容（SKILL.md）

上一节的「方法论 SOP」只是兜底；本仓库已把 nomi agent 的**真实内置 skill 定义**同步进 `skills/nomifun/<name>/`，离线即返回完整的 `SKILL.md`（含 `references`）：

- `nomifun_skill` 工具解析技能根目录的优先级：
  1. 环境变量 `GANYU_NOMIFUN_SKILLS_DIR`（指向任意技能目录）
  2. 本仓库随附的 `skills/nomifun`（已同步快照）
  3. nomifun 实时目录 `%LOCALAPPDATA%/NomiFun/skills`
- 找到后返回 `<dir>/<name>/SKILL.md`（去掉 YAML frontmatter）及其 `references/*.md`；
  全部缺失才回退到目录内置的 SOP。
- 个别文件夹名与注册名不一致，已在 `folder_for_cap()` 中映射：
  `ai-agentic-video-editor→ai_agentic_video_editor`、`ffmpeg-video-editor→FFmpeg Video Editor`、`prd-to-prototype→PRD to Prototype`。

### 重新同步（当 nomifun 侧技能更新时）

```powershell
$src = "$env:LOCALAPPDATA\NomiFun\skills"
$dst = "D:\workbuddy_all\harness_all\ganyu-agent\skills\nomifun"
if (Test-Path $dst) { Remove-Item $dst -Recurse -Force }
New-Item -ItemType Directory -Path $dst | Out-Null
Get-ChildItem $src -Directory | ForEach-Object {
    Copy-Item $_.FullName -Destination (Join-Path $dst $_.Name) -Recurse
}
```

> 提示：同步进仓库的是 nomifun 内置技能的**内容快照**。若希望 ganyu 始终使用最新版本，
> 可不依赖仓库内的 `skills/nomifun`，改用 `GANYU_NOMIFUN_SKILLS_DIR` 指向实时目录
> （如 `$env:LOCALAPPDATA\NomiFun\skills`），即“零拷贝实时同步”。

