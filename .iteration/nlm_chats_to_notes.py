# -*- coding: utf-8 -*-
"""
Fetch last N conversation Q&A via notebooklm-py GET_CONVERSATION_TURNS,
save as Notes, then list notes for capture.
No CDP. Uses existing notebooklm-mcp-cli profile cookies.
Does not print cookies/tokens.
"""
from __future__ import annotations

import asyncio
import json
import os
import sys
from pathlib import Path

os.environ.setdefault("HTTP_PROXY", "http://127.0.0.1:51081")
os.environ.setdefault("HTTPS_PROXY", "http://127.0.0.1:51081")
os.environ.setdefault("NO_PROXY", "127.0.0.1,localhost")

NB = "66919cb9-1329-4ddf-955c-f426d15a9fe6"
OUT_DIR = Path(r"C:\code\wind\.iteration")
STORAGE = OUT_DIR / "notebooklm-py-storage_state.json"
PAIRS_JSON = OUT_DIR / "nlm-conversation-qa-pairs.json"
NOTES_JSON = OUT_DIR / "nlm-conversation-notes-created.json"
MD = OUT_DIR / "nlm-conversation-as-notes.md"


def mcp_cli_cookies_to_storage_state() -> Path:
    # Read mcp-cli profile cookies file directly (no import of mcp package).
    cookies_path = Path.home() / ".notebooklm-mcp-cli" / "profiles" / "default" / "cookies.json"
    raw = json.loads(cookies_path.read_text(encoding="utf-8"))
    cookies_in = raw if isinstance(raw, list) else raw.get("cookies", raw)
    cookies_out = []
    if isinstance(cookies_in, dict):
        # name -> value map
        for name, value in cookies_in.items():
            cookies_out.append(
                {
                    "name": name,
                    "value": value,
                    "domain": ".google.com",
                    "path": "/",
                    "expires": -1,
                    "httpOnly": False,
                    "secure": True,
                    "sameSite": "None",
                }
            )
    else:
        for c in cookies_in:
            if not isinstance(c, dict) or not c.get("name"):
                continue
            cookies_out.append(
                {
                    "name": c["name"],
                    "value": c.get("value", ""),
                    "domain": c.get("domain") or ".google.com",
                    "path": c.get("path") or "/",
                    "expires": c.get("expires", -1) if c.get("expires") is not None else -1,
                    "httpOnly": bool(c.get("httpOnly", c.get("http_only", False))),
                    "secure": bool(c.get("secure", True)),
                    "sameSite": c.get("sameSite") or c.get("same_site") or "None",
                }
            )
    state = {"cookies": cookies_out, "origins": []}
    STORAGE.write_text(json.dumps(state), encoding="utf-8")
    try:
        os.chmod(STORAGE, 0o600)
    except Exception:
        pass
    return STORAGE


async def main() -> int:
    from notebooklm import NotebookLMClient

    storage = mcp_cli_cookies_to_storage_state()
    async with NotebookLMClient.from_storage(str(storage)) as client:
        # Most recent conversation history: get many turns then group into last 3 Q&A
        # User asked for 3 对话 — treat as last 3 Q&A turns (user+assistant pairs)
        qa = await client.chat.get_history(NB, limit=30)
        PAIRS_JSON.write_text(
            json.dumps(
                [{"i": i + 1, "question": q, "answer": a} for i, (q, a) in enumerate(qa)],
                ensure_ascii=False,
                indent=2,
            ),
            encoding="utf-8",
        )
        print("qa_pairs", len(qa))
        if not qa:
            print("EMPTY history — cannot create notes")
            return 2

        # last 3 pairs (newest are at end after parse which is oldest-first)
        last3 = qa[-3:] if len(qa) >= 3 else qa
        created = []
        for idx, (q, a) in enumerate(last3, 1):
            title = f"[对话] {q.strip()[:40]}".replace("\n", " ")
            body = (
                f"# 对话 {idx}/3（自 NotebookLM chat history 导出）\n\n"
                f"**Notebook:** `{NB}`\n\n"
                f"## USER\n\n{q.strip()}\n\n"
                f"## NLM\n\n{a.strip()}\n"
            )
            note = await client.notes.create(NB, content=body, title=title)
            created.append(
                {
                    "title": title,
                    "note_id": getattr(note, "id", None) or str(note),
                    "q_preview": q.strip()[:120],
                }
            )
            print("created_note", created[-1]["note_id"], title[:60])

        NOTES_JSON.write_text(json.dumps(created, ensure_ascii=False, indent=2), encoding="utf-8")

        # re-list notes
        notes = await client.notes.list(NB)
        lines = ["# Conversation → Notes capture", f"notebook: {NB}", f"history_pairs: {len(qa)}", ""]
        for n in notes:
            nid = getattr(n, "id", "")
            title = getattr(n, "title", "")
            lines.append(f"- {nid} · {title}")
        # get full content of the three we created
        lines.append("\n## Created conversation notes (full)\n")
        for c in created:
            nid = c["note_id"]
            try:
                full = await client.notes.get(NB, nid)
                content = getattr(full, "content", None) or getattr(full, "text", "") or str(full)
            except Exception as e:
                content = f"(get failed: {type(e).__name__})"
            lines.append(f"### {c['title']}\n\nid: `{nid}`\n\n{content}\n")

        MD.write_text("\n".join(lines), encoding="utf-8")
        print("wrote", MD)
    return 0


if __name__ == "__main__":
    raise SystemExit(asyncio.run(main()))
