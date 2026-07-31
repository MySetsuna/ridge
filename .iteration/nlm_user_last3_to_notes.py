# -*- coding: utf-8 -*-
"""Save last 3 *user* product Q&A (excluding automation self-queries) as Notes."""
from __future__ import annotations

import asyncio
import json
import os
from pathlib import Path

os.environ.setdefault("HTTP_PROXY", "http://127.0.0.1:51081")
os.environ.setdefault("HTTPS_PROXY", "http://127.0.0.1:51081")
os.environ.setdefault("NO_PROXY", "127.0.0.1,localhost")

NB = "66919cb9-1329-4ddf-955c-f426d15a9fe6"
OUT = Path(r"C:\code\wind\.iteration")
STORAGE = OUT / "notebooklm-py-storage_state.json"
PAIRS = OUT / "nlm-conversation-qa-pairs.json"
CREATED = OUT / "nlm-user-last3-notes.json"
MD = OUT / "nlm-user-last3-notes.md"

SKIP_PREFIXES = (
    "请仅根据本笔记本",
    "从笔记本笔记与来源中",
    "仅据所选两源",
    "基于当前 PROJECT-STATE",
    "重新审计截至",
    "基于当前 Notebook",
    "按当前两份来源",
)


async def main() -> int:
    from notebooklm import NotebookLMClient

    qa = json.loads(PAIRS.read_text(encoding="utf-8"))
    # oldest-first; take last 3 that are not automation prompts
    user = []
    for item in reversed(qa):
        q = (item.get("question") or "").strip()
        if any(q.startswith(p) for p in SKIP_PREFIXES):
            continue
        user.append(item)
        if len(user) == 3:
            break
    user = list(reversed(user))
    if len(user) < 3:
        print("only found", len(user), "user pairs")
    print("selected", [u["i"] for u in user])

    async with NotebookLMClient.from_storage(str(STORAGE)) as client:
        created = []
        for idx, item in enumerate(user, 1):
            q = item["question"].strip()
            a = item["answer"].strip()
            title = f"[对话·用户] {q[:48]}".replace("\n", " ")
            body = (
                f"# 用户对话 {idx}/3（history export, no CDP）\n\n"
                f"- notebook: `{NB}`\n"
                f"- history_index: {item['i']}\n\n"
                f"## USER\n\n{q}\n\n"
                f"## NLM\n\n{a}\n"
            )
            note = await client.notes.create(NB, title=title, content=body)
            created.append(
                {
                    "history_index": item["i"],
                    "note_id": note.id,
                    "title": title,
                    "question": q,
                    "answer": a,
                }
            )
            print("note", note.id, title[:70])

        CREATED.write_text(json.dumps(
            [{k: v for k, v in c.items() if k != "answer"} | {"answer_len": len(c["answer"])}
             for c in created],
            ensure_ascii=False,
            indent=2,
        ), encoding="utf-8")

        lines = ["# User last-3 conversations as Notes\n"]
        for c in created:
            lines.append(f"## {c['title']}\n")
            lines.append(f"note_id: `{c['note_id']}` history_index: {c['history_index']}\n")
            lines.append("### USER\n")
            lines.append(c["question"] + "\n")
            lines.append("### NLM\n")
            lines.append(c["answer"] + "\n")
        MD.write_text("\n".join(lines), encoding="utf-8")
        print("wrote", MD)
    return 0


if __name__ == "__main__":
    raise SystemExit(asyncio.run(main()))
