# ganyu-agent 全面工程保障审计报告

**日期**：2026-08-18
**工作流**：综合（代码审查 + 架构评估 + 部署/事故响应 + 测试覆盖 + 文档与工程化）
**参与成员**：Cody（代码审查师）、Archi（系统架构师）、Rex（SRE 工程师）、Tessa（测试专家）、Docu（技术文档师）
**被审计系统**：`ganyu-agent` v0.1.1（Rust, edition 2021），`src` 下 34 个 `.rs`、约 7.6k LOC，位于 `d:\workbuddy_all\harness_all\ganyu-agent`

---

## 📌 TL;DR（执行摘要）

- **整体评级**：🟡 有条件通过（代码与架构健康度 B+；但运行时事故响应与 CI 持续门禁薄弱，且你要求的"上传仓库初始化"能力在代码中不存在）。
- **严重度分布**：代码层 🔴0 / 🟠4 / 🟡~25 / 🟢~6；运维就绪度另有 **4 项 🔴 缺口**（无进程守护 / 无可观测性底座 / 升级无 `.old` 备份且原地覆盖 / 审计日志无轮转）。
- **关键结论**：
  1. 安全基件扎实——失败闭环（fail-closed）贯穿文件沙箱、SSRF、shell 开关、密钥权限、ed25519 供应链验签、记忆加密；**无 🔴 级代码缺陷**。
  2. 性能热点明确：`OpenVikingMemory` 每次调用新建 `reqwest::Client`（#1）、`LocalMemory` 每次 `commit` 全量重写文件 + 阻塞 IO（#3/#7）、`breakers.get(&name).unwrap()` 并发 panic 风险（#2）。
  3. 部署供应链安全达标（CI 签名 fail-closed、安装脚本硬编码公钥验签、权限收紧）；但**运行时事故响应薄弱**——无监控/告警、无 runbook/SEV 框架、`panic=abort` 且无 supervisor，崩溃即宕机。
  4. **"上传仓库初始化"能力在代码中不存在**（5 位专家一致确认）；最接近的"自更新/发布资产提取"已实现，但内嵌 `main.rs` 上帝模块、提取非原子（U1）。需你确认该能力是未实现 / 改名 / 还是审计范围误标。
  5. 测试覆盖约 75%，但 R-6 黑板 256KiB 上限、C1 shell 开关、M5 输出净化等刚落地的 fail-closed 边界**零验证**。

---

## 🎯 核心结论卡片

| 项目 | 内容 |
|------|------|
| 整体评级 | 🟡 有条件通过（B+） |
| 阻塞项数量 | 代码级阻塞 0；运维就绪度 🔴 缺口 4；文档矛盾（P0）1；"上传仓库初始化"范围待澄清 1 |
| 关键行动项 | 8 条（P0×3 + P1×5，详见行动清单） |
| 建议下一步 | 先修 P0（hardened 补 sandbox / R-6·C1·M5 补测 / SECURITY-REPORT §0·§1 矛盾），再排 P1 性能·部署·架构重构 |

---

## 一、代码审查发现（Cody，按严重度排序）

> 总体评级 **高（B+）**：安全基件为最亮眼部分，库代码生产路径规避了 panic 风险 `unwrap`，并发锁序一致、无死锁，工作流均有步数/轮次上限与 DAG 环检测。

