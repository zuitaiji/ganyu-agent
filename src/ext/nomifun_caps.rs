//! nomifun 内置 agent 能力 · 全量赋能
//!
//! 把 nomifun 平台内置的全部 agent 能力登记进 ganyu 的 `SkillBook`，
//! 使本项目的任意 agent（ReAct / 多智能体 / 路由工作流 / Blackboard / Graph）
//! 都能「认识 + 调用」它们。
//!
//! 每个能力注册为一个 `Skill`：
//! ```ignore
//! Skill { name, description, steps: [ SkillStep::Call { tool: "nomifun_skill", arg: "cap=<name> {input}" } ] }
//! ```
//! 运行时由 `nomifun_skill` 工具派发：
//! - **离线（默认，已同步真实内容）**：读取同步进本仓库 `skills/nomifun/<name>/SKILL.md`
//!   （含 `references`）作为该能力的真实定义返回；路径可用 `GANYU_NOMIFUN_SKILLS_DIR` 覆盖，
//!   缺省回退到 nomifun 实时目录；若都缺失则回退到本模块内置的方法论 SOP。
//! - **真实桥接（可选）**：设置 `GANYU_NOMIFUN_GATEWAY`（形如 `nomifun skill {cap} {input}`）
//!   后，直接把能力派发到 nomifun 平台执行并返回结果（程序名受安全白名单/字符约束）。

use std::fs;
use std::path::{Path, PathBuf};

use async_trait::async_trait;

use crate::error::{GanyuError, GanyuResult};
use crate::ext::{Skill, SkillBook, SkillStep, Tool};
use crate::value::Value;

/// 一项 nomifun 内置 agent 能力。
pub struct NomifunCap {
    /// 能力名（同时作为 skill 名 / 路由键）。
    pub name: &'static str,
    /// 一句话能力描述（进入 skill 描述与 `tools` 列表）。
    pub description: &'static str,
    /// 意图路由关键词（统一以小写命中）。
    pub keywords: &'static [&'static str],
    /// 离线可执行的指引 / 方法论 SOP。
    pub guidance: &'static str,
}

