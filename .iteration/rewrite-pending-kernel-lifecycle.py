# -*- coding: utf-8 -*-
"""Rewrite Pending: kernel lifecycle UX + shell attach, not always-on service."""
from pathlib import Path
import json
import re
import subprocess
import sys

root = Path(r"C:\code\wind")
skill = Path(r"C:\Users\12867\.grok\skills\notebooklm-iteration-loop")
sys.path.insert(0, str(skill / "scripts"))
from requirements_store import apply_operation, read_records

(root / ".iteration" / "request.txt").write_text(
    "修订内核需求：内核不是系统常驻服务；可与外壳分离存活；托盘/Rdg 退出语义；"
    "彻底退出内核会联动退出外壳；二次确认与 hover 提示；桌面重开 detect-or-spawn。\n"
    "综合 NLM 对话原意 + 用户本轮细节明确。\n",
    encoding="utf-8",
)

mds = [
    """### PENDING-REQ-RIDGE-KERNEL-HOST-01 · 内核进程与外壳生命周期（深根模式）

- 类型:NEW
- 原始意图:综合 NLM 对话与用户修订：Ridge 为「内核进程 + 薄外壳」。内核**不是**系统级常驻服务（非 Windows Service / 非开机永驻），而是用户会话内可独立存活的明确进程；有清晰启动与**彻底退出**入口。外壳（桌面 Tauri / rdg / Web）可单独退出而内核仍运行（深根模式）；也可彻底退出内核，则所有外壳一并结束。桌面重开时先检测内核是否已在跑：在则接入，不在则自启。Web Remote 直连内核；窗级工作区组合仍是桌面外壳分流。优先级：先把内核与生命周期做对，再扩其他功能。
- 关联 Active 条款:REQ-20260730-01、REQ-RDG-REMOTE-CONNECT-01
- 目标行为:内核是用户会话内可独立于外壳存活的明确进程（非 Windows 服务/非开机永驻）；可检测单实例（socket/pid 等）。桌面托盘/菜单：原「彻底退出」改为「退出桌面端」（只关桌面 UI，内核继续）；新增「彻底退出」结束内核且不弹确认窗，Hover 须提示将一并退出仍连接的 rdg 等外壳。rdg 增加「彻底退出」结束内核；若此时桌面仍在运行须系统级二次确认，取消则不杀内核；仅退 rdg UI 则保留内核。一旦内核退出（桌面彻底退出 / rdg 确认后 / CLI），所有已连接外壳（桌面与 rdg）自动退出。桌面与 rdg 启动时先检测内核：在则接入、不在则自启，默认禁止静默双开两套内核。提供 CLI 结束内核（与 GUI 彻底退出同效）。
- 范围:内核进程模型与发现/attach、桌面托盘/菜单文案与行为、rdg 退出菜单与系统确认框、启动 detect-or-spawn、CLI 杀内核、外壳在内核死亡时的自退
- 非目标:不做成 Windows 服务/开机自启默认；不重做业务功能 UI；不引入 CRDT；不自动发版
- 不可动边界:退出桌面不得误杀用户未选择「彻底退出」时的内核；彻底退出后不得残留孤儿外壳占资源；不丢 PTY 字节；默认单内核实例避免静默双开
- 假设/待确认:单实例默认是否允许高级用户多内核；Web 外壳无「彻底退出」菜单时仅随内核死亡断开即可；托盘文案最终用词（中/英）
- 确定性验收:① 退出桌面后端内核进程仍在、rdg 仍可 attach；② 桌面「彻底退出」无确认框结束内核，rdg 自动退出；Hover 文案含一并退出 rdg 提示；③ rdg 彻底退出且桌面仍在时弹出系统确认，取消则内核与桌面均在；确认后内核与桌面均退出；④ 杀内核后桌面与 rdg 均退出；⑤ 冷启动桌面：无内核则拉起，有内核则接入（测双启场景）；⑥ CLI 可结束内核
- 预期追踪:PENDING-REQ-RIDGE-KERNEL-HOST-01 → NLM notes 1dd91891+27be8446 + 用户修订 → tray/rdg/lifecycle
""",
    """### PENDING-REQ-RIDGE-KERNEL-DOMAIN-01 · 领域能力 SSOT 在内核（外壳只投影）

- 类型:MODIFY
- 原始意图:用户要求文件系统、Git、远端接入、Agent 名册/编组/历史、设置等均为**内核能力**，Tauri/rdg 只是外壳投影。能力若仍挂在 Tauri 命令上，无头 rdg 与「退出桌面、内核仍跑」的深根模式都会假死或不可用。本条与 HOST 生命周期配套：HOST 定义进程与退出；本条定义**哪些能力必须挂在内核进程内**。
- 关联 Active 条款:REQ-20260730-01、REQ-AGENT-CATALOG-01、REQ-RDG-REMOTE-CONNECT-01、REQ-AGENT-COMMUNE-CONTINUITY-01
- 目标行为:下列能力的权威实现与调用出口在内核进程（或 packages 纯库由内核暴露），桌面/rdg 仅 UI：① 工作区文件/目录与路径沙箱；② Git/SCM 与 process 护栏；③ 远端 Host 接入/拓扑/会话；④ Agent 发现、花名册、编组、历史与 resume 计划；⑤ 与运行时相关的设置。深根模式下退出桌面后，rdg 仍能调用上述能力。
- 范围:ridge-core/teammate/git/remote/settings 出口迁入或固定在内核；各外壳改道调用
- 非目标:不在本条做大爆炸 UI 重写；纯外观主题可仍在外壳；不伪造无头已全绿
- 不可动边界:外部进程墙钟超时+杀树+同生死许可；双端并发 cap 同常量；复合身份不回退 activeWorkspace 猜
- 假设/待确认:编组前端 localStorage 是否分阶段迁内核；设置文件路径与单内核实例绑定方式
- 确定性验收:深根（桌面已退、内核在）下 rdg 至少完成 FS/Git/Agent 花名册读/Remote 之一的真实路径；同路径桌面可复现；内核退出后上述调用 fail-closed
- 预期追踪:PENDING-REQ-RIDGE-KERNEL-DOMAIN-01 → note 27be8446 → core 能力矩阵
""",
    """### PENDING-REQ-RIDGE-MCP-AS-KERNEL-API-01 · ridge-mcp 接内核而非 Tauri 随从

- 类型:FIX
- 原始意图:ridge-mcp 被 Tauri 接管即做错。应基于 teammate 服务或 tmux 垫片，直接拉起终端并启动 agent，支持相互定位与交流——是**内核**暴露给外部 agent 的 API 面。深根模式下桌面退出后 MCP 仍应能连内核（若内核在），而非依赖桌面进程。
- 关联 Active 条款:REQ-RIDGE-MCP-INSTALLER-01、REQ-AGENT-COMMUNE-LAUNCH-PROFILE-01、REQ-MCP-JOIN-GROUP-01、REQ-AGENT-COMMUNE-MCP-SUBMIT-03
- 目标行为:ridge-mcp 默认发现并连接**当前内核**（与桌面/rdg 同发现机制），不依赖 Tauri invoke 作为唯一后端；可 split/launch agent、roster 寻址、跨 agent 消息/委派；内核不在时明确错误；内核被彻底退出后 MCP 连接断开且不假成功。
- 范围:packages/ridge-mcp、teammate、ridge-tmux、发现/attach 与 HOST 一致、文档
- 非目标:不删除可选桌面捆绑安装；不重做 Commune 全部 UI
- 不可动边界:非法目标稳定失败码；禁止静默落到错误 pane
- 假设/待确认:与 KERNEL-HOST 单实例发现共用同一约定
- 确定性验收:① 无 Tauri 仅内核时 MCP initialize+tools/list+至少一类协作工具可测；② 桌面退出、内核仍在时 MCP 仍可用或可重连；③ 内核退出后 MCP 失败可观测；④ 文档写明拓扑
- 预期追踪:PENDING-REQ-RIDGE-MCP-AS-KERNEL-API-01 → note a7962b2f → ridge-mcp/teammate
""",
]