| # | 严重度 | 类别 | 文件:行 | 问题摘要 |
|---|--------|------|---------|---------|
| 1 | 🟠高 | 性能 | `core/memory.rs:399-404` | `OpenVikingMemory::client()` 每次 HTTP 调用都新建 `reqwest::Client`，连接池/TLS 复用全失效 |
| 2 | 🟠高 | 正确性 | `routing/mod.rs:154` | `breakers.get(&name).unwrap()` 在 `hot_reload` 并发时存在潜在 panic（不变量被临时破坏） |
| 3 | 🟠高 | 性能 | `core/memory.rs:98-133` | `LocalMemory` 每次 `put`/`commit` 全量重写整个 JSON 文件（O(n) 序列化+阻塞写），`commit` 每次 `Agent::run` 都触发 |
| 4 | 🟠高 | 正确性 | `main.rs:703,716` | 自更新解包 `tmp.to_str().unwrap()` 在临时路径含非 UTF-8 时 panic（应用退出而非优雅失败） |
| 5 | 🟡中 | 性能 | `knowledge/mdl.rs:127-140` | `detect_injection` 每次调用现编译 18 条正则（无缓存） |
| 6 | 🟡中 | 性能 | `security.rs:233` | `ssrf_guard_resolve` 在 async `web_fetch` 内同步阻塞 DNS 解析（`to_socket_addrs`） |
| 7 | 🟡中 | 性能 | `core/memory.rs:127-131` | `save()` 在 async 上下文用 `std::fs::write/rename` 阻塞 worker 线程 |
| 8 | 🟡中 | 正确性 | `core/memory.rs:269` | `encrypt()` `.unwrap_or_default()` 在 GCM 失败时写出空 `ENC2:` 损坏文件（概率极低） |
| 9 | 🟡中 | 安全 | `ext/mod.rs:435` | `is_safe_program` 允许程序名含 `/`，可放行相对路径插件二进制（绕开 PATH 解析） |
| 10 | 🟡中 | 安全一致性 | `ext/mod.rs:251-290` | `CommandTool`（插件）未套用 `exec` 已具备的 Landlock FS 沙箱 |
| 11 | 🟡中 | 正确性 | `routing/mod.rs:130` | LLM 缓存键 `serde_json::to_string(messages).unwrap_or_default()` 失败变常量空键 → 跨请求缓存互相污染 |
| 12 | 🟡中 | 性能 | `core/unit.rs:70` | `board_set` 每次写入都 `format!("{v}").len()` 遍历全量条目求字节和 |
| 13 | 🟡中 | 安全 | `security.rs:36-39` | Windows 下 `USERNAME` 为空时 `restrict_file_permissions` 返回 false（权限未收紧，fail-soft） |
| 14 | 🟡中 | 可维护性 | 多文件 | 硬限制/超时等魔法值散落（256KiB/1MB/10MB/1_000_000/MAX_STEPS=8/MAX_PLAN_STEPS=20/KDF_ROUNDS） |
| 15 | 🟡中 | 可维护性 | `security.rs` vs `core/memory.rs` | hex 编解码两份实现（`decode_hex` vs `hex_encode/hex_decode`） |
| 16 | 🟢低 | 可维护性 | `config.rs` | `read/write_model_config/gateway_token` 反复内联声明同构结构体 |
| 17 | 🟢低 | 正确性(文案) | `core/workflow/graph.rs:75,204` | 错误信息"或孤立节点"不准确——孤立节点实际被允许执行 |
| 18 | 🟢低 | 性能 | `cache.rs` | `LruCache` `order` 用 `Vec` 线性 `position()` 查找，小容量 OK，容量放大时需留意 |

**安全正向确认**：文件沙箱 `resolve_sandboxed`、SSRF `ssrf_guard_resolve`（含 `resolve(host, ip:0)` 连接层钉选，经 reqwest 0.12 官方文档核实为正确写法）、shell 双层 fail-closed、`restrict_file_permissions`（unix 0600 / Windows `icacls`）、ed25519 自更新验签（R-1）、AES-256-GCM 记忆加密均实现到位。并发安全性整体良好（锁序一致，无死锁）。

---

## 二、架构评估与技术债（Archi）

> 健康度 **良好（B+）**：分层清晰、抽象到位（Unit×RunContext×Workflow 统一底座）、安全为"失败闭环 + 纵深防御"、自愈体系完整。主要短板：① `main.rs` 1248 行上帝模块；② `core↔ext` 双向耦合；③ 工作流单元被硬编码为离线 `LocalReasoner`；④ `hardened` 特性组合漏了 `sandbox`。