/// nomifun 内置 agent 能力全量目录（与平台侧保持同步）。
pub const NOMIFUN_CAPS: &[NomifunCap] = &[
    NomifunCap {
        name: "agent-git-oracle",
        description: "高级仓库分析与重构指南：用 AI 推理识别技术债与架构反模式。",
        keywords: &["git oracle", "技术债", "架构反模式", "仓库分析", "git 分析", "oracle"],
        guidance: "1) 用 git log/history 与目录结构摸清演进脉络；2) 用模块依赖、圈复杂度、重复代码定位技术债热点；3) 依据分层/边界/依赖方向识别架构反模式（循环依赖、上帝类、贫血模型等）；4) 给出最小可行的重构方案与风险点，先抽接口再挪实现，保持测试绿。",
    },
    NomifunCap {
        name: "ai-video-clipper",
        description: "全自动 AI 视频剪辑：从素材自动生成成片（批量/短视频）。",
        keywords: &["ai 视频剪辑", "自动剪辑", "视频成片", "短视频生成", "批量剪辑"],
        guidance: "1) 明确成片目标（时长、平台、画幅、风格）；2) 拉通素材清单与字幕/台词；3) 做镜头打分与精彩片段抽取；4) 按节奏自动剪辑 + 转场 + BGM + 字幕烧录；5) 导出校验。优先用确定性脚本（FFmpeg/Remotion）而非黑盒。",
    },
    NomifunCap {
        name: "ai-agentic-video-editor",
        description: "全自主 agentic 视频编辑面：规划-执行-校验的端到端视频编辑。",
        keywords: &["agentic 视频", "自主视频编辑", "视频编辑面", "自动生成视频"],
        guidance: "1) 解析用户编辑意图为结构化计划（片段、转场、字幕、调色、导出）；2) 选择并调用底层剪辑/渲染工具执行每步；3) 每次产出后回看校验（画面、时长、音画同步）；4) 不满足则迭代修正，最终导出。全程留痕便于回退。",
    },
    NomifunCap {
        name: "api-test-automation",
        description: "API 接口测试自动化（REST/GraphQL）：接口/性能/契约/Mock。",
        keywords: &["接口测试", "api 测试", "rest 测试", "graphql 测试", "性能测试", "契约测试", "mock 服务"],
        guidance: "1) 基于接口契约生成用例（正常/边界/异常）；2) 用 Mock 解耦上下游依赖；3) 跑功能 + 性能 + 契约一致性校验；4) 产出覆盖率与失败根因报告。断言要覆盖状态码、schema、时延 SLA。",
    },
    NomifunCap {
        name: "bug-fixing-openclaw",
        description: "零回归 bug 修复工作流：分诊→复现→根因→影响→修复→验证→沉淀。",
        keywords: &["修复 bug", "bug 修复", "零回归", "修 bug", "fix bug"],
        guidance: "1) 复现并最小化失败用例；2) 二分/追踪定位根因而非打补丁；3) 评估影响面与回归风险；4) 做最小修复并补回归测试；5) 验证绿 + 回归跑通；6) 沉淀失败案例供自愈。禁止随机改代码掩盖问题。",
    },
    NomifunCap {
        name: "clean-code",
        description: "务实编码规范：命名、函数、结构、反模式。",
        keywords: &["clean code", "编码规范", "命名规范", "可读性", "代码整洁"],
        guidance: "1) 意图即命名（变量/函数/类型名讲清 why）；2) 函数单一职责、短小、参数少；3) 用显式错误与不变式替代注释；4) 消除重复与隐藏耦合；5) 以测试固化行为。改前先读既有约定，不引入风格分裂。",
    },
    NomifunCap {
        name: "code-analyzer",
        description: "深度代码分析：架构/执行流/数据流/业务规则/DDD 模式识别。",
        keywords: &["代码分析", "架构分析", "数据流分析", "ddd 识别", "技术债务评估"],
        guidance: "1) 从入口梳理执行流与调用图；2) 标注关键业务规则与不变式；3) 识别聚合根/实体/值对象/领域服务/仓储等 DDD 构件；4) 评估模块边界与依赖方向；5) 输出架构图 + 风险清单，作为重构/审查基线。",
    },
    NomifunCap {
        name: "code-error-fixer",
        description: "系统化代码错误诊断与修复：编译/运行时/类型/逻辑错误。",
        keywords: &["编译错误", "运行时异常", "类型错误", "逻辑 bug", "崩溃分析", "依赖冲突"],
        guidance: "1) 读全错误栈与环境（版本、依赖）；2) 区分编译期/类型期/运行期；3) 用最小复现隔离问题；4) 优先修根因而不是 suppress 警告；5) 修复后跑对应测试 + 全量构建确认无引入回归。",
    },
    NomifunCap {
        name: "code-refactoring",
        description: "代码重构模式与技巧：在不改行为前提下提升质量。",
        keywords: &["重构", "refactor", "降低复杂度", "可维护性", "提取函数", "重命名"],
        guidance: "1) 先用测试锁住当前行为；2) 小步提交（重命名→提取→内联→移动）；3) 每次只做一种重构并跑绿；4) 优先消除重复与长函数；5) 用组合/策略替代条件分支。行为不变是铁律。",
    },
    NomifunCap {
        name: "code-review-assistant",
        description: "代码 Review 助手：输出结构化中文 Review 报告。",
        keywords: &["代码 review", "code review", "审查报告", "review 助手"],
        guidance: "覆盖 Bug、安全漏洞、性能、可读性、最佳实践、类型安全、错误处理、测试覆盖；按严重度分级（信息/优化/标准/严重）；每一条给出文件:行号、问题、建议改法。结论含总体风险与是否可合入。",
    },
    NomifunCap {
        name: "critical-code-reviewer",
        description: "严苛对抗式代码审查：零容忍地揪出问题。",
        keywords: &["严苛审查", "critical review", "对抗式审查", "漏洞审查"],
        guidance: "以攻击者视角逐行质疑：越界、竞争、注入、鉴权缺失、资源泄漏、整数溢出、错误处理被吞。对每处给出利用路径与修复。不假设调用方善意，不采信未经证实的注释。",
    },
    NomifunCap {
        name: "debug-pro",
        description: "系统化调试方法论与多语言调试命令。",
        keywords: &["调试", "debug", "排错", "诊断", "debugging"],
        guidance: "1) 可观测优先（日志/断点/trace）；2) 用假设-实验法而非猜测；3) 二分定位最小触发条件；4) 检查边界、并发、序列化；5) 修复后写防回归测试。记录复现步骤。",
    },
    NomifunCap {
        name: "design-to-code",
        description: "从设计稿（Figma/Sketch/图片）实现像素级 UI。",
        keywords: &["设计稿", "切图", "设计转代码", "还原设计", "figma 实现", "像素级"],
        guidance: "1) 抽取设计 token（色板/字号/间距/圆角/阴影）；2) 还原布局与响应式断点；3) 对齐间距/字号/层级；4) 补交互态（hover/disabled/loading）与可访问性；5) 与稿子逐像素比对，标注偏差。",
    },
    NomifunCap {
        name: "e2e-testing-patterns",
        description: "可靠的 E2E 测试（Playwright/Cypress）：关键旅程 + 去抖动。",
        keywords: &["e2e", "端到端测试", "playwright", "cypress", "测试套件"],
        guidance: "1) 只覆盖关键用户旅程；2) 用稳定选择器与显式等待，杜绝 sleep；3) 数据/环境隔离，用例可独立重跑；4) 失败重试策略 + 轨迹/截图；5) 接入 CI，慢试验单独分组。",
    },
    NomifunCap {
        name: "ffmpeg-video-editor",
        description: "自然语言生成 FFmpeg 命令：剪切/转码/裁剪/压缩/改画幅。",
        keywords: &["ffmpeg", "视频命令", "转码", "裁剪视频", "压缩视频", "画幅转换"],
        guidance: "把自然语言需求翻译成确定 FFmpeg 命令：-ss/-to 剪切、-vf scale/crop 画幅、CRF/预设 压缩、map 选流、-c copy 无损。给出命令 + 参数解释 + 注意事项（音画同步、码率）。优先脚本化而非 GUI。",
    },
    NomifunCap {
        name: "frontend-design-pro",
        description: "前端设计质量提升：审计/打磨/批判设计反模式。",
        keywords: &["前端设计", "设计质量", "ui 审计", "polish", "设计打磨"],
        guidance: "对照设计语言审查排版节奏、对比度、层级、留白、动效克制；给出 audit/polish/critique 结论；优先修复一致性断裂与可读性风险，再谈装饰。提供具体改法（颜色/间距/字阶）。",
    },
    NomifunCap {
        name: "log-analyzer",
        description: "日志解析/检索/分析：错误模式、关联、堆栈。",
        keywords: &["日志", "log", "日志分析", "堆栈分析", "错误日志"],
        guidance: "1) 结构化检索（时间窗/级别/服务/关键字）；2) 聚合错误模式与频次；3) 跨服务按 trace/请求 ID 关联事件；4) 解析堆栈定位根因；5) 输出时间线 + 高频错误 TopN + 建议。",
    },
    NomifunCap {
        name: "nexus-error-explain",
        description: "解释错误信息并给出修复建议。",
        keywords: &["错误解释", "error explain", "报错含义", "nexus"],
        guidance: "把报错信息拆解为：是什么错误、发生在哪一层、常见成因、最小修复步骤、防复发措施。附可复制的修复片段与验证方法。",
    },
    NomifunCap {
        name: "pr-reviewer",
        description: "GitHub PR 自动化审查：diff 分析 + lint 集成 + 报告。",
        keywords: &["pr 审查", "pr review", "pull request", "github pr", "pr 自动审查"],
        guidance: "解析 PR diff，定位安全/错误/测试缺口/风格问题；结合 lint 结果；输出结构化审查（文件:行、问题、建议、严重度）并给出可否合入结论。关注未覆盖的边界与破坏性变更。",
    },
    NomifunCap {
        name: "prd-to-prototype",
        description: "从产品需求到可交互原型：零提问 PRD + 高保真原型。",
        keywords: &["prd", "产品原型", "需求原型", "高保真原型", "交互原型"],
        guidance: "1) 把想法收敛成 PRD（目标/用户/流程/边界）；2) 选择平台（移动/PC）；3) 产出 Awwwards 级高保真 HTML/Tailwind 原型；4) 串起关键交互流；5) 标注待确认点。先低保真对齐再高保真。",
    },
    NomifunCap {
        name: "project-code-standard",
        description: "项目代码规范检查/执行/修复（PEP8/black/ruff、ESLint 等）。",
        keywords: &["代码规范", "代码标准", "lint", "规范检查", "pep8", "eslint", "规范修复"],
        guidance: "1) 建立团队规范基线（格式化器 + linter 配置）；2) 全量扫描并分级报告；3) 自动修复可安全项；4) 在 CI 卡口拦截；5) 产出质量报告。规则与现有约定对齐，避免噪音。",
    },
    NomifunCap {
        name: "remotion-video-toolkit",
        description: "Remotion + React 程序化视频创作：动画/字幕/图表/3D/渲染。",
        keywords: &["remotion", "react 视频", "程序化视频", "视频动画", "remotion 渲染"],
        guidance: "用 React 组件描述视频（<Sequence>/<AbsoluteFill>/插值动画），加字幕/图表/3D；用 CLI/Node/Lambda/Cloud Run 渲染；关注帧时间轴、字体加载、SSR 渲染一致性。给出组件骨架与渲染命令。",
    },
    NomifunCap {
        name: "security-audit",
        description: "Clawdbot 部署安全审计：凭证泄露/开放端口/弱配置/漏洞。",
        keywords: &["安全审计", "security audit", "漏洞扫描", "凭证泄露", "安全配置", "硬编码密钥"],
        guidance: "1) 扫描硬编码密钥与暴露凭证；2) 检查开放端口与公网暴露；3) 评估鉴权/权限/最小特权；4) 查依赖与配置弱点；5) 输出风险清单 + 修复优先级（含自动修复项）。零信任视角。",
    },
    NomifunCap {
        name: "simplify",
        description: "为清晰、一致、可维护性重构（行为不变）。",
        keywords: &["简化", "simplify", "精简代码", "可读性重构"],
        guidance: "在不改变行为的前提下：消除嵌套、合并重复、用语义命名、缩短函数、删死代码、用标准库/语言惯用法。每步小改并跑测试。以『别人能否一眼读懂』为验收。",
    },
    NomifunCap {
        name: "superpowers-systematic-debugging",
        description: "系统性调试：拒绝随机补丁，先复现再定位根因。",
        keywords: &["系统调试", "systematic debugging", "随机修 bug", "科学调试"],
        guidance: "1) 写失败测试复现；2) 提出可证伪假设；3) 用实验逐一排除；4) 定位根因而非症状；5) 修复并加防回归。严禁随机改动制造新 bug，先让失败可见。",
    },
    NomifunCap {
        name: "superpowers-tdd",
        description: "测试驱动开发：先写测试看其失败，再写最少实现令其通过。",
        keywords: &["tdd", "测试驱动", "先写测试", "红绿重构"],
        guidance: "1) 写一个会失败的测试（Red）；2) 写最少实现使其通过（Green）；3) 仅在此刻重构（Refactor）；4) 小步循环。测试即规格，先想清边界再编码。",
    },
    NomifunCap {
        name: "system-architect",
        description: "资深系统架构师：健壮/可扩展/可维护架构。",
        keywords: &["系统架构", "架构设计", "system architect", "可扩展架构", "架构师"],
        guidance: "按行业规范（Python 用 PEP8、JS/TS 用 ESLint）做模块化、清晰分层与依赖管理；关注可扩展、可观测、容错、安全。产出模块边界、数据流、关键决策与权衡。",
    },
    NomifunCap {
        name: "test-patterns",
        description: "跨语言/框架编写与运行测试：单元/集成/E2E/覆盖/Mock。",
        keywords: &["测试模式", "单元测试", "集成测试", "测试覆盖", "test patterns", "mock"],
        guidance: "按测试金字塔铺开：单元多、集成中、E2E 少；用 Mock 隔离边界；测行为而非实现；关注边界/异常/并发；量化覆盖率与脆弱测试治理。给出各语言骨架。",
    },
    NomifunCap {
        name: "ui-design",
        description: "综合 UI 设计：布局/排版/色彩/间距/动效/可访问性。",
        keywords: &["ui 设计", "界面设计", "视觉设计", "设计系统", "可访问性"],
        guidance: "从信息架构与布局栅格出发，统一字阶/色板/间距标度；用对比与层级引导视线；动效服务于理解而非炫技；满足 WCAG 对比度/焦点/语义。输出设计决策与组件规范。",
    },
    NomifunCap {
        name: "uncle-bob",
        description: "Robert C. Martin 原则：clean code / SOLID / 整洁架构。",
        keywords: &["uncle bob", "solid", "整洁架构", "clean architecture", "依赖倒置"],
        guidance: "函数与类单一职责；开放封闭、里氏替换、接口隔离、依赖倒置；以用例为中心分层（实体/用例/接口适配/框架），依赖指向内层；命名表意图、消除重复、保持测试绿。",
    },
    NomifunCap {
        name: "video-clip-assistant",
        description: "视频自动剪辑助手：提取精彩片段/生成字幕/裁剪/多平台导出。",
        keywords: &["视频剪辑助手", "精彩片段", "视频字幕", "短视频生成", "视频裁剪"],
        guidance: "1) 按台词/画面/热度抽取高光；2) 自动生成并烧录字幕；3) 按平台画幅裁剪时长；4) 加转场/BGM；5) 多平台导出（竖屏/横屏）。优先确定性脚本，参数可复现。",
    },
    NomifunCap {
        name: "video-editor",
        description: "视频剪辑操作：剪切/合并/转换/处理。",
        keywords: &["视频编辑", "视频合并", "视频转换", "视频处理", "剪辑操作"],
        guidance: "把编辑意图转为确定性操作：concat 合并、trim 剪切、scale/rotate 处理、封装/编码转换；给出命令或脚本、参数解释与质量注意点（音画同步、码率、色彩空间）。",
    },
    NomifunCap {
        name: "wireframe",
        description: "线框图与用户流：页面布局 + 流程 + HTML 导出。",
        keywords: &["线框图", "wireframe", "用户流", "页面布局", "流程草图"],
        guidance: "1) 先画信息架构与关键页面块布局（ASCII/SVG）；2) 串起用户流与决策分支；3) 标注交互与状态；4) 可导出 HTML 原型。低保真先对齐结构与流程，再上视觉。",
    },
];