pendings = []
for md in mds:
    m = re.search(r"### (PENDING-REQ-[A-Z0-9-]+)", md)
    pendings.append({"id": m.group(1), "section": "pending", "markdown": md.strip()})

old = list(read_records(root / "docs" / "PENDING-REQUIREMENTS.md", "pending").keys())
new_ids = {p["id"] for p in pendings}
remove = [i for i in old if i not in new_ids]
op = {"schema_version": 1, "upsert": pendings, "remove": remove}
(root / ".iteration" / "kernel-lifecycle-pending-op.json").write_text(
    json.dumps(op, ensure_ascii=False, indent=2), encoding="utf-8"
)
apply_operation(
    root / "docs" / "REQUIREMENTS-SPEC.md",
    root / "docs" / "PENDING-REQUIREMENTS.md",
    op,
    evidence="用户修订：内核非常驻服务；退出/接入/二次确认/hover 细节",
)
print("pending", list(read_records(root / "docs" / "PENDING-REQUIREMENTS.md", "pending").keys()))

decision = {
    "schema_version": 1,
    "intake_id": "INTAKE-20260731-KERNEL-LIFECYCLE",
    "classification": "pending",
    "summary": "Kernel lifecycle deep-root mode + domain + mcp",
    "requirement_ids": [],
    "pending_ids": [p["id"] for p in pendings],
    "approval_reasons": [
        "user-lifecycle-detail-revision",
        "not-always-on-service",
        "tray-rdg-exit-semantics",
    ],
    "open_questions": [],
    "approval_evidence": "",
}
(root / ".iteration" / "intake-decision-kernel-lifecycle.json").write_text(
    json.dumps(decision, ensure_ascii=False, indent=2), encoding="utf-8"
)
r = subprocess.run(
    [
        sys.executable,
        str(skill / "scripts" / "requirements_intake.py"),
        "build",
        "--request-file",
        str(root / ".iteration" / "request.txt"),
        "--decision",
        str(root / ".iteration" / "intake-decision-kernel-lifecycle.json"),
        "--intake-file",
        str(root / ".iteration" / "intakes" / "INTAKE-20260731-KERNEL-LIFECYCLE.json"),
    ],
    capture_output=True,
    text=True,
    encoding="utf-8",
)
print("INTAKE", r.returncode)
print(r.stdout)