### 模块依赖图（节选）
```
main.rs(1248 行, 上帝模块: CLI/Telegram/自更新/selftest)
   └─ 编排/扩展层: core::agent · core::loop_ · core::workflow(×6) · ext(ToolRegistry/SkillBook) · knowledge(mdl/sag) · routing(Gateway) · persona
        └─ 基础层: core::unit · core::llm · core::memory · heal · observe · cache · security · config · session · value · error · sandbox
⚠️ core↔ext 双向依赖（分层倒置，债 A2）
```

### 技术债清单（Priority = (Impact+Risk)×(6−Effort)，各 1–5）

| 排名 | ID | 债务 | I | R | E | Priority |
|---|---|---|---|---|---|---|
| 1 | F1 | `hardened` 组合漏 `sandbox`：听起来"全加固"，Linux 下 exec 实际未套 Landlock | 3 | 3 | 1 | **30** |
| 2 | A3 | 工作流单元硬编码 `LocalReasoner`，多范式编排无法用联网模型 | 3 | 2 | 2 | 20 |
| 3 | M1 | `is_safe_program` 与 `is_safe_gateway_prog` 重复安全校验逻辑 | 2 | 2 | 1 | 20 |
| 4 | A2 | `core ↔ ext` 双向耦合 / 分层倒置 | 3 | 3 | 3 | 18 |
| 5 | U1 | 上传仓库初始化/自更新内嵌 `main.rs`，无独立模块/状态机，提取非原子 | 3 | 3 | 3 | 18 |
| 6 | M2 | `memory.rs`(625 行) 含 LocalMemory+Cipher+OpenVikingMemory，宜拆 crypto 子模块 | 2 | 2 | 2 | 16 |
| 7 | M3 | 意图路由表分裂(`skills.rs` + `nomifun_caps`)且 `SkillBook` 直接依赖 nomifun | 2 | 2 | 2 | 16 |
| 8 | M4 | `OpenVikingMemory` 网络代理(`OV_BASE`)是待激活分支，默认永不走，标记 dead_code | 2 | 2 | 2 | 16 |
| 9 | F2 | crypto 默认开但运行时仍需 `GANYU_MEM_KEY`；无 key 时记忆明文落盘 | 2 | 2 | 2 | 16 |
| 10 | D1 | 自更新/安装依赖宿主 `tar`/`certutil`/`sha256sum` 外部二进制，非自包含 | 2 | 3 | 3 | 15 |
| 11 | A1 | `main.rs` 1248 行上帝模块（CLI/IO/网络/安全/网关/自更新混杂） | 4 | 3 | 4 | 14 |
| 12 | A4 | `build_workflow(&ctx…)` 的 `ctx` 参数未被使用（签名残留） | 1 | 1 | 1 | 10 |

**特性门控评估**：`default=[crypto,secret]` 正确（消除"忘开特性就编译掉安全能力"的裸奔）；`network/shell/sandbox` 默认关闭、显式 opt-in，符合失败闭环。残留风险两处：① **F1（一行修）** `hardened` 漏 `sandbox`；② **F2** `crypto` 默认开 ≠ 运行时强制加密（仍需 `GANYU_MEM_KEY`）。两者都是"编译期安全已默认、运行期仍需显式配置"的语义落差，应在文档/`doctor` 显式告知。

---

## 三、部署就绪度与事故响应（Rex）

> 结论：**"部署供应链安全"具备，"运行时事故响应"薄弱**。

### 部署检查清单（Go / No-Go 摘要）
- ✅ Go：install 幂等、文件权限仅属主、供应链强校验（硬编码公钥验签）、发布产物齐备+签名、安装后 selftest。
- ⚠️ No-Go(部分)：升级无 `.old` 备份、解包**原地覆盖** binDir（中断/损坏可能留不可用二进制或删掉良好 exe）；Unix **不持久化 PATH**（仅打印 export 提示）；卸载残留 Windows 用户 PATH 项。
- ⚠️ No-Go(缺口)：CI 资产平台仅 `win-x86_64/linux-x86_64/macos-arm64`，缺 `win-arm64/linux-arm64/macos-x86_64` → 这些平台 install/update 报 asset not found。
- ⚠️ 风险：无 systemd/nssm 进程守护；发布即公开无灰度，回滚靠手动 `GANYU_VERSION=旧tag` 重装。

