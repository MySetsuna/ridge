#!/usr/bin/env python3
"""Test fake_acp_agent by sending properly framed JSON-RPC requests."""
import json
import subprocess
import sys
import time


def frame(obj):
    body = json.dumps(obj).encode("utf-8")
    return b"Content-Length: " + str(len(body)).encode() + b"\r\n\r\n" + body


def main():
    p = subprocess.Popen(
        ["./target/debug/fake_acp_agent.exe"],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    reqs = [
        {"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocol_version": "v1", "client": {}}},
        {"jsonrpc": "2.0", "id": 2, "method": "session/new", "params": {}},
        {"jsonrpc": "2.0", "id": 3, "method": "session/prompt", "params": {"session_id": "sess-x", "prompt": [{"kind": "text", "text": "hi"}]}},
    ]
    for r in reqs:
        p.stdin.write(frame(r))
    p.stdin.flush()
    time.sleep(0.5)
    # Send approve
    p.stdin.write(frame({"jsonrpc": "2.0", "method": "session/approve", "params": {"session_id": "sess-x", "approved": True}}))
    p.stdin.flush()
    time.sleep(0.5)
    out, err = p.communicate(timeout=5)
    print("=== STDOUT ===")
    sys.stdout.write(out.decode("utf-8", errors="replace"))
    print("\n=== STDERR ===")
    sys.stdout.write(err.decode("utf-8", errors="replace"))


if __name__ == "__main__":
    main()
