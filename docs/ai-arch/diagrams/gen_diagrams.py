# -*- coding: utf-8 -*-
"""纯 Python 生成 SVG 流程图 / 泳道图（无第三方依赖），用于 ganyu-agent UserStory.md。
对应 diagrams-generator 技能的「流程图/状态机」场景产出，输出 SVG 文件并入文档。
"""
import os

OUT = os.path.dirname(__file__)
os.makedirs(OUT, exist_ok=True)

def esc(s):
    return s.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")

def box(x, y, w, h, label, fill="#E8EEF7", stroke="#3B5B92", fs=13, bold=False):
    weight = "font-weight:bold;" if bold else ""
    lines = label.split("\n")
    t = ""
    n = len(lines)
    for i, ln in enumerate(lines):
        dy = y + h/2 + (i - (n-1)/2) * (fs + 4) + fs/3
        t += (f'<text x="{x+w/2}" y="{dy}" font-size="{fs}" text-anchor="middle" '
              f'fill="#1A2740" style="{weight}font-family:Segoe UI,Microsoft YaHei,sans-serif">{esc(ln)}</text>')
    return (f'<rect x="{x}" y="{y}" width="{w}" height="{h}" rx="8" ry="8" '
            f'fill="{fill}" stroke="{stroke}" stroke-width="1.5"/>{t}')

def edge(x1, y1, x2, y2, label="", color="#6B7A99", dash=False):
    d = ' stroke-dasharray="6,4"' if dash else ''
    a = (f'<line x1="{x1}" y1="{y1}" x2="{x2}" y2="{y2}" stroke="{color}" '
         f'stroke-width="1.4"{d} marker-end="url(#arrow)"/>')
    if label:
        mx, my = (x1+x2)/2, (y1+y2)/2
        a += (f'<rect x="{mx-58}" y="{my-12}" width="116" height="16" rx="3" fill="#FFFFFF" opacity="0.85"/>'
              f'<text x="{mx}" y="{my}" font-size="10.5" text-anchor="middle" '
              f'fill="{color}" style="font-family:Segoe UI,Microsoft YaHei,sans-serif">{esc(label)}</text>')
    return a

HEAD = ('<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" '
        'viewBox="0 0 {w} {h}" font-family="Segoe UI,Microsoft YaHei,sans-serif">'
        '<defs><marker id="arrow" markerWidth="10" markerHeight="10" refX="8" refY="3" '
        'orient="auto" markerUnits="strokeWidth"><path d="M0,0 L8,3 L0,6 Z" fill="#6B7A99"/></marker></defs>')
TAIL = '</svg>'

# ═════════════════════════════════════════════════════════
# 图 1：角色交互图
# ═════════════════════════════════════════════════════════
def gen_role_interaction():
    w, h = 820, 580
    s = [HEAD.format(w=w, h=h)]
    cx, cy, cw, ch = 300, 250, 220, 96
    s.append(box(cx, cy, cw, ch, "ganyu-agent 核心", "#3B5B92", "#27406B", 17, bold=True))
    s.append(f'<text x="{cx+cw/2}" y="{cy+ch/2+34}" font-size="11" text-anchor="middle" '
             f'fill="#FFFFFF" style="font-family:Segoe UI,Microsoft YaHei,sans-serif">多范式引擎 · 安全基件 · upload-repo-init</text>')
    roles = [
        (50, 50, "甲方决策者 (项目 Owner)"),
        (575, 50, "AI 编码团队 Lead"),
        (50, 450, "合规审计"),
        (575, 450, "CI / 运维 SRE"),
        (310, 490, "终端开发者 / CLI 使用者"),
    ]
    for (rx, ry, rl) in roles:
        s.append(box(rx, ry, 195, 56, rl, "#E8EEF7", "#3B5B92", 12, bold=True))
    s.append(edge(245, 78, cx+35, cy, "架构/ROI/合规把关"))
    s.append(edge(575+97, 78, cx+cw-35, cy, "监控/干预/合规查看"))
    s.append(edge(147, 450+28, cx+35, cy+ch, "审计留痕/完整性核验"))
    s.append(edge(575+97, 450+28, cx+cw-35, cy+ch, "部署/升级/监控/回滚"))
    s.append(edge(407, 490, cx+cw/2, cy+ch, "工作流/记忆/插件/仓库初始化"))
    # 外部系统
    ext = [("LLM 网关 / 后端", 620, 120), ("记忆后端", 620, 195),
           ("插件/skill (vetted)", 620, 270), ("构建发布 CI", 50, 195),
           ("远端仓库 (opt-in)", 50, 120)]
    for (name, ex, ey) in ext:
        s.append(box(ex, ey, 150, 50, name, "#FBEEDA", "#C8922E", 11))
    s.append(edge(cx+cw, cy+12, 620, 145, "模型推理/补全"))
    s.append(edge(cx+cw, cy+45, 620, 220, "记忆读写/召回"))
    s.append(edge(cx+cw, cy+78, 620, 295, "工具扩展(本地子进程)"))
    s.append(edge(cx, cy+12, 200, 145, "push(F10)", dash=True))
    s.append(edge(cx, cy+78, 200, 220, "三平台签名构建", dash=True))
    s.append(TAIL)
    p = os.path.join(OUT, "role_interaction.svg")
    with open(p, "w", encoding="utf-8") as f:
        f.write("\n".join(s))
    print("written", p)