### 事故响应演练（模拟）
- **场景 A（自更新签名校验失败）**：正常 SEV-3（fail-closed 已阻断）；若确认 CI 私钥泄漏 → SEV-1（全用户可被投毒）。根因 5Why 指向"单密钥、无自动轮换、无阈值/多签"，恢复靠密钥吊销+轮换+重签+公告。
- **场景 B（panic=abort 崩溃）**：单次 SEV-2（单服务不可用）；若记忆损坏致启动即崩且无人值守 → SEV-1 风险。根因 5Why 指向"无全局 panic hook 记录、无 supervisor 重启、无启动健康门禁"。

### 运维真实风险（按严重度）
- 🔴【M】无进程守护/自动重启：`panic=abort` + 无 supervisor → 长驻服务崩溃后需人工介入。
- 🔴【M】无可观测性底座：`GANYU_AUDIT` 默认 OFF，无 metrics/health/alert → 无法主动发现宕机/错误率升高。
- 🔴【M】升级无 `.old` 备份 + 解包原地覆盖 → 中断/损坏更新可能留不可用二进制。
- 🔴【M】审计日志无轮转：`GANYU_AUDIT=file` 仅 append，磁盘无界增长。
- 🟡【L】CI 资产平台覆盖缺口；Unix PATH 不持久化；磁盘无界（`.build-cache`/记忆 JSON/审计）；配置热更新未激活（`Gateway::hot_reload` 无信号触发）；卸载残留 Windows PATH 项。

---

## 四、测试覆盖分析（Tessa）

> 覆盖健康度 **中上（≈75%）**：安全基件、缓存、工作流编排、记忆加密、配置、路由、自愈主体均已覆盖，且 R-1~R-9 加固配套了关键单测。主要薄弱点在"刚落地的加固点缺少针对性验证"。

### 覆盖矩阵（节选）
| 能力 | 状态 | 缺口 |
|---|---|---|
| resolve_sandboxed | 🟡部分 | 缺 NUL 拒绝、符号链接逃逸分支 |
| ssrf_guard / ed25519(R-1) | ✅已覆盖 | 充分 |
| shell_allowed(C1) | ❌缺失 | 关键 fail-closed 开关零断言 |
| restrict_file_permissions(R-8/R-9) | 🟡部分 | Windows `icacls` 分支未测；config 落盘权限未断言 |
| 缓存 LRU/TTL | ✅已覆盖 | 充分 |
| 黑板 256KiB 上限(R-6) | 🟡部分 | **完全未测**（仅断言 key 存在） |
| 工作流(graph/multi/plan/router/single) | ✅已覆盖 | 充分 |
| 记忆加密 save/load | ✅已覆盖 | 充分 |
| 配置(写入后权限/gateway token) | 🟡部分 | 落盘权限未直接断言；security_baseline/ensure_config_template 未测 |
| 上传仓库初始化 | N/A 未落地 | src 无实现 |
| 自愈 heal | 🟡部分 | `with_retry_async`/`RateLimiter` 未直接单测 |
| 知识 SAG | 🟡部分 | 内部逻辑(`parse_intent`/`template_sql`/非 SQL 降级)未单测 |
| routing/sandbox | 🟡部分 | `sanitize_model_output`(M5) 单测缺失；Landlock 真实拒绝未验 |

### 缺失测试优先级
- **P0（安全）**：① `board_set` 256KiB 上限 fail-soft（R-6）；② `shell_allowed()`（C1）双层 fail-closed；③ `sanitize_model_output()`（M5）出口信任边界；④ `resolve_sandboxed` 的 NUL/符号链接分支。
- **P1**：`restrict_file_permissions` Windows 分支 + config 落盘权限断言；`config::security_baseline`/`ensure_config_template`；`main.rs` 自更新编排路径；SAG 内部逻辑。
- **P2**：`with_retry_async`、`RateLimiter`、`ext::discover`/`is_safe_program`、`Gateway::hot_reload`、Landlock 真实拒绝（linux+sandbox）。

