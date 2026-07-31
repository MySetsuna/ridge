# -*- coding: utf-8 -*-
from pathlib import Path
import re
import inspect

root = Path(r"C:\Users\12867\AppData\Roaming\uv\tools\notebooklm-mcp-cli\Lib\site-packages\notebooklm_tools")
print("=== files mentioning conversation list-ish ===")
for p in root.rglob("*.py"):
    s = p.read_text(encoding="utf-8", errors="ignore")
    if not re.search(r"conversation|chat.?history|ListChat|GetConversation", s, re.I):
        continue
    # method defs
    defs = re.findall(r"def\s+(\w*(?:conversation|chat|history)\w*)\s*\(", s, re.I)
    lits = re.findall(
        r"['\"]([^'\"]*(?:[Cc]onversation|[Cc]hat[Hh]istory|ListChat)[^'\"]{0,40})['\"]",
        s,
    )
    if defs or lits:
        print(p.relative_to(root))
        if defs:
            print("  defs:", sorted(set(defs))[:30])
        if lits:
            print("  lits:", sorted(set(lits))[:30])

# try client public methods
print("\n=== client methods ===")
try:
    from notebooklm_tools.core.client import NotebookLMClient

    methods = [m for m in dir(NotebookLMClient) if not m.startswith("_")]
    print([m for m in methods if re.search(r"conv|chat|hist|query|note", m, re.I)])
except Exception as e:
    print("client import fail", e)

# auth load + try undocumented
print("\n=== try list conversations via client ===")
try:
    from notebooklm_tools import NotebookLMClient  # type: ignore
except Exception:
    try:
        from notebooklm_tools.core.client import NotebookLMClient
    except Exception as e:
        print("no client", e)
        raise SystemExit(0)

# find how to construct authenticated client
import notebooklm_tools
print("pkg", notebooklm_tools.__file__)
print([x for x in dir(notebooklm_tools) if not x.startswith("_")])
