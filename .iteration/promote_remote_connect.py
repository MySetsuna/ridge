# -*- coding: utf-8 -*-
from pathlib import Path
import json
import sys

evidence = "批准。然后此前将其推进设置到此前目标中（goal），不要暂停goal"
Path(".iteration/request.txt").write_text(evidence + "\n", encoding="utf-8")

sys.path.insert(0, r"C:\Users\12867\.grok\skills\notebooklm-iteration-loop")
from scripts.requirements_store import read_records

active = read_records(Path("docs/REQUIREMENTS-SPEC.md"), "active")
goal = active["REQ-20260730-01"]["markdown"]

if "REQ-RDG-REMOTE-CONNECT-01" not in goal:
    goal = goal.replace(
        "按依赖、风险、优先级与可验证性逐轮推进。",
        "按依赖、风险、优先级与可验证性逐轮推进。"
        "其中 `REQ-RDG-REMOTE-CONNECT-01`（rdg 公网/LAN × 桌面/手机浏览器四路径真接通）为本 goal 内 P0 硬闸，与其余项并行推进、不得因本条而暂停 goal 其余交付；四格冒烟未过不得宣称 Remote 可用。",
    )
    goal = goal.replace(
        "#### 执行顺序\n\n1. 同步并冻结基线",
        "#### 执行顺序\n\n0. **P0（不暂停其余项）**：落地并验收 `REQ-RDG-REMOTE-CONNECT-01` 四路径真接通；与 1–6 并行/穿插，不单开平行 goal、不因本条停 goal。\n1. 同步并冻结基线",
    )
    goal = goal.replace("- 版本:`v1`", "- 版本:`v1.1`", 1)

new_req = """### REQ-RDG-REMOTE-CONNECT-01 · rdg 公网/LAN Remote 真接通（桌面+手机浏览器）

- 状态:`ACTIVE`
- 版本:`v1`
- 行为:rdg 作 host 时，本迭代须四路径真正接通可用会话（非空白壳/半开服务）：(1) LAN×桌面浏览器：显式启 LAN 后打开 dashboard 根 URL，TOTP/session/E2EE+WS 握手，workspace/pane 列表、PTY 订阅、stdin 回显、resize/claim；(2) LAN×手机浏览器：同根 URL 与 mobile 产物等价接入；(3) 公网×桌面浏览器：rdg 启 public host 后同账户经 cloud relay/WebRTC/E2EE 完成 hello/拓扑/pane/PTY；(4) 公网×手机浏览器：mobile 产物同路径可用。真接通=控制器见 host 在线/拓扑，至少一 pane 可订阅并有输出或输入回显；失败须可行动错误。本条挂入 `REQ-20260730-01` 为 goal 内 P0 硬闸，与 goal 其余项并行推进、不得暂停 goal。
- 边界:范围含 packages/ridge-cli（TUI 启停、LAN/public 生命周期、dashboard 根 URL）、LAN host、packages/remote transport/provider、src/remote/** 与 remote-dist/{desktop,mobile} 启动链、cloud host daemon/artifact 加载、接通相关确定性测与受控真连冒烟。非目标：不重做浏览器完整桌面 IDE；不改会员计费；不开放异账号公网整机；不以 VNC 作路径；未接通前不做无关 UI 大重构；不自动发版/推送。不可动：LAN 不以 cloud 账户作门禁（TOTP/session/E2EE）；公网同账户双校验且 relay 不见 PTY 明文；LAN/Cloud 启动判定不得因 cloud API 成败互相误杀；退出 TUI 回收本 TUI 启动的服务；产物线 remote-dist/{desktop,mobile} 单真相。公网真连无凭证时以协议集成测+runbook 标用户轨。
- 验收:① 启动判定与握手→subscribe→stdin 测绿；② rdg dashboard 根 URL/默认 stopped/显式 start-stop，exit 后端口关闭；③ pnpm build:remote 出 desktop+mobile 且 rdg 可提供静态资源；④ 四格冒烟均有命令+退出码+证据或明确 blocked；⑤ 至少一条 LAN 真浏览器与一条公网（fixture 或真连）E2E 证据，禁只绿单测仍空白壳。
- 追踪:`REQ-RDG-REMOTE-CONNECT-01` → ridge-cli remote/lan_host/dashboard → packages/remote provider → src/remote bootstrap → remote-dist → handshake/subscribe/stdin 测与 smoke；归属 goal `REQ-20260730-01` 执行序 0"""

op = {
    "schema_version": 1,
    "upsert": [
        {
            "id": "REQ-RDG-REMOTE-CONNECT-01",
            "section": "active",
            "markdown": new_req,
        },
        {
            "id": "REQ-20260730-01",
            "section": "active",
            "markdown": goal,
        },
    ],
    "remove": ["PENDING-REQ-RDG-REMOTE-CONNECT-01"],
}
Path(".iteration/approval-operation.json").write_text(
    json.dumps(op, ensure_ascii=False, indent=2), encoding="utf-8"
)
print("ok", "P0" in goal or "REQ-RDG-REMOTE-CONNECT-01" in goal)