### CI 状态建议
1. 设 `cargo test` 必须在 CI 通过（矩阵跑 `default` 与 `--features hardened`，Linux 额外 `--features hardened,sandbox`）。
2. 清理现有告警：`workflows.rs:42/65`（多余 `mut`）、`workflows.rs:220`（未被使用的 `let c`）、`integration.rs:19/52`（多余 `mut`）——否则 `RUSTFLAGS="-D warnings"` 会阻断合理新代码。
3. 测试产物污染：多测试在 CWD 写 `.ganyu_*.json`，建议统一到临时目录或 `.gitignore` 兜底。

---

## 五、文档与工程化（Docu）

> 健康度：文档整体良好偏上（安全文档 F-01~F-14、R-1~R-9 尤为详尽、可验证）；工程化中等偏上——**发布与供应链强，持续集成与规范门槛弱**。

### 文档债务（按优先级）
- **P0-1**：`SECURITY-REPORT.md` 摘要(§0)与 STRIDE(§1)仍写"R-6/R-8/R-9 接受残余"，但 §3/§4.5 已记"已第三阶段加固闭环"——**自相矛盾，直接误导安全态势判断**（修复成本极低，改 §0/§1 两处即可）。
- **P0-2**："上传仓库初始化"能力文档暴露度 = 0（代码中亦无实现入口）。
- **P1-3**：README「目录」漏列 `skills/`（33 项 nomifun 能力主打特性）。
- **P1-4**：`persona/` 模块未进入架构文档（实际是"人格/共情 prompt 构造器"）。
- **P1-5**：公钥轮换 runbook 漏列嵌入点（`SECURITY.md` 与 `main.rs` 互操作向量），轮换时若不更新这两处会出现不一致。
- **P1-6**：`security_fixes.md` 测试计数不严谨（"55" 实际来自 `cargo test --features hardened`，R-1 签名互操作测试被 `network` 门控，默认特性不编译）。
- **P2-7**：缺 `CONTRIBUTING.md` / `CHANGELOG.md`。
- **P2-8**：`config-guide.md` 模板 D 引用不存在的 `ganyu-agent:latest` 镜像（仓库无 Dockerfile）。
- **P2-9**：release 矩阵缺 `windows-arm64`/`macos-x86_64`（与 SRE 一致）。
- **P2-10**：无 `rust-toolchain`（MSRV 未钉），CI 用 `dtolnay/rust-toolchain@stable`，可复现性弱。

### 工程化成熟度评分
| 维度 | 评分 | 扣分点 |
|------|------|--------|
| 构建 | 4/5 | release profile 齐全且文档化；无 MSRV 钉选 |
| 测试 | 3/5 | 有测试但无 PR/merge 门禁，main 可静默回归 |
| 发布 | 4.5/5 | 三平台+sha256+ed25519 签名+cargo audit；无 CHANGELOG/Docker |
| 规范 | 2/5 | 无 rustfmt.toml/clippy.toml、CI 无 fmt/clippy 门槛、无 PR 门禁、无 CONTRIBUTING/CHANGELOG |
| **总体** | **≈3.4/5** | 发布/供应链强，CI 与规范门槛弱 |

### 结构化目录 / Onboarding
- 优点：`src/` 划分清晰、命名自解释；onboarding 资源充足（README 快速开始、architecture、config-guide、8 篇 ADR、SECURITY 12 层防线）。
- 建议：① 架构文档补 `persona/`；② README 补 `skills/`（并注明"随附能力集，非核心代码"）；③ 顶层补 `CONTRIBUTING.md`/`CHANGELOG.md`；④ 加"5 分钟写一个插件"可运行样例入口。

---

## 六、跨切面专题：上传仓库初始化处理盘点

> **结论（5 位专家一致）**：当前代码库中**不存在"上传仓库初始化（upload / repo init / git init / push）"这一能力**。