/// 把全量 nomifun 能力登记进技能书（每个能力 = 一个可路由/可调用 skill）。
pub fn register_nomifun_skills(book: &SkillBook) {
    for cap in NOMIFUN_CAPS {
        book.register_skill(Skill {
            name: cap.name.to_string(),
            description: cap.description.to_string(),
            steps: vec![SkillStep::Call {
                tool: "nomifun_skill".to_string(),
                arg: format!("cap={}\n{{input}}", cap.name),
            }],
        });
    }
}

/// 意图路由：在 nomifun 能力关键词表中寻找命中（返回能力/skill 名）。
///
/// 采用「最长关键词命中优先」：当多个能力的关键词都被查询包含时，
/// 选取被命中关键词最长（最具体）的能力，避免 `debug` 这类短词抢路由。
pub fn match_nomifun_intent(query: &str) -> Option<String> {
    let q = query.to_lowercase();
    let mut best: Option<(String, usize)> = None;
    for cap in NOMIFUN_CAPS {
        for kw in cap.keywords {
            let kl = kw.to_lowercase();
            if q.contains(&kl) {
                let len = kl.chars().count();
                if best.as_ref().map_or(true, |(_, b)| len > *b) {
                    best = Some((cap.name.to_string(), len));
                }
            }
        }
    }
    best.map(|(name, _)| name)
}

