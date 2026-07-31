# -*- coding: utf-8 -*-
from pathlib import Path
import json
import subprocess
import sys

root = Path(r"C:\code\wind")
skill = Path(r"C:\Users\12867\.grok\skills\notebooklm-iteration-loop")
sys.path.insert(0, str(skill / "scripts"))
from requirements_store import apply_operation, read_records

evidence = "我批准所有项"
request = (
    "用户批准全部 Pending 项：PENDING-REQ-MCP-JOIN-GROUP-01、PENDING-REQ-NLM-OPENPLAN-01。\n"
    f"{evidence}\n"
    "提升为 Active 后继续 Goal（join_group 实现 + openplan 追踪）。\n"
)
(root / ".iteration" / "request.txt").write_text(request, encoding="utf-8")

# 1) pending intake (draft shown)
pending_decision = {
    "schema_version": 1,
    "intake_id": "INTAKE-20260731-PENDING-ALL",
    "classification": "pending",
    "summary": "Approve all pending join_group and nlm openplan",
    "requirement_ids": [],
    "pending_ids": [
        "PENDING-REQ-MCP-JOIN-GROUP-01",
        "PENDING-REQ-NLM-OPENPLAN-01",
    ],
    "approval_reasons": [
        "user-explicit-approve-all-pending",
        "goal-residual-gate-unblock",
    ],
    "open_questions": [],
    "approval_evidence": "",
}
(root / ".iteration" / "intake-decision-pending-all.json").write_text(
    json.dumps(pending_decision, ensure_ascii=False, indent=2), encoding="utf-8"
)
r = subprocess.run(
    [
        sys.executable,
        str(skill / "scripts" / "requirements_intake.py"),
        "build",
        "--request-file", str(root / ".iteration" / "request.txt"),
        "--decision", str(root / ".iteration" / "intake-decision-pending-all.json"),
        "--intake-file", str(root / ".iteration" / "intakes" / "INTAKE-20260731-PENDING-ALL.json"),
        "--file", str(root / "docs" / "REQUIREMENTS-SPEC.md"),
        "--pending-file", str(root / "docs" / "PENDING-REQUIREMENTS.md"),
    ],
    capture_output=True,
    text=True,
    encoding="utf-8",
)
print("PENDING_INTAKE", r.returncode, r.stdout.strip(), r.stderr.strip())

# 2) promote to Active
join_md = """### REQ-MCP-JOIN-GROUP-01 · ridge_join_group 参数/宿主校验与可观测落地

- 状态:`ACTIVE`
- 版本:`v1`
- 行为:桌面 host 在 `group_name`+(agent_id|target_pane_id) 合法且目标在花名册时 emit TEAMMATE_GROUP_ADD_MEMBER 并由前端编组 store 消费；非法参数稳定 -32602+可读 message；companion-only 宿主返回明确 capability 错误（非 silent OK），文档与 tools/list 一致；fire-and-forget+前端 localStorage SSOT 为已知限制直至另批迁后端。
- 边界:范围 packages/ridge-mcp、src-tauri teammate mcp/join_group 桥、AgentCenterPanel 编组加成员、docs/mcp-integration。非目标:不重做整套编组 SSOT 迁后端。不可动:不得静默落到错误 pane/0 号分屏；越界 target 必须失败。
- 验收:合法/非法/companion 三路径有 MCP 调用证据；失败码与 message 写入 checklist；非法不得 silent OK。
- 追踪:`REQ-MCP-JOIN-GROUP-01` → mcp.rs join_group → AgentCenterPanel group event → SCRATCH smoke
"""

open_md = """### REQ-NLM-OPENPLAN-01 · 开放规划 post-v0.1.3 优先项入迭代队列

- 状态:`ACTIVE`
- 版本:`v1`
- 行为:将 NLM 开放规划 post-v0.1.3 中 R-VERIFY、R-CDP-150、R-INCR、R-WSLEG、R-RDG-INCR 等开放项登记为可跟踪迭代条目，并在 checklist 标注来源笔记本 66919cb9 与 note 4b8db248；与 Active goal 对账后按优先级推进。
- 边界:范围 docs/PENDING 已提升后的 Active 追踪、iteration checklist、PROJECT-STATE 索引；不在本条内强制完成全部真机验收。非目标:不把已实现闭环索引 note 当未完成需求。NLM 仅建议层；验收仍以代码/测试/证据为准。
- 验收:nlm-extracted-requirements.md 或等价证据含开放规划表；checklist 有来源标注；本条 Active 存在且可被 gate 引用。
- 追踪:`REQ-NLM-OPENPLAN-01` → nlm note 4b8db248 → checklist / PROJECT-STATE
"""

