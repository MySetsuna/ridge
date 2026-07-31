# -*- coding: utf-8 -*-
from pathlib import Path
import json
import re
import subprocess
import sys

root = Path(r"C:\code\wind")
skill = Path(r"C:\Users\12867\.grok\skills\notebooklm-iteration-loop")
sys.path.insert(0, str(skill / "scripts"))
from requirements_store import apply_operation, read_records

mds = [
    """### PENDING-REQ-KERNEL-DAEMON-01 · Ridge 内核从 Tauri 彻底解耦并 Daemon 化

- 类型:NEW
- 原始意图:用户对话 note 1dd91891：把 ridge 内核彻底从 tauri 耦合拆除；前端状态只在 tauri/tui/web 三种外壳管理；web remote 直连内核不经 tauri；rdg 与桌面接同一套服务；窗级工作区组合是纯外壳分流；先做完内核再做其他功能
- 关联 Active 条款:REQ-20260730-01、REQ-RDG-REMOTE-CONNECT-01
- 目标行为:存在可独立运行的内核进程/服务（Daemon 或等价）作为 PTY/工作区/调度 SSOT；Tauri/rdg/Web 均为无状态或薄投影客户端；Web Remote 与 rdg 不经 Tauri IPC 即可接入同一内核；桌面多窗口仅外壳分流认领工作区
- 范围:packages/ridge-core、ridge-term、daemon/IPC 面、rdg host、desktop 接入层、remote 直连路径
- 非目标:本条不重做业务 UI；不引入 CRDT；不强制一次删光所有桌面能力
- 不可动边界:不丢 PTY 字节；不引入第二套工作区真相源；不自动发版
- 假设/待确认:Daemon 形态（独立进程 vs 桌面内嵌服务）与迁移切片顺序
- 确定性验收:无 Tauri 时 rdg 或 headless host 可拉起内核并列出/操作工作区+pane；Web/桌面接入同一内核有证据；双客户端不同时写冲突有契约
- 预期追踪:PENDING-REQ-KERNEL-DAEMON-01 → note 1dd91891 → ridge-core/daemon
""",
    """### PENDING-REQ-MCP-KERNEL-SURFACE-01 · ridge-mcp 以 teammate/tmux 内核面为宿主

- 类型:FIX
- 原始意图:用户对话 note a7962b2f：ridge-mcp 不应完全由 Tauri 接管；应基于 teammate 服务或 tmux 垫片，直接拉起终端并启动 agent，并使 agent 相互定位与交流
- 关联 Active 条款:REQ-RIDGE-MCP-INSTALLER-01、REQ-AGENT-COMMUNE-LAUNCH-PROFILE-01、REQ-MCP-JOIN-GROUP-01
- 目标行为:ridge-mcp 通过内核 teammate/socket（或 tmux shim）连接，不依赖 Tauri command 作为唯一后端；可 split/launch agent、roster 寻址、agent 间通讯；无桌面 GUI 时能力仍可用或明确 capability 错误
- 范围:packages/ridge-mcp、teammate server、ridge-tmux、desktop mcp 桥的去耦合
- 非目标:不删除桌面安装路径；不重做全部 Commune UI
- 不可动边界:不得静默落到错误 pane；复合身份 (workspaceId,paneId) 强制校验
- 假设/待确认:与 KERNEL-DAEMON 的先后依赖（建议 Daemon 后重写连接器）
- 确定性验收:无 Tauri 或 headless 路径下 tools/list 与至少 launch/send/profile 路径可测；非法寻址 -32602；文档口径与宿主一致
- 预期追踪:PENDING-REQ-MCP-KERNEL-SURFACE-01 → note a7962b2f → ridge-mcp/teammate
""",
    """### PENDING-REQ-KERNEL-CAPABILITIES-01 · FS/Git/Agent/远端/设置归内核能力

- 类型:MODIFY
- 原始意图:用户对话 note 27be8446：文件系统、git、接入、agent 编组与历史、远端接入、设置等均应为内核能力，完全不依赖 tauri；外壳只做投影；怀疑 rdg 不可用正因能力未与 tauri 解耦
- 关联 Active 条款:REQ-20260730-01、REQ-AGENT-CATALOG-01、REQ-RDG-REMOTE-CONNECT-01
- 目标行为:FS/Git/Agent 名册编组历史/远端接入/设置的读写与执行出口在内核层可调用；rdg/headless 与桌面共享同一实现；Tauri 仅 UI/窗口/托盘/权限桥
- 范围:ridge-core 命令出口、teammate、git process_guard、remote host、settings 持久化、rdg 接入
- 非目标:不在本条完成全部 UI 迁移；不改用户数据无故删除
- 不可动边界:外部进程须墙钟超时+杀进程树；双端并发 cap 同常量
- 假设/待确认:设置存储位置与跨外壳共享格式
- 确定性验收:rdg 或 headless 调用矩阵覆盖 FS/Git/Agent/remote 至少各 1 条真实路径；桌面同路径同源；无 Tauri 事件桥时 agent 花名册仍可读
- 预期追踪:PENDING-REQ-KERNEL-CAPABILITIES-01 → note 27be8446 → core/rdg
""",
]

pendings = []
for md in mds:
    m = re.search(r"### (PENDING-REQ-[A-Z0-9-]+)", md)
    pendings.append({"id": m.group(1), "section": "pending", "markdown": md.strip()})

old = list(read_records(root / "docs" / "PENDING-REQUIREMENTS.md", "pending").keys())
remove = [i for i in old if i not in {p["id"] for p in pendings}]
op = {"schema_version": 1, "upsert": pendings, "remove": remove}
(root / ".iteration" / "kernel-dialog-pending-op.json").write_text(
    json.dumps(op, ensure_ascii=False, indent=2), encoding="utf-8"
)
apply_operation(
    root / "docs" / "REQUIREMENTS-SPEC.md",
    root / "docs" / "PENDING-REQUIREMENTS.md",
    op,
    evidence="对话转Note后抓取整理Pending",
)
print("pending", list(read_records(root / "docs" / "PENDING-REQUIREMENTS.md", "pending").keys()))

decision = {
    "schema_version": 1,
    "intake_id": "INTAKE-20260731-KERNEL-DIALOGS",
    "classification": "pending",
    "summary": "Three user NLM dialogs as kernel Pending REQs",
    "requirement_ids": [],
    "pending_ids": [p["id"] for p in pendings],
    "approval_reasons": [
        "user-asked-dialogs-to-notes-then-fetch",
        "history-rpc-khqZz-no-cdp",
        "mcp-note-list-verified",
    ],
    "open_questions": [],
    "approval_evidence": "",
}
(root / ".iteration" / "intake-decision-kernel-dialogs.json").write_text(
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
        str(root / ".iteration" / "intake-decision-kernel-dialogs.json"),
        "--intake-file",
        str(root / ".iteration" / "intakes" / "INTAKE-20260731-KERNEL-DIALOGS.json"),
    ],
    capture_output=True,
    text=True,
    encoding="utf-8",
)
print("INTAKE", r.returncode)
print(r.stdout)
print(r.stderr)
