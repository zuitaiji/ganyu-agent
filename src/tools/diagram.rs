//! `ganyu tool diagram`：纯 Rust 生成 SVG 流程图 / 泳道图（无第三方依赖）。
//! 等价替换 `docs/ai-arch/diagrams/gen_diagrams.py`：输出 role_interaction.svg 与
//! upload_repo_init_lane.svg。默认写到当前目录，第一个参数为输出目录。

use crate::error::GanyuResult;

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[allow(clippy::too_many_arguments)]
fn box_node(
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    label: &str,
    fill: &str,
    stroke: &str,
    fs: f64,
    bold: bool,
) -> String {
    let weight = if bold { "font-weight:bold;" } else { "" };
    let lines: Vec<&str> = label.split('\n').collect();
    let n = lines.len();
    let mut t = String::new();
    for (i, ln) in lines.iter().enumerate() {
        let dy = y + h / 2.0 + (i as f64 - (n as f64 - 1.0) / 2.0) * (fs + 4.0) + fs / 3.0;
        t.push_str(&format!(
            "<text x=\"{}\" y=\"{}\" font-size=\"{}\" text-anchor=\"middle\" fill=\"#1A2740\" style=\"font-weight:{};font-family:Segoe UI,Microsoft YaHei,sans-serif\">{}</text>",
            x + w / 2.0,
            dy,
            fs,
            weight,
            esc(ln)
        ));
    }
    format!(
        "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"8\" ry=\"8\" fill=\"{}\" stroke=\"{}\" stroke-width=\"1.5\"/>{}",
        x, y, w, h, fill, stroke, t
    )
}

fn edge(
    x1: f64,
    y1: f64,
    x2: f64,
    y2: f64,
    label: &str,
    color: &str,
    dash: bool,
) -> String {
    let d = if dash { " stroke-dasharray=\"6,4\"" } else { "" };
    let mut a = format!(
        "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"{}\" stroke-width=\"1.4\"{}{}\"/>",
        x1, y1, x2, y2, color, d, " marker-end=\"url(#arrow)\""
    );
    if !label.is_empty() {
        let mx = (x1 + x2) / 2.0;
        let my = (y1 + y2) / 2.0;
        a.push_str(&format!(
            "<rect x=\"{}\" y=\"{}\" width=\"116\" height=\"16\" rx=\"3\" fill=\"#FFFFFF\" opacity=\"0.85\"/>",
            mx - 58.0,
            my - 12.0
        ));
        a.push_str(&format!(
            "<text x=\"{}\" y=\"{}\" font-size=\"10.5\" text-anchor=\"middle\" fill=\"{}\" style=\"font-family:Segoe UI,Microsoft YaHei,sans-serif\">{}</text>",
            mx, my, color, esc(label)
        ));
    }
    a
}

const HEAD: &str = "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}\" height=\"{h}\" viewBox=\"0 0 {w} {h}\" font-family=\"Segoe UI,Microsoft YaHei,sans-serif\"><defs><marker id=\"arrow\" markerWidth=\"10\" markerHeight=\"10\" refX=\"8\" refY=\"3\" orient=\"auto\" markerUnits=\"strokeWidth\"><path d=\"M0,0 L8,3 L0,6 Z\" fill=\"#6B7A99\"/></marker></defs>";
const TAIL: &str = "</svg>";