op = {
    "schema_version": 1,
    "upsert": [
        {"id": "REQ-MCP-JOIN-GROUP-01", "section": "active", "markdown": join_md.strip()},
        {"id": "REQ-NLM-OPENPLAN-01", "section": "active", "markdown": open_md.strip()},
    ],
    "remove": [
        "PENDING-REQ-MCP-JOIN-GROUP-01",
        "PENDING-REQ-NLM-OPENPLAN-01",
    ],
}
(root / ".iteration" / "approve-all-operation.json").write_text(
    json.dumps(op, ensure_ascii=False, indent=2), encoding="utf-8"
)
apply_operation(
    root / "docs" / "REQUIREMENTS-SPEC.md",
    root / "docs" / "PENDING-REQUIREMENTS.md",
    op,
    evidence=evidence,
)
print("PROMOTE ok")
print("pending left", list(read_records(root / "docs" / "PENDING-REQUIREMENTS.md", "pending")))
print("active has join", "REQ-MCP-JOIN-GROUP-01" in read_records(root / "docs" / "REQUIREMENTS-SPEC.md", "active"))
print("active has open", "REQ-NLM-OPENPLAN-01" in read_records(root / "docs" / "REQUIREMENTS-SPEC.md", "active"))

# 3) append ledger rows
spec = (root / "docs" / "REQUIREMENTS-SPEC.md").read_text(encoding="utf-8")
ledger_rows = (
    "| v0.3.5 | 2026-07-31 | `PENDING-REQ-MCP-JOIN-GROUP-01` | ridge_join_group 参数/宿主校验转 Active | 新增 `REQ-MCP-JOIN-GROUP-01` | 用户明确「我批准所有项」 |\n"
    "| v0.3.5 | 2026-07-31 | `PENDING-REQ-NLM-OPENPLAN-01` | NLM 开放规划优先项入 Active 追踪 | 新增 `REQ-NLM-OPENPLAN-01` | 同上 |\n"
)
marker = "| 版本 | 日期 | Pending ID | 变更 | 关联/取代 | 批准证据 |\n| --- | --- | --- | --- | --- | --- |\n"
if marker in spec and "REQ-MCP-JOIN-GROUP-01" not in spec.split("## 修订账本")[-1][:800]:
    # insert after header row
    pass
if "PENDING-REQ-MCP-JOIN-GROUP-01` | ridge_join_group" not in spec:
    # find first data row after ledger header and prepend
    idx = spec.find("| --- | --- | --- | --- | --- | --- |")
    if idx < 0:
        raise SystemExit("ledger header not found")
    end = spec.find("\n", idx) + 1
    spec = spec[:end] + ledger_rows + spec[end:]
    (root / "docs" / "REQUIREMENTS-SPEC.md").write_text(spec, encoding="utf-8")
    print("LEDGER appended")
else:
    print("LEDGER already present")

# 4) approved intake
approved_decision = {
    "schema_version": 1,
    "intake_id": "INTAKE-20260731-APPROVED-ALL",
    "classification": "approved",
    "summary": "Approved join_group and nlm openplan; continue Goal",
    "requirement_ids": [
        "REQ-MCP-JOIN-GROUP-01",
        "REQ-NLM-OPENPLAN-01",
        "REQ-AGENT-CATALOG-01",
        "REQ-20260730-01",
        "REQ-RDG-REMOTE-CONNECT-01",
    ],
    "pending_ids": [
        "PENDING-REQ-MCP-JOIN-GROUP-01",
        "PENDING-REQ-NLM-OPENPLAN-01",
    ],
    "approval_reasons": [],
    "open_questions": [],
    "approval_evidence": evidence,
}
(root / ".iteration" / "intake-decision-approved-all.json").write_text(
    json.dumps(approved_decision, ensure_ascii=False, indent=2), encoding="utf-8"
)
r2 = subprocess.run(
    [
        sys.executable,
        str(skill / "scripts" / "requirements_intake.py"),
        "build",
        "--request-file", str(root / ".iteration" / "request.txt"),
        "--decision", str(root / ".iteration" / "intake-decision-approved-all.json"),
        "--intake-file", str(root / ".iteration" / "intakes" / "INTAKE-20260731-APPROVED-ALL.json"),
        "--previous-intake", str(root / ".iteration" / "intakes" / "INTAKE-20260731-PENDING-ALL.json"),
        "--file", str(root / "docs" / "REQUIREMENTS-SPEC.md"),
        "--pending-file", str(root / "docs" / "PENDING-REQUIREMENTS.md"),
    ],
    capture_output=True,
    text=True,
    encoding="utf-8",
)
print("APPROVED_INTAKE", r2.returncode, r2.stdout.strip(), r2.stderr.strip())
print((root / ".iteration" / "intakes" / "INTAKE-20260731-APPROVED-ALL.json").read_text(encoding="utf-8"))

r3 = subprocess.run(
    [
        sys.executable,
        str(skill / "scripts" / "requirements_gate.py"),
        "assert-task-executable",
        "--file", str(root / "docs" / "REQUIREMENTS-SPEC.md"),
        "--pending-file", str(root / "docs" / "PENDING-REQUIREMENTS.md"),
        "--request-file", str(root / ".iteration" / "request.txt"),
        "--intake-file", str(root / ".iteration" / "intakes" / "INTAKE-20260731-APPROVED-ALL.json"),
        "--json",
    ],
    capture_output=True,
    text=True,
    encoding="utf-8",
)
print("GATE", r3.returncode)
print(r3.stdout)
print(r3.stderr)