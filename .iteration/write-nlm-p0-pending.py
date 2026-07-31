# -*- coding: utf-8 -*-
from pathlib import Path
import json, subprocess, sys

root = Path(r"C:\code\wind")
skill = Path(r"C:\Users\12867\.grok\skills\notebooklm-iteration-loop")
sys.path.insert(0, str(skill / "scripts"))
from requirements_store import apply_operation, read_records

items = [
(
"PENDING-REQ-R-CDP-150-01",
"WebView2 150 CDP 工具链恢复（R-CDP-150）",
"""### PENDING-REQ-R-CDP-150-01 · WebView2 150 CDP 工具链恢复（R-CDP-150）

- 类型:FIX
- 原始意图:NLM 开放规划 note 4b8db248 P0：WebView2 150 弃用 WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS 注入导致 DevToolsActivePort 不生成，dev CDP 自动化断；须先修验证工具链以解锁一切真机证据
- 关联 Active 条款:REQ-20260730-01、REQ-NLM-OPENPLAN-01、REQ-RDG-REMOTE-CONNECT-01
- 目标行为:开发构建路径可稳定启用 CDP（候选 tauri.conf app.windows[].additionalBrowserArguments 注入 dev 专用）；pnpm tauri:dev:cdp 打印 CDP ready；cdp:smoke exit 0
- 范围:src-tauri tauri 窗口/WebView2 启动参数、package.json tauri:dev:cdp 与 cdp:smoke 脚本、相关文档
- 非目标:不改业务 Remote 协议；不替代 R-VERIFY 真机矩阵本身
- 不可动边界:不得把生产安装包默认开远程调试端口；密钥/cookie 不入仓
- 假设/待确认:CDP 端口与现有脚本约定是否需随 Chromium 150 调整
- 确定性验收:pnpm tauri:dev:cdp 打印 CDP ready；cdp:smoke exit 0；证据写入 checklist
- 预期追踪:PENDING-REQ-R-CDP-150-01 → note 4b8db248 R-CDP-150 → tauri conf / cdp scripts
"""
),
(
"PENDING-REQ-R-VERIFY-01",
"P4 手机保活真机验 R1–R8（R-VERIFY）",
"""### PENDING-REQ-R-VERIFY-01 · P4 手机保活真机验 R1–R8（R-VERIFY）

- 类型:FIX
- 原始意图:NLM 开放规划 note 4b8db248 P0：P4 手机保活 R1–R8 真机验 + iter-60 G2/G3 remote 腿交互证据仍缺；本机 WebView2 150 CDP 失效阻塞证据采集
- 关联 Active 条款:REQ-MOBILE-REMOTE-STATE-01、REQ-REMOTE-SMOOTH-STATE-02、REQ-NLM-OPENPLAN-01
- 目标行为:R1–R8 CDP/真机证据落入 evidence JSON；validate-remote-smoke-evidence.mjs exit 0；回归可退基线 b9031a0
- 范围:mobile remote 保活/订阅/切回、证据脚本与 runbook、依赖 R-CDP-150 的采集路径
- 非目标:不重做 P4 架构；不以 mock 假绿替代真机/受控 CDP 证据
- 不可动边界:不伪造 NLM/证据；不自动发版
- 假设/待确认:R-CDP-150 通过后方可全量采证；无真机时须明确 blocked 分项
- 确定性验收:evidence JSON 过 validate-remote-smoke-evidence.mjs exit 0；checklist 逐 R1–R8 标注 pass/blocked+原因
- 预期追踪:PENDING-REQ-R-VERIFY-01 → note 4b8db248 R-VERIFY → evidence JSON
"""
),
(
"PENDING-REQ-R-INCR-01",
"增量 replay 前端激活（R-INCR）",
"""### PENDING-REQ-R-INCR-01 · 增量 replay 前端激活（R-INCR）

- 类型:FIX
- 原始意图:NLM 开放规划 note 4b8db248 P0：增量 replay 前端激活，消除 resume-live-only gap
- 关联 Active 条款:REQ-REMOTE-SMOOTH-STATE-02、REQ-MOBILE-REMOTE-STATE-01、REQ-NLM-OPENPLAN-01
- 目标行为:切走→后台产出→切回历史连续无 gap、无重复；host since() 边界与前端游标有纯逻辑测
- 范围:packages/remote 与 src/remote 订阅/游标、host since 边界、rdg ScrollbackRing since 对接可后续并行 R-RDG-INCR
- 非目标:不重做全量 resync SSOT；不引入第二连接绕过调度
- 不可动边界:输入/control 不得被历史页阻塞；取消须归零队列计数
- 假设/待确认:R-RDG-INCR 是否同轮最小并联
- 确定性验收:切走产出切回无 gap 无重复的确定性测或受控 E2E；since/游标单测绿
- 预期追踪:PENDING-REQ-R-INCR-01 → note 4b8db248 R-INCR → remote since/cursor tests
"""
),
]

# clear any existing pending of these ids first if present
pending_path = root / "docs" / "PENDING-REQUIREMENTS.md"
active_path = root / "docs" / "REQUIREMENTS-SPEC.md"
existing = read_records(pending_path, "pending")
remove = [i for i,_,_ in items if i in existing]
upsert = [{"id": i, "section": "pending", "markdown": md.strip()} for i,_,md in items]
op = {"schema_version": 1, "upsert": upsert, "remove": remove}
(root / ".iteration" / "nlm-open-p0-pending-op.json").write_text(
    json.dumps(op, ensure_ascii=False, indent=2), encoding="utf-8"
)
# pending-only write: evidence not required for pending section only
# but apply_operation requires evidence if any active - we're only pending
apply_operation(active_path, pending_path, op, evidence=None)
print("pending ids", list(read_records(pending_path, "pending").keys()))

# pending intake
decision = {
  "schema_version": 1,
  "intake_id": "INTAKE-20260731-NLM-P0",
  "classification": "pending",
  "summary": "NLM openplan P0 three items as next iteration Pending",
  "requirement_ids": [],
  "pending_ids": [i for i,_,_ in items],
  "approval_reasons": [
    "user-requested-next-iter-from-nlm-latest",
    "openplan-note-4b8db248-p0-order",
  ],
  "open_questions": [],
  "approval_evidence": "",
}
(root / ".iteration" / "intake-decision-nlm-p0.json").write_text(
    json.dumps(decision, ensure_ascii=False, indent=2), encoding="utf-8"
)
r = subprocess.run([
  sys.executable, str(skill/"scripts"/"requirements_intake.py"), "build",
  "--request-file", str(root/".iteration"/"request.txt"),
  "--decision", str(root/".iteration"/"intake-decision-nlm-p0.json"),
  "--intake-file", str(root/".iteration"/"intakes"/"INTAKE-20260731-NLM-P0.json"),
  "--file", str(active_path),
  "--pending-file", str(pending_path),
], capture_output=True, text=True, encoding="utf-8")
print("INTAKE", r.returncode)
print(r.stdout)
print(r.stderr)
print((root/".iteration"/"intakes"/"INTAKE-20260731-NLM-P0.json").read_text(encoding="utf-8"))