fn gen_role_interaction(out_dir: &str) -> GanyuResult<()> {
    let w: usize = 820;
    let h: usize = 580;
    let mut s: Vec<String> = Vec::new();
    s.push(HEAD.replace("{w}", &w.to_string()).replace("{h}", &h.to_string()));

    let cx = 300.0;
    let cy = 250.0;
    let cw = 220.0;
    let ch = 96.0;
    s.push(box_node(cx, cy, cw, ch, "ganyu-agent 核心", "#3B5B92", "#27406B", 17.0, true));
    s.push(format!(
        "<text x=\"{}\" y=\"{}\" font-size=\"11\" text-anchor=\"middle\" fill=\"#FFFFFF\" style=\"font-family:Segoe UI,Microsoft YaHei,sans-serif\">多范式引擎 · 安全基件 · upload-repo-init</text>",
        cx + cw / 2.0,
        cy + ch / 2.0 + 34.0
    ));

    let roles: [(f64, f64, &str); 5] = [
        (50.0,  50.0,  "甲方决策者 (项目 Owner)"),
        (575.0, 50.0,  "AI 编码团队 Lead"),
        (50.0,  450.0, "合规审计"),
        (575.0, 450.0, "CI / 运维 SRE"),
        (310.0, 490.0, "终端开发者 / CLI 使用者"),
    ];
    for (rx, ry, rl) in roles.iter() {
        s.push(box_node(*rx, *ry, 195.0, 56.0, rl, "#E8EEF7", "#3B5B92", 12.0, true));
    }
    s.push(edge(245.0, 78.0, cx + 35.0, cy, "架构/ROI/合规把关", "#6B7A99", false));
    s.push(edge(575.0 + 97.0, 78.0, cx + cw - 35.0, cy, "监控/干预/合规查看", "#6B7A99", false));
    s.push(edge(147.0, 450.0 + 28.0, cx + 35.0, cy + ch, "审计留痕/完整性核验", "#6B7A99", false));
    s.push(edge(575.0 + 97.0, 450.0 + 28.0, cx + cw - 35.0, cy + ch, "部署/升级/监控/回滚", "#6B7A99", false));
    s.push(edge(407.0, 490.0, cx + cw / 2.0, cy + ch, "工作流/记忆/插件/仓库初始化", "#6B7A99", false));

    let ext: [(&str, f64, f64); 5] = [
        ("LLM 网关 / 后端", 620.0, 120.0),
        ("记忆后端", 620.0, 195.0),
        ("插件/skill (vetted)", 620.0, 270.0),
        ("构建发布 CI", 50.0, 195.0),
        ("远端仓库 (opt-in)", 50.0, 120.0),
    ];
    for (name, ex, ey) in ext.iter() {
        s.push(box_node(*ex, *ey, 150.0, 50.0, name, "#FBEEDA", "#C8922E", 11.0, false));
    }
    s.push(edge(cx + cw, cy + 12.0, 620.0, 145.0, "模型推理/补全", "#6B7A99", false));
    s.push(edge(cx + cw, cy + 45.0, 620.0, 220.0, "记忆读写/召回", "#6B7A99", false));
    s.push(edge(cx + cw, cy + 78.0, 620.0, 295.0, "工具扩展(本地子进程)", "#6B7A99", false));
    s.push(edge(cx, cy + 12.0, 200.0, 145.0, "push(F10)", "#6B7A99", true));
    s.push(edge(cx, cy + 78.0, 200.0, 220.0, "三平台签名构建", "#6B7A99", true));
    s.push(TAIL.to_string());

    let p = format!("{}/role_interaction.svg", out_dir);
    std::fs::write(&p, s.join("\n")).map_err(|e| crate::error::GanyuError::Io(e))?;
    println!("written {p}");
    Ok(())
}

