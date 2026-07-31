# -*- coding: utf-8 -*-
"""Probe notebook payload + conversations raw for any chat turn text."""
from __future__ import annotations

import json
import os
import re
from pathlib import Path

os.environ.setdefault("HTTP_PROXY", "http://127.0.0.1:51081")
os.environ.setdefault("HTTPS_PROXY", "http://127.0.0.1:51081")
os.environ.setdefault("NO_PROXY", "127.0.0.1,localhost")

from notebooklm_tools.cli.utils import get_client

NB = "66919cb9-1329-4ddf-955c-f426d15a9fe6"
OUT = Path(r"C:\code\wind\.iteration\nlm-notebook-probe.json")


def all_strings(obj, acc=None):
    if acc is None:
        acc = []
    if isinstance(obj, str):
        if len(obj) >= 20:
            acc.append(obj[:3000])
    elif isinstance(obj, list):
        for x in obj:
            all_strings(x, acc)
    elif isinstance(obj, dict):
        for v in obj.values():
            all_strings(v, acc)
    return acc


def main() -> int:
    client = get_client("default")
    nb = client.get_notebook(NB)
    conv = client._call_rpc(
        client.RPC_GET_CONVERSATIONS,
        [[], None, NB, 50],
        path=f"/notebook/{NB}",
    )
    # Try alternate param shapes sometimes seen in web clients
    alts = []
    for params in (
        [NB],
        [NB, 20],
        [None, NB, 20],
        [[NB], None, 20],
        [[], NB, 20],
        [[[]], None, NB, 50, None, 1],
    ):
        try:
            r = client._call_rpc(
                client.RPC_GET_CONVERSATIONS, params, path=f"/notebook/{NB}"
            )
            alts.append({"params": params, "raw": r})
        except Exception as e:
            alts.append({"params": params, "error": type(e).__name__ + ":" + str(e)[:200]})

    strings = all_strings(nb) + all_strings(conv)
    for a in alts:
        if "raw" in a:
            strings.extend(all_strings(a["raw"]))

    # Chinese-ish or question-like
    interesting = []
    for s in strings:
        if re.search(r"[\u4e00-\u9fff]", s) or "?" in s or "？" in s:
            if not s.startswith("http") and "google" not in s.lower():
                interesting.append(s[:500])

    payload = {
        "notebook_type": type(nb).__name__,
        "conv": conv,
        "alts": alts,
        "interesting_count": len(interesting),
        "interesting_sample": interesting[:40],
        "string_count": len(strings),
    }
    OUT.write_text(json.dumps(payload, ensure_ascii=False, indent=2, default=str), encoding="utf-8")
    print("wrote", OUT)
    print("interesting", len(interesting))
    for i, s in enumerate(interesting[:25]):
        print(f"[{i}]", s.replace("\n", " / ")[:200])
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
