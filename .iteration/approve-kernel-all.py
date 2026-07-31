# -*- coding: utf-8 -*-
from pathlib import Path
import json, re, subprocess, sys

root = Path(r"C:\code\wind")
skill = Path(r"C:\Users\12867\.grok\skills\notebooklm-iteration-loop")
sys.path.insert(0, str(skill / "scripts"))
from requirements_store import apply_operation, read_records

evidence = "全数批准"
request = (
    "用户全数批准内核相关 Pending：\n"
    "PENDING-REQ-RIDGE-KERNEL-HOST-01、PENDING-REQ-RIDGE-KERNEL-DOMAIN-01、"
    "PENDING-REQ-RIDGE-MCP-AS-KERNEL-API-01。\n"
    f"{evidence}\n"
)
(root / ".iteration" / "request.txt").write_text(request, encoding="utf-8")

pending_ids = [
    "PENDING-REQ-RIDGE-KERNEL-HOST-01",
    "PENDING-REQ-RIDGE-KERNEL-DOMAIN-01",
    "PENDING-REQ-RIDGE-MCP-AS-KERNEL-API-01",
]

# 1) pending intake (draft bound)
pending_decision = {
    "schema_version": 1,
    "intake_id": "INTAKE-20260731-KERNEL-LIFECYCLE-P",
    "classification": "pending",
    "summary": "Bind kernel lifecycle pendings for approval",
    "requirement_ids": [],
    "pending_ids": pending_ids,
    "approval_reasons": ["user-full-approve-kernel-lifecycle"],
    "open_questions": [],
    "approval_evidence": "",
}
(root / ".iteration" / "intake-decision-kernel-p.json").write_text(
    json.dumps(pending_decision, ensure_ascii=False, indent=2), encoding="utf-8"
)
r0 = subprocess.run(
    [
        sys.executable,
        str(skill / "scripts" / "requirements_intake.py"),
        "build",
        "--request-file",
        str(root / ".iteration" / "request.txt"),
        "--decision",
        str(root / ".iteration" / "intake-decision-kernel-p.json"),
        "--intake-file",
        str(root / ".iteration" / "intakes" / "INTAKE-20260731-KERNEL-LIFECYCLE-P.json"),
    ],
    capture_output=True,
    text=True,
    encoding="utf-8",
)
print("PENDING_INTAKE", r0.returncode, r0.stdout.strip()[:300])

# 2) promote: pending markdown -> active REQ-*
pending_recs = read_records(root / "docs" / "PENDING-REQUIREMENTS.md", "pending")
upserts = []
removals = []
id_map = {
    "PENDING-REQ-RIDGE-KERNEL-HOST-01": "REQ-RIDGE-KERNEL-HOST-01",
    "PENDING-REQ-RIDGE-KERNEL-DOMAIN-01": "REQ-RIDGE-KERNEL-DOMAIN-01",
    "PENDING-REQ-RIDGE-MCP-AS-KERNEL-API-01": "REQ-RIDGE-MCP-AS-KERNEL-API-01",
}
for pid, aid in id_map.items():
    rec = pending_recs[pid]
    md = rec["markdown"]
    # convert heading id
    md = md.replace(f"### {pid}", f"### {aid}", 1)
    # convert pending fields to active required fields
    # Keep body; inject ACTIVE status/version/behavior from pending labels
    def field(label):
        for line in md.splitlines():
            s = line.strip()
            if s.startswith(f"- {label}"):
                return s[len(f"- {label}") :].strip()
        return ""

    title_line = md.splitlines()[0]
    behavior = field("目标行为:")
    boundary_parts = [
        field("范围:"),
        field("非目标:"),
        field("不可动边界:"),
    ]
    boundary = " ".join(p for p in boundary_parts if p)
    acceptance = field("确定性验收:")
    track = field("预期追踪:")
    intent = field("原始意图:")
    active_md = "\n".join(
        [
            title_line,
            "",
            "- 状态:`ACTIVE`",
            "- 版本:`v1`",
            f"- 行为:{behavior}",
            f"- 边界:{boundary} 原始意图摘要:{intent[:200]}",
            f"- 验收:{acceptance}",
            f"- 追踪:{track}",
        ]
    )
    upserts.append({"id": aid, "section": "active", "markdown": active_md})
    removals.append(pid)