fn gen_upload_repo_lane(out_dir: &str) -> GanyuResult<()> {
    let w: usize = 880;
    let h: usize = 600;
    let mut s: Vec<String> = Vec::new();
    s.push(HEAD.replace("{w}", &w.to_string()).replace("{h}", &h.to_string()));

    let lanes: [(f64, &str); 4] = [
        (0.0, "终端开发者"),
        (96.0, "ganyu-agent 状态机"),
        (192.0, "安全基件"),
        (288.0, "本地仓库"),
    ];
    for (y, name) in lanes.iter() {
        s.push(format!(
            "<rect x=\"0\" y=\"{}\" width=\"{}\" height=\"96\" fill=\"#F4F7FB\"/>",
            y, w
        ));
        s.push(format!(
            "<rect x=\"0\" y=\"{}\" width=\"170\" height=\"96\" fill=\"#DCE6F2\" stroke=\"#9FB2CE\"/>",
            y
        ));
        s.push(format!(
            "<text x=\"85\" y=\"{}\" font-size=\"13\" text-anchor=\"middle\" fill=\"#1A2740\" style=\"font-weight:bold;font-family:Segoe UI,Microsoft YaHei,sans-serif\">{}</text>",
            y + 52.0,
            name
        ));
    }

    let step = |lane_y: f64, x: f64, label: &str, fill: &str, stroke: &str, fs: f64| -> String {
        box_node(x, lane_y + 22.0, 210.0, 52.0, label, fill, stroke, fs, false)
    };

    s.push(step(0.0, 210.0, "执行 `ganyu repo init <path>`\n(单命令, 步骤数 <= 1)", "#DDEBFF", "#3B5B92", 11.0));
    s.push(step(96.0, 210.0, "Check-Before-Act 幂等键检查", "#DDEBFF", "#3B5B92", 11.0));
    s.push(step(192.0, 210.0, "resolve_sandboxed /\nssrf_guard_resolve", "#FBEEDA", "#C8922E", 11.0));
    s.push(step(96.0, 450.0, "状态机: Pending->Running", "#DDEBFF", "#3B5B92", 11.0));
    s.push(step(192.0, 450.0, "restrict_file_permissions /\nshell 双层门禁 (fail-closed)", "#FBEEDA", "#C8922E", 11.0));
    s.push(step(288.0, 450.0, "git init/clone/commit\n写入 .git / objects", "#E6F4E6", "#2E7D32", 11.0));
    s.push(step(96.0, 690.0, "状态机: -> Succeeded", "#DDEBFF", "#3B5B92", 11.0));
    s.push(step(288.0, 690.0, "已初始化仓库落盘", "#E6F4E6", "#2E7D32", 11.0));
    s.push(step(0.0, 690.0, "收到一致结果 / 审计日志 NDJSON", "#DDEBFF", "#3B5B92", 11.0));

    s.push(box_node(210.0, 396.0, 470.0, 40.0, "失败 -> 补偿回滚 (Failed) -> 开发者收到明确错误", "#FBD9D9", "#B23B3B", 11.0, false));
    s.push(box_node(210.0, 546.0, 470.0, 40.0, "重复执行 -> 幂等键命中 -> 直接返回 Succeeded（结果一致）", "#DDEBFF", "#3B5B92", 11.0, false));

    s.push(edge(315.0, 0.0 + 48.0, 315.0, 96.0 + 22.0, "启动", "#6B7A99", false));
    s.push(edge(315.0, 96.0 + 74.0, 315.0, 192.0 + 22.0, "委派安全校验", "#6B7A99", false));
    s.push(edge(420.0, 192.0 + 48.0, 450.0, 96.0 + 48.0, "校验通过", "#2E7D32", false));
    s.push(edge(450.0, 96.0 + 74.0, 450.0, 288.0 + 22.0, "驱动 git 操作", "#6B7A99", false));
    s.push(edge(450.0, 288.0 + 74.0, 450.0, 96.0 + 74.0, "完成回调", "#2E7D32", false));
    s.push(edge(450.0, 96.0 + 74.0, 690.0, 96.0 + 48.0, "Succeeded", "#6B7A99", false));
    s.push(edge(795.0, 288.0 + 48.0, 795.0, 0.0 + 48.0, "结果反馈", "#6B7A99", false));
    s.push(edge(315.0, 96.0 + 74.0, 210.0, 416.0, "异常分支", "#B23B3B", true));
    s.push(edge(315.0, 96.0 + 48.0, 210.0, 566.0, "幂等命中", "#3B5B92", true));
    s.push(TAIL.to_string());

    let p = format!("{}/upload_repo_init_lane.svg", out_dir);
    std::fs::write(&p, s.join("\n")).map_err(|e| crate::error::GanyuError::Io(e))?;
    println!("written {p}");
    Ok(())
}

/// 入口：`ganyu tool diagram [out_dir]`。等价于 `python gen_diagrams.py`。
pub fn run(_args: &[String]) -> GanyuResult<()> {
    let out_dir = if let Some(a) = _args.first() {
        if a == "--help" || a == "-h" {
            println!("用法: ganyu tool diagram [out_dir]");
            println!("  生成 role_interaction.svg 与 upload_repo_init_lane.svg");
            println!("  out_dir 缺省为当前目录");
            return Ok(());
        }
        a.clone()
    } else {
        ".".to_string()
    };
    std::fs::create_dir_all(&out_dir).map_err(|e| crate::error::GanyuError::Io(e))?;
    gen_role_interaction(&out_dir)?;
    gen_upload_repo_lane(&out_dir)?;
    Ok(())
}
