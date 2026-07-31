# -*- coding: utf-8 -*-
"""Rewrite Pending: synthesize dialog intent, not one-REQ-per-dialog."""
from pathlib import Path
import json
import re
import subprocess
import sys

root = Path(r"C:\code\wind")
skill = Path(r"C:\Users\12867\.grok\skills\notebooklm-iteration-loop")
sys.path.insert(0, str(skill / "scripts"))
from requirements_store import apply_operation, read_records

# Request rebind
(root / ".iteration" / "request.txt").write_text(
    "整合 NLM 三轮用户对话原意，重写 Pending：按真正需求意图归纳，禁止一问一条。\n"
    "来源 note: 1dd91891 / a7962b2f / 27be8446（对话导出）。\n",
    encoding="utf-8",
)

mds = [
    """### PENDING-REQ-RIDGE-KERNEL-HOST-01 · 单一内核宿主与三外壳边界

- 类型:NEW
- 原始意图:综合用户三轮对话：Ridge 应是「内核 + 外壳」而非 Tauri 应用内嵌一切。内核为工作区/PTY/调度等事实 SSOT；Tauri、rdg(TUI)、Web Remote 仅为外壳——只管理各自前端/窗口状态与投影。Web Remote 直连内核、不经 Tauri；rdg 与桌面接入同一套内核服务；桌面多开时「工作区按窗口组合」是纯 Tauri 外壳分流。用户明确优先级：先做完内核，再扩其他功能。
- 关联 Active 条款:REQ-20260730-01、REQ-RDG-REMOTE-CONNECT-01
- 目标行为:1) 存在可独立于 Tauri GUI 启动的内核宿主（Daemon/等价常驻服务或可 headless 拉起的同源 host）；2) 内核持有工作区拓扑、PTY 生命周期、scrollback/调度等 SSOT；3) 桌面/rdg/Web 均可作为客户端接入同一内核；4) 外壳本地状态（焦点窗、窗内 active workspace 投影、主题等）不得冒充内核真相源；5) 无头 rdg 可用性不再依赖「必须先起 Tauri」。
- 范围:packages/ridge-core、ridge-term、内核 IPC/Daemon 面、rdg host、desktop 接入改为客户端、remote 直连内核路径
- 非目标:本条不重做业务 UI 视觉；不引入 CRDT/多主复制；不要求一次删除全部桌面代码；不自动发版
- 不可动边界:不得丢 PTY 字节/乱序；不得出现双工作区 SSOT 竞写无契约；密钥与用户数据不进仓
- 假设/待确认:内核进程模型（独立 daemon vs 首壳拉起+后续 attach）与迁移切片顺序；与既有 LAN/cloud host 进程关系
- 确定性验收:① 无 Tauri 进程时 headless/rdg 可启动内核并 list 工作区/pane；② 桌面与 rdg（或第二客户端）同时 attach 同一内核有证据且写冲突有契约；③ Web/Remote 路径不经 Tauri IPC 完成至少 handshake+subscribe；④ 文档写清内核/外壳职责表
- 预期追踪:PENDING-REQ-RIDGE-KERNEL-HOST-01 → notes 1dd91891+27be8446 → ridge-core/daemon/rdg
""",
    """### PENDING-REQ-RIDGE-KERNEL-DOMAIN-01 · 领域能力 SSOT 下沉内核

- 类型:MODIFY
- 原始意图:用户指出文件系统、Git、远端接入、Agent 名册/编组/历史、设置等「都应该是内核能力」，Tauri「完完全全是外壳」。并归因：能力未与 Tauri 解耦，可能导致无头 rdg 长期不可用。本条把「哪些能力必须可在无 GUI 下由内核提供」立成合同，避免只拆进程壳、业务仍挂在 Tauri 命令上。
- 关联 Active 条款:REQ-20260730-01、REQ-AGENT-CATALOG-01、REQ-RDG-REMOTE-CONNECT-01、REQ-AGENT-COMMUNE-CONTINUITY-01
- 目标行为:下列能力的权威实现与调用出口在内核层（或 packages 纯库 + 内核暴露），桌面仅 UI 投影：① 工作区文件/目录操作与路径沙箱；② Git/SCM 执行与 process 护栏；③ 远端 Host 接入/拓扑/会话；④ Agent 发现、花名册、编组、历史与 resume 计划；⑤ 用户设置中与运行时行为相关的项。rdg/headless 与桌面调用同一实现路径。
- 范围:ridge-core 命令/服务出口、teammate、git process_guard、remote host、settings 持久化、各外壳对上述 API 的调用改道
- 非目标:不在本条做大爆炸 UI 重写；不把纯外观主题强制进内核；不伪造无头已可用
- 不可动边界:外部进程须墙钟超时+杀进程树+同生死许可；前后端并发 cap 同常量；复合身份 (workspaceId,paneId) 不回退 activeWorkspace 猜
- 假设/待确认:设置存储格式与多外壳共享位置；Agent 编组前端 localStorage SSOT 是否迁内核（可分阶段，须写清）
- 确定性验收:headless/rdg 能力矩阵至少各覆盖 FS、Git、Agent 花名册读、Remote 接入（或明确 blocked 分项）一条真实路径；同路径桌面可复现；无 Tauri 事件桥时花名册/设置仍可读可写（写权限按阶段）
- 预期追踪:PENDING-REQ-RIDGE-KERNEL-DOMAIN-01 → note 27be8446 → core 能力矩阵
""",
    """### PENDING-REQ-RIDGE-MCP-AS-KERNEL-API-01 · ridge-mcp 作为内核协作 API 面

- 类型:FIX
- 原始意图:用户判定「ridge-mcp 完全由 Tauri 接管」即做错。正确形态：MCP 基于 teammate 服务或 tmux 垫片，直接拉起终端并启动 agent，支持 agent 相互定位与交流——是内核暴露给外部 agent 的标准接口，不是桌面随从。
- 关联 Active 条款:REQ-RIDGE-MCP-INSTALLER-01、REQ-AGENT-COMMUNE-LAUNCH-PROFILE-01、REQ-MCP-JOIN-GROUP-01、REQ-AGENT-COMMUNE-MCP-SUBMIT-03
- 目标行为:ridge-mcp 默认连接内核 teammate/socket（或文档化的 headless host），不依赖 Tauri invoke 作为唯一后端；在无桌面 GUI 时仍可（或能力明确失败）：split/launch agent、roster 寻址、跨 agent 传消息/委派；启动会话携带复合身份；与 KERNEL-HOST/DOMAIN 对齐后成为「装了内核就能协作」的入口。
- 范围:packages/ridge-mcp、teammate server、ridge-tmux shim、去 Tauri-only 路径、安装/文档口径
- 非目标:不删除可选桌面捆绑安装；不重做 Commune 全部 UI
- 不可动边界:越界/未知目标必须失败（-32602 等），禁止静默落到 0 号分屏；不得在无身份时假装入组成功
- 假设/待确认:依赖 KERNEL-HOST 可达性；companion-only 宿主错误码（Unsupported vs InvalidParams）统一表
- 确定性验收:① 无 Tauri 时 MCP initialize+tools/list+至少 launch 或 send/profile 一类工具可测；② 非法 pane/agent 稳定错误；③ 文档写明宿主拓扑（kernel vs desktop bridge）
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
(root / ".iteration" / "kernel-intent-pending-op.json").write_text(
    json.dumps(op, ensure_ascii=False, indent=2), encoding="utf-8"
)
apply_operation(
    root / "docs" / "REQUIREMENTS-SPEC.md",
    root / "docs" / "PENDING-REQUIREMENTS.md",
    op,
    evidence="整合对话原意重写Pending，非一问一条",
)
print("pending", list(read_records(root / "docs" / "PENDING-REQUIREMENTS.md", "pending").keys()))

decision = {
    "schema_version": 1,
    "intake_id": "INTAKE-20260731-KERNEL-INTENT",
    "classification": "pending",
    "summary": "Synthesized kernel/shell intent from 3 user NLM dialogs",
    "requirement_ids": [],
    "pending_ids": [p["id"] for p in pendings],
    "approval_reasons": [
        "user-rejected-one-dialog-one-req",
        "synthesize-true-intent-kernel-shell",
    ],
    "open_questions": [],
    "approval_evidence": "",
}
(root / ".iteration" / "intake-decision-kernel-intent.json").write_text(
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
        str(root / ".iteration" / "intake-decision-kernel-intent.json"),
        "--intake-file",
        str(root / ".iteration" / "intakes" / "INTAKE-20260731-KERNEL-INTENT.json"),
    ],
    capture_output=True,
    text=True,
    encoding="utf-8",
)
print("INTAKE", r.returncode)
print(r.stdout)