证据链：
- **Cody**：全仓库检索 `upload|上传|init_repo|git init|git_init` 仅命中 docs/skills 的"视频资源上传""CI upload-artifact"与 `main.rs` 自更新、`agent-git-oracle`（只读仓库分析）——均非本能力；`src` 内无 git init/clone/上传提交逻辑、无状态机/幂等/回滚实现、无单测。
- **Rex / Tessa / Docu**：install 脚本仅有 `git clone`（安装期）+ CI 的 `git tag/push`；`src` 下零命中；文档暴露度 0；main.rs CLI 子命令无"仓库上传/初始化"子命令。
- **Archi**：将"上传仓库初始化"对应到自更新/发布资产提取（`main.rs:557–753` + `install.sh/ps1`），该能力已实现但内嵌上帝模块、提取非原子（U1）。

**最贴近的实现 = 自更新管道（下载→验签→sha→提取）**，非"上传/初始化仓库"。因此你要求的"处理盘点"有两种可能解读：
- (a) 若指**自更新/发布资产提取**：已实现但需重构（抽 `src/update.rs` 状态机 + staging 原子替换，见 U1/A1）。
- (b) 若指**git 仓库的初始化与上传**：当前**未实现**，属范围缺口。

**若该能力后续要落地，应强制满足的可靠性清单**（供实现者参考，Rex/Cody 共识）：
1. 幂等：重复 init 不破坏已有权限/数据；
2. 崩溃安全重试：中途失败可安全重跑，不留半初始化仓库目录（参考 install.sh `trap 'rm -rf "$TMP"' EXIT`）；
3. 资源泄漏防护：失败路径清理临时 clone/staging；
4. 并发初始化安全：文件锁（flock）防两进程互踩；
5. 回滚：失败/中止 → 本地 rm + 远端 repo 删除（或延迟到 push 才建），不留孤儿远端仓库；
6. 安全复用：目标过 `resolve_sandboxed`、URL 过 `ssrf_guard_resolve`、`git` 子进程走 shell 双层门控 + Landlock、凭据临时 env 注入（禁写全局 env）、写入后 `restrict_file_permissions` 收紧。

**需你确认**：该能力是（a）尚未排期、（b）以其他命名存在、还是（c）审计范围误标——决定纳入 roadmap 还是关闭此项。

---

## ✅ 行动清单（按优先级排序）