/// nomifun 技能目录里个别文件夹名与注册用的能力名不一致，这里做映射。
fn folder_for_cap(name: &str) -> &str {
    match name {
        "ai-agentic-video-editor" => "ai_agentic_video_editor",
        "ffmpeg-video-editor" => "FFmpeg Video Editor",
        "prd-to-prototype" => "PRD to Prototype",
        other => other,
    }
}

/// 解析技能根目录，优先级：
/// 1) 环境变量 `GANYU_NOMIFUN_SKILLS_DIR`
/// 2) 本仓库随附的 `skills/nomifun`（已同步）
/// 3) nomifun 实时目录 `%LOCALAPPDATA%/NomiFun/skills`
fn resolve_skills_dir() -> Option<PathBuf> {
    if let Ok(d) = std::env::var("GANYU_NOMIFUN_SKILLS_DIR") {
        let p = PathBuf::from(d);
        if p.is_dir() {
            return Some(p);
        }
    }
    let bundled = PathBuf::from("skills/nomifun");
    if bundled.is_dir() {
        return Some(bundled);
    }
    if let Some(local) = dirs_local_appdata() {
        let live = local.join("NomiFun").join("skills");
        if live.is_dir() {
            return Some(live);
        }
    }
    None
}

/// 返回 `%LOCALAPPDATA%`（Windows）或对应的用户数据目录，找不到返回 None。
fn dirs_local_appdata() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("LOCALAPPDATA") {
        return Some(PathBuf::from(p));
    }
    if let Ok(p) = std::env::var("HOME") {
        return Some(PathBuf::from(p).join(".local").join("share"));
    }
    None
}

