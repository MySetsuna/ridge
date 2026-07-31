# -*- coding: utf-8 -*-
"""Fetch NotebookLM chat conversations via authenticated RPC (no CDP, no cookie dump)."""
from __future__ import annotations

import json
import os
import re
import sys
from pathlib import Path

os.environ.setdefault("HTTP_PROXY", "http://127.0.0.1:51081")
os.environ.setdefault("HTTPS_PROXY", "http://127.0.0.1:51081")
os.environ.setdefault("NO_PROXY", "127.0.0.1,localhost")

from notebooklm_tools.cli.utils import get_client

NB = "66919cb9-1329-4ddf-955c-f426d15a9fe6"
OUT = Path(r"C:\code\wind\.iteration\nlm-conversations-raw.json")
OUT_SUM = Path(r"C:\code\wind\.iteration\nlm-conversations-latest3.md")


def walk_strings(obj, path="$", out=None, max_n=500):
    if out is None:
        out = []
    if len(out) >= max_n:
        return out
    if isinstance(obj, str):
        if len(obj) >= 8 and not re.fullmatch(r"[0-9a-f-]{16,}", obj):
            out.append((path, obj[:2000]))
        return out
    if isinstance(obj, list):
        for i, v in enumerate(obj[:80]):
            walk_strings(v, f"{path}[{i}]", out, max_n)
    elif isinstance(obj, dict):
        for k, v in list(obj.items())[:40]:
            walk_strings(v, f"{path}.{k}", out, max_n)
    return out


def shape(obj, depth=0, max_depth=4):
    if depth >= max_depth:
        return type(obj).__name__
    if isinstance(obj, list):
        if not obj:
            return []
        return [shape(obj[0], depth + 1, max_depth), f"...len={len(obj)}"]
    if isinstance(obj, dict):
        return {k: shape(v, depth + 1, max_depth) for k, v in list(obj.items())[:12]}
    if isinstance(obj, str):
        return f"str:{len(obj)}"
    if obj is None:
        return None
    return type(obj).__name__


def main() -> int:
    client = get_client("default")
    # Full list: same RPC as get_conversation_id but keep raw payload
    raw = client._call_rpc(
        client.RPC_GET_CONVERSATIONS,
        [[], None, NB, 50],
        path=f"/notebook/{NB}",
    )
    OUT.write_text(
        json.dumps({"shape": shape(raw), "raw": raw}, ensure_ascii=False, indent=2, default=str),
        encoding="utf-8",
    )
    print("wrote", OUT)
    print("shape", json.dumps(shape(raw), ensure_ascii=False)[:800])

    strings = walk_strings(raw)
    # Prefer user-looking turns: longer Chinese/English Qs
    candidates = []
    for path, text in strings:
        t = text.strip()
        if len(t) < 12:
            continue
        # skip UI chrome-ish
        if t.startswith("http") or t.count("\n") > 80:
            continue
        candidates.append({"path": path, "text": t, "len": len(t)})

    # Heuristic: last 3 distinct user questions often appear as short-to-medium strings
    # Print top unique strings by reverse order of discovery (API often newest first)
    seen = set()
    unique = []
    for c in candidates:
        key = c["text"][:120]
        if key in seen:
            continue
        seen.add(key)
        unique.append(c)

    print("string_count", len(unique))
    for i, c in enumerate(unique[:30]):
        print(f"--- [{i}] path={c['path']} len={c['len']} ---")
        print(c["text"][:400].replace("\n", " / "))

    # Also try get_conversation_id path for single id
    cid = client.get_conversation_id(NB)
    print("primary_conversation_id", cid)

    OUT_SUM.write_text(
        "# Latest conversation strings (RPC get_conversations)\n\n"
        f"notebook: {NB}\n"
        f"primary_conversation_id: {cid}\n"
        f"unique_strings: {len(unique)}\n\n"
        + "\n\n".join(
            f"## [{i}]\npath: `{c['path']}`\n\n{c['text']}" for i, c in enumerate(unique[:40])
        ),
        encoding="utf-8",
    )
    print("wrote", OUT_SUM)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