op = {"schema_version": 1, "upsert": upserts, "remove": removals}
(root / ".iteration" / "approve-kernel-lifecycle-op.json").write_text(
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
active = read_records(root / "docs" / "REQUIREMENTS-SPEC.md", "active")
for aid in id_map.values():
    print("active has", aid, aid in active)

# 3) ledger
spec_path = root / "docs" / "REQUIREMENTS-SPEC.md"
spec = spec_path.read_text(encoding="utf-8")
rows = (
    "| v0.3.6 | 2026-07-31 | `PENDING-REQ-RIDGE-KERNEL-HOST-01` | 内核进程与外壳生命周期（深根模式）转 Active | 新增 `REQ-RIDGE-KERNEL-HOST-01` | 用户明确「全数批准」 |\n"
    "| v0.3.6 | 2026-07-31 | `PENDING-REQ-RIDGE-KERNEL-DOMAIN-01` | 领域能力 SSOT 在内核转 Active | 新增 `REQ-RIDGE-KERNEL-DOMAIN-01` | 同上 |\n"
    "| v0.3.6 | 2026-07-31 | `PENDING-REQ-RIDGE-MCP-AS-KERNEL-API-01` | ridge-mcp 接内核面转 Active | 新增 `REQ-RIDGE-MCP-AS-KERNEL-API-01` | 同上 |\n"
)
if "REQ-RIDGE-KERNEL-HOST-01` | 内核进程" not in spec:
    idx = spec.find("| --- | --- | --- | --- | --- | --- |")
    if idx < 0:
        raise SystemExit("ledger missing")
    end = spec.find("\n", idx) + 1
    spec = spec[:end] + rows + spec[end:]
    spec_path.write_text(spec, encoding="utf-8")
    print("LEDGER ok")
else:
    print("LEDGER skip")

# 4) approved intake
approved = {
    "schema_version": 1,
    "intake_id": "INTAKE-20260731-KERNEL-LIFECYCLE-A",
    "classification": "approved",
    "summary": "Approved kernel host/domain/mcp REQs; ready to implement",
    "requirement_ids": list(id_map.values()),
    "pending_ids": pending_ids,
    "approval_reasons": [],
    "open_questions": [],
    "approval_evidence": evidence,
}
(root / ".iteration" / "intake-decision-kernel-a.json").write_text(
    json.dumps(approved, ensure_ascii=False, indent=2), encoding="utf-8"
)
r1 = subprocess.run(
    [
        sys.executable,
        str(skill / "scripts" / "requirements_intake.py"),
        "build",
        "--request-file",
        str(root / ".iteration" / "request.txt"),
        "--decision",
        str(root / ".iteration" / "intake-decision-kernel-a.json"),
        "--intake-file",
        str(root / ".iteration" / "intakes" / "INTAKE-20260731-KERNEL-LIFECYCLE-A.json"),
        "--previous-intake",
        str(root / ".iteration" / "intakes" / "INTAKE-20260731-KERNEL-LIFECYCLE-P.json"),
    ],
    capture_output=True,
    text=True,
    encoding="utf-8",
)
print("APPROVED_INTAKE", r1.returncode)
print(r1.stdout)
print(r1.stderr)

r2 = subprocess.run(
    [
        sys.executable,
        str(skill / "scripts" / "requirements_gate.py"),
        "assert-task-executable",
        "--file",
        str(root / "docs" / "REQUIREMENTS-SPEC.md"),
        "--pending-file",
        str(root / "docs" / "PENDING-REQUIREMENTS.md"),
        "--request-file",
        str(root / ".iteration" / "request.txt"),
        "--intake-file",
        str(root / ".iteration" / "intakes" / "INTAKE-20260731-KERNEL-LIFECYCLE-A.json"),
        "--json",
    ],
    capture_output=True,
    text=True,
    encoding="utf-8",
)
print("GATE", r2.returncode)
print(r2.stdout)