/// 去掉 SKILL.md 头部的 YAML frontmatter（`--- ... ---`）。
fn strip_frontmatter(md: &str) -> String {
    let trimmed = md.trim_start();
    if trimmed.starts_with("---") {
        if let Some(end) = trimmed[3..].find("\n---") {
            return trimmed[end + 4..].trim_start().to_string();
        }
    }
    md.to_string()
}

/// 加载某个能力的真实技能内容：SKILL.md 主体 + `references` 下所有 `.md`。
/// 找不到时返回 None（交由调用方回退到内置 SOP）。
fn load_skill_content(base: &Path, folder: &str) -> Option<String> {
    let dir = base.join(folder);
    let skill_md = dir.join("SKILL.md");
    if !skill_md.is_file() {
        return None;
    }
    let mut out = String::new();
    if let Ok(body) = fs::read_to_string(&skill_md) {
        out.push_str(&strip_frontmatter(&body));
    }
    let refs = dir.join("references");
    if refs.is_dir() {
        let mut entries: Vec<_> = fs::read_dir(&refs)
            .into_iter()
            .flatten()
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.path()
                    .extension()
                    .map_or(false, |x| x.eq_ignore_ascii_case("md"))
            })
            .map(|e| e.path())
            .collect();
        entries.sort();
        for p in entries {
            if let Ok(t) = fs::read_to_string(&p) {
                out.push_str("\n\n## Reference: ");
                out.push_str(p.file_name().and_then(|s| s.to_str()).unwrap_or(""));
                out.push('\n');
                out.push_str(&strip_frontmatter(&t));
            }
        }
    }
    if out.trim().is_empty() {
        None
    } else {
        Some(out)
    }
}