# ═════════════════════════════════════════════════════════
# 图 2：upload-repo-init 本地初始化泳道图
# ═════════════════════════════════════════════════════════
def gen_upload_repo_lane():
    w, h = 880, 600
    s = [HEAD.format(w=w, h=h)]
    lanes = [(0, "终端开发者"), (96, "ganyu-agent 状态机"),
             (192, "安全基件"), (288, "本地仓库")]
    for (y, name) in lanes:
        s.append(f'<rect x="0" y="{y}" width="{w}" height="96" fill="#F4F7FB"/>')
        s.append(f'<rect x="0" y="{y}" width="170" height="96" fill="#DCE6F2" stroke="#9FB2CE"/>')
        s.append(f'<text x="85" y="{y+52}" font-size="13" text-anchor="middle" '
                 f'fill="#1A2740" style="font-weight:bold;font-family:Segoe UI,Microsoft YaHei,sans-serif">{name}</text>')
    def step(lane_y, x, label, fill="#E8EEF7", stroke="#3B5B92", w=210, h=52, fs=11):
        return box(x, lane_y+22, w, h, label, fill, stroke, fs)
    # 主链路
    s.append(step(0, 210, "执行 `ganyu repo init <path>`\n(单命令, 步骤数 ≤ 1)", "#DDEBFF"))
    s.append(step(96, 210, "Check-Before-Act 幂等键检查", "#DDEBFF"))
    s.append(step(192, 210, "resolve_sandboxed /\nssrf_guard_resolve", "#FBEEDA"))
    s.append(step(96, 450, "状态机: Pending→Running", "#DDEBFF"))
    s.append(step(192, 450, "restrict_file_permissions /\nshell 双层门禁 (fail-closed)", "#FBEEDA"))
    s.append(step(288, 450, "git init/clone/commit\n写入 .git / objects", "#E6F4E6"))
    s.append(step(96, 690, "状态机: → Succeeded", "#DDEBFF"))
    s.append(step(288, 690, "已初始化仓库落盘", "#E6F4E6"))
    s.append(step(0, 690, "收到一致结果 / 审计日志 NDJSON", "#DDEBFF"))
    # 异常与幂等说明框
    s.append(box(210, 396, 470, 40, "失败 → 补偿回滚 (Failed) → 开发者收到明确错误", "#FBD9D9", "#B23B3B", 11))
    s.append(box(210, 546, 470, 40, "重复执行 → 幂等键命中 → 直接返回 Succeeded（结果一致）", "#DDEBFF", "#3B5B92", 11))
    # 连线（主链路自上而下）
    s.append(edge(315, 0+48, 315, 96+22, "启动"))
    s.append(edge(315, 96+74, 315, 192+22, "委派安全校验"))
    s.append(edge(420, 192+48, 450, 96+48, "校验通过", "#2E7D32"))
    s.append(edge(450, 96+74, 450, 288+22, "驱动 git 操作"))
    s.append(edge(450, 288+74, 450, 96+74, "完成回调", "#2E7D32"))
    s.append(edge(450, 96+74, 690, 96+48, "Succeeded"))
    s.append(edge(795, 288+48, 795, 0+48, "结果反馈"))
    s.append(edge(315, 96+74, 210, 416, "异常分支", "#B23B3B", dash=True))
    s.append(edge(315, 96+48, 210, 566, "幂等命中", "#3B5B92", dash=True))
    s.append(TAIL)
    p = os.path.join(OUT, "upload_repo_init_lane.svg")
    with open(p, "w", encoding="utf-8") as f:
        f.write("\n".join(s))
    print("written", p)

if __name__ == "__main__":
    gen_role_interaction()
    gen_upload_repo_lane()