| # | 行动 | 负责角色 | 紧急度 | 预期完成 |
|---|------|---------|--------|---------|
| 1 | [F1] `hardened` 组合补 `sandbox`（Cargo.toml:70 加 `sandbox`）—— 一行消除 Linux exec 未隔离残留裸奔 | Archi | **P0** | 即时 |
| 2 | [测试 P0] 补 R-6 黑板 256KiB 上限 / C1 `shell_allowed` / M5 `sanitize_model_output` 三项 fail-closed 边界单测 | Tessa | **P0** | 本周 |
| 3 | [文档 P0-1] 修 `SECURITY-REPORT.md` §0/§1 与 §3/§4.5 对 R-6/R-8/R-9 的矛盾表述 | Docu | **P0** | 即时 |
| 4 | [代码 #1] `OpenVikingMemory::client()` 复用单例 `reqwest::Client`（core/memory.rs:399-404） | Cody | P1 | 本周 |
| 5 | [代码 #3/#7] `LocalMemory` 改分文件/追加/debounce + `tokio::fs`（core/memory.rs:98-133,127-131） | Cody | P1 | 两周 |
| 6 | [代码 #2] `breakers.get(&name).unwrap()` 改 `if let`（routing/mod.rs:154）消除 hot_reload 并发 panic | Cody | P1 | 本周 |
| 7 | [SRE] 进程守护(systemd/nssm)+健康门禁(selftest)+panic 日志化(set_hook) | Rex | P1 | 两周 |
| 8 | [SRE] 基础可观测性 + SEV 框架/incident runbook | Rex | P1 | 一个月 |
| 9 | [架构 U1/A1] 自更新抽 `src/update.rs` 状态机 + staging 原子替换，拆分上帝模块 | Archi | P1 | 一个月 |
| 10 | [SRE/Docu] 升级保留 `.old`+原子 swap；补齐 CI 平台资产；密钥轮换 runbook 补全 6 处嵌入点 | Rex/Docu | P1 | 两周 |
| 11 | [Docu] CI 加 PR 门禁(build+test+fmt --check+clippy -D warnings) + 清理 tests/ 告警 | Docu | P1 | 两周 |
| 12 | [代码 #9/#10] `is_safe_program` 禁 `/`（强制 PATH 解析）+ `CommandTool` 套 Landlock（ext/mod.rs:435,251-290） | Cody | P1 | 两周 |
| 13 | [架构 A3] 工作流注入 `LlmReasoner`，多范式真正用联网模型（main.rs:1044） | Archi | P2 | 排期 |
| 14 | [架构 M1] 合并 `is_safe_program`/`is_safe_gateway_prog` → `security::is_safe_command` | Archi | P2 | 排期 |
| 15 | [架构 A2/M2/M3] 解 core↔ext 耦合 / 拆 memory.rs crypto 子模块 / 合并意图路由表 | Archi | P2 | 排期 |
| 16 | [代码 #13] Windows `USERNAME` 空回退 `whoami`/SID（security.rs:36-39） | Cody | P2 | 排期 |
| 17 | [代码 #14/#15] 魔法值集中 `consts`；合并 hex 双实现 | Cody | P2 | 排期 |
| 18 | [测试 P1/P2] SAG 内部 / RateLimiter / with_retry_async / hot_reload / Landlock 真实拒绝 / config 落盘权限断言 | Tessa | P2 | 排期 |
| 19 | [Docu] `persona/` 入架构文档、README 补 `skills/`、加 CONTRIBUTING/CHANGELOG、修 config-guide 模板 D | Docu | P2 | 排期 |
| 20 | [用户确认] "上传仓库初始化"能力范围澄清（未实现/改名/误标） | 用户 | **P0(需 input)** | 待回复 |

---

## ⚠️ 待完善 / 已知局限

- 本次为**静态审查**：受沙箱限制未跑运行时测试（`cargo check --tests --features hardened` 已验证 RC=0）；性能结论基于代码路径分析，未做 benchmark 实测。
- Windows `icacls` 分支与 `restrict_file_permissions` 的 Windows 路径需在 Windows 环境实跑验证（SRE/Tessa 均指出）。
- 架构师技术债 Priority 为启发式公式估算（(Impact+Risk)×(6−Effort)），仅作排序参考，非绝对量化。
- 各成员未修改任何文件，结论均基于只读探查；唯一需用户拍板的是"上传仓库初始化"范围（行动 #20）。
- 上一阶段提交的 `57eeb67` 已把 R-6/R-8/R-9 记入 §3/§4.5，但 Docu 发现 §0/§1 摘要仍写"接受残余"——属文档一致性遗漏，已列入行动 #3。

---

## 📚 数据来源 & 成员产出索引

- **Cody（代码审查师）** 原始产出：完整中文代码审查报告（Top-18 发现表 + 分维度详述 + upload-repo 专项），含 `reqwest::resolve` SSRF 误报排除论证。
- **Archi（系统架构师）** 原始产出：架构评估与技债盘点（模块依赖图 + Priority 排序债表 + 特性门控评估 + upload 对应到自更新 U1）。
- **Rex（SRE 工程师）** 原始产出：部署就绪度 Go/No-Go 清单 + 事故响应演练（场景 A/B 的 SEV/5Why/恢复）+ 运维真实风险清单。
- **Tessa（测试专家）** 原始产出：测试覆盖矩阵（能力×状态×建议）+ 缺失测试优先级 + 补测计划 + CI 状态建议。
- **Docu（技术文档师）** 原始产出：文档债务清单（P0–P2）+ 工程化成熟度评分表 + 结构化目录/onboarding 建议 + upload 文档暴露度专项。

---

> 本报告由工程保障团队 AI 协作生成，关键决策（尤其"上传仓库初始化"范围与 P0 修复优先级）请由人类工程负责人复核。