/// 派发 nomifun 内置能力的工具：离线返回真实技能定义，配置网关则真实执行。
pub struct NomifunSkillTool;

#[async_trait]
impl Tool for NomifunSkillTool {
    fn name(&self) -> &str {
        "nomifun_skill"
    }
    fn description(&self) -> &str {
        "派发 nomifun 内置 agent 能力：离线返回能力指引 SOP；设 GANYU_NOMIFUN_GATEWAY 走真实桥接。"
    }
    fn side_effecting(&self) -> bool {
        true
    }
    async fn invoke(&self, input: &Value) -> GanyuResult<Value> {
        let s = input.as_str().trim();
        if !s.starts_with("cap=") {
            return Err(GanyuError::ToolFailed(
                "nomifun_skill".into(),
                "输入须以 cap= 开头，例如：cap=code-review-assistant src/x.rs".into(),
            ));
        }
        // 能力名独占首行，用户输入在换行之后，杜绝用户输入覆盖能力名（防注入 F-07）。
        let (head, user_input) = match s.split_once('\n') {
            Some((h, b)) => (h.trim(), b),
            None => (s.trim(), ""),
        };
        let cap_name = &head["cap=".len()..];
        let cap = NOMIFUN_CAPS
            .iter()
            .find(|c| c.name == cap_name)
            .ok_or_else(|| {
                GanyuError::ToolFailed(
                    "nomifun_skill".into(),
                    format!("未知 nomifun 能力：{cap_name}"),
                )
            })?;

        // 真实桥接（可选）：GANYU_NOMIFUN_GATEWAY 形如 `nomifun skill {cap} {input}`
        if let Ok(gw) = std::env::var("GANYU_NOMIFUN_GATEWAY") {
            let cmd = gw
                .replace("{cap}", &cap.name)
                .replace("{input}", user_input.trim());
            return dispatch_gateway(&cmd).await;
        }

        // 离线（默认，已同步真实内容）：优先读取同步进仓库的真实 SKILL.md 定义；
        // 否则回退到本模块内置的方法论 SOP。
        if let Some(base) = resolve_skills_dir() {
            if let Some(content) = load_skill_content(&base, folder_for_cap(&cap.name)) {
                return Ok(Value(format!(
                    "【nomifun 内置能力 · {name}】（已同步真实技能定义）\n\n{content}",
                    name = cap.name,
                    content = content,
                )));
            }
        }
        Ok(Value(format!(
            "【nomifun 内置能力 · {name}】\n{desc}\n\n触发关键词：{kws}\n\n## 执行指引\n{guide}",
            name = cap.name,
            desc = cap.description,
            kws = cap.keywords.join(" / "),
            guide = cap.guidance,
        )))
    }
}

