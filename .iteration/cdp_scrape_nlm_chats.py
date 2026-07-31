# -*- coding: utf-8 -*-
"""Scrape NotebookLM chat turns via Chrome CDP (no cookies printed)."""
from __future__ import annotations

import json
import sys
import time
import urllib.request
from pathlib import Path

try:
    import websocket  # type: ignore
except ImportError:
    import subprocess

    subprocess.check_call([sys.executable, "-m", "pip", "install", "websocket-client", "-q"])
    import websocket  # type: ignore

CDP = "http://127.0.0.1:19222"
NB = "66919cb9-1329-4ddf-955c-f426d15a9fe6"
URLS = [
    f"https://notebooklm.google.com/notebook/{NB}",
    f"https://notebook.google.com/notebook/{NB}",
]
OUT = Path(r"C:\code\wind\.iteration\nlm-latest-chats.json")


def get_json(path: str):
    with urllib.request.urlopen(CDP + path, timeout=10) as r:
        return json.loads(r.read().decode("utf-8"))


def main() -> int:
    pages = get_json("/json/list")
    target = None
    for p in pages:
        u = p.get("url") or ""
        if p.get("type") != "page":
            continue
        if NB in u:
            target = p
            break
        if "notebook.google.com" in u or "notebooklm.google.com" in u:
            target = target or p  # first NLM page as fallback
    if not target:
        print("no notebook page in CDP; open NotebookLM in the CDP Chrome first")
        for p in pages:
            print(" -", p.get("type"), p.get("url"))
        return 2

    ws_url = target["webSocketDebuggerUrl"]
    print("using", target.get("title"), target.get("url"))
    ws = websocket.create_connection(ws_url, timeout=60)
    msg_id = 0

    def call(method: str, params=None, timeout=60):
        nonlocal msg_id
        msg_id += 1
        payload = {"id": msg_id, "method": method}
        if params is not None:
            payload["params"] = params
        ws.send(json.dumps(payload))
        deadline = time.time() + timeout
        while time.time() < deadline:
            raw = ws.recv()
            data = json.loads(raw)
            if data.get("id") == msg_id:
                return data
        raise TimeoutError(method)

    call("Page.enable")
    call("Runtime.enable")
    # Prefer notebook.google.com (live host) then notebooklm
    want = f"https://notebook.google.com/notebook/{NB}"
    cur = target.get("url") or ""
    if NB not in cur:
        print("navigate", want)
        call("Page.navigate", {"url": want})
        time.sleep(8)
    else:
        print("already on notebook")
        time.sleep(2)

    # Wait for app shell
    for _ in range(20):
        ready = call(
            "Runtime.evaluate",
            {
                "expression": "document.body && document.body.innerText && document.body.innerText.length > 200",
                "returnByValue": True,
            },
        )
        if ready.get("result", {}).get("result", {}).get("value"):
            break
        time.sleep(1)

    # Extract chat-like turns from DOM text nodes / role labels
    expr = r"""
(() => {
  const body = document.body ? document.body.innerText : '';
  // Collect message bubbles: look for common patterns in NotebookLM chat
  const candidates = [];
  const nodes = Array.from(document.querySelectorAll('[data-message-author], [data-turn], [role="listitem"], .chat-message, [class*="message"], [class*="turn"], [class*="query"], [class*="response"]'));
  for (const n of nodes) {
    const t = (n.innerText || '').trim();
    if (t.length < 8) continue;
    const role = n.getAttribute('data-message-author')
      || n.getAttribute('data-role')
      || (t.startsWith('You') ? 'user' : '');
    candidates.push({role, text: t.slice(0, 4000), cls: (n.className||'').toString().slice(0,120)});
  }
  // Fallback: split full body by user markers if DOM structure opaque
  const lines = body.split(/\n+/).map(s => s.trim()).filter(Boolean);
  return {
    url: location.href,
    title: document.title,
    bodyLen: body.length,
    bodyHead: body.slice(0, 3000),
    bodyTail: body.slice(-6000),
    candidateCount: candidates.length,
    candidates: candidates.slice(-80),
    lineCount: lines.length,
    lastLines: lines.slice(-120),
  };
})()
"""
    res = call("Runtime.evaluate", {"expression": expr, "returnByValue": True, "awaitPromise": False})
    value = res.get("result", {}).get("result", {}).get("value")
    if value is None:
        print("eval failed", json.dumps(res, ensure_ascii=False)[:1000])
        ws.close()
        return 3

    OUT.write_text(json.dumps(value, ensure_ascii=False, indent=2), encoding="utf-8")
    print("wrote", OUT, "bodyLen", value.get("bodyLen"), "candidates", value.get("candidateCount"))
    print("title", value.get("title"))
    print("url", value.get("url"))
    print("--- tail ---")
    print("\n".join(value.get("lastLines") or [])[-2000:])
    ws.close()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