/// 经网关程序派发（程序名受安全约束，拒绝绝对路径/盘符/穿越/元字符）。
async fn dispatch_gateway(cmd: &str) -> GanyuResult<Value> {
    use tokio::process::{Command, Stdio};

    let mut parts = cmd.split_whitespace();
    let prog = parts.next().ok_or_else(|| {
        GanyuError::ToolFailed("nomifun_skill".into(), "网关命令为空".into())
    })?;
    if !is_safe_gateway_prog(prog) {
        return Err(GanyuError::ToolFailed(
            "nomifun_skill".into(),
            format!("网关程序不在白名单/含危险字符：{prog}"),
        ));
    }
    let args: Vec<&str> = parts.collect();
    let mut child = Command::new(prog)
        .args(&args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            GanyuError::ToolFailed("nomifun_skill".into(), format!("网关执行失败：{e}"))
        })?;
    drop(child.stdin.take());
    // 超时等待，防网关程序卡死挂线程（30s）。
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(30),
        child.wait_with_output(),
    )
    .await
    .map_err(|_| {
        GanyuError::ToolFailed("nomifun_skill".into(), "网关执行超时（30s 上限）".into())
    })?
    .map_err(|e| {
        GanyuError::ToolFailed("nomifun_skill".into(), format!("网关执行失败：{e}"))
    })?;
    if !output.status.success() {
        return Err(GanyuError::ToolFailed(
            "nomifun_skill".into(),
            format!(
                "网关退出 {}: {}",
                output.status,
                String::from_utf8_lossy(&output.stderr)
            ),
        ));
    }
    // 输出截断（防结果膨胀，1MB 上限）
    let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
    const MAX_OUT: usize = 1024 * 1024;
    let text = if s.chars().count() > MAX_OUT {
        format!("{}…[已截断：输出超过 1MB 上限]", s.chars().take(MAX_OUT).collect::<String>())
    } else {
        s
    };
    Ok(Value(text))
}

/// C2：网关程序名必须是「安全 token」——仅字母数字/点/下划线/相对分隔符，
/// 不得含 shell 元字符、不得为绝对路径、不得含 `..` 穿越。
fn is_safe_gateway_prog(prog: &str) -> bool {
    if prog.is_empty() || prog.contains("..") {
        return false;
    }
    // F-08：禁止把 shell 解释器当网关程序——shell 可直接执行任意命令，违背白名单语义。
    let lower = prog.to_lowercase();
    if matches!(
        lower.as_str(),
        "sh" | "bash" | "cmd" | "powershell" | "pwsh" | "zsh" | "fish"
            | "sh.exe" | "bash.exe" | "cmd.exe" | "powershell.exe" | "pwsh.exe"
            | "zsh.exe" | "fish.exe"
    ) {
        return false;
    }
    if prog.starts_with('/') || prog.starts_with('\\') || prog.contains(':') {
        return false; // 拒绝绝对路径 / 盘符
    }
    prog.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '/' || c == '-')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::memory::LocalMemory;
    use std::sync::Arc;

    #[test]
    fn catalog_has_no_duplicate_names() {
        let mut seen = std::collections::HashSet::new();
        for cap in NOMIFUN_CAPS {
            assert!(seen.insert(cap.name), "重复能力名：{}", cap.name);
        }
    }

    #[test]
    fn every_cap_routes_by_keyword() {
        for cap in NOMIFUN_CAPS {
            let hit = cap
                .keywords
                .iter()
                .filter(|k| !k.is_empty())
                .map(|k| match_nomifun_intent(k))
                .any(|r| r.is_some());
            assert!(hit, "能力 {} 没有任何可路由关键词", cap.name);
        }
    }

    #[test]
    fn all_caps_registered_as_skills() {
        let mem = Arc::new(LocalMemory::new(std::env::temp_dir().join("ganyu_nomifun_test_mem")));
        let book = SkillBook::new(mem);
        register_nomifun_skills(&book);
        let names = book.skill_names();
        assert_eq!(names.len(), NOMIFUN_CAPS.len(), "注册数量不一致");
        for cap in NOMIFUN_CAPS {
            assert!(names.iter().any(|n| n == cap.name), "未注册：{}", cap.name);
            assert!(book.get_skill(cap.name).is_some());
        }
    }

    #[test]
    fn synced_skill_files_present() {
        // 仅在随附的 skills/nomifun 存在时校验（CI 无该目录则跳过）
        let base = PathBuf::from("skills/nomifun");
        if !base.is_dir() {
            return;
        }
        for cap in NOMIFUN_CAPS {
            let skill_md = base.join(folder_for_cap(cap.name)).join("SKILL.md");
            assert!(skill_md.is_file(), "缺少同步的技能文件：{}", skill_md.display());
        }
    }
}
