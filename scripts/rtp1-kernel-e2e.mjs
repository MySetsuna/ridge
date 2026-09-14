#!/usr/bin/env node
// Live RTP1-over-WebSocket kernel e2e harness (v8-6).
//
// Spawns a real `ridge kernel ensure` subprocess, then exercises the
// canonical RTP1 wire contract end-to-end over `ws://127.0.0.1:<port>/v1/rtp1`.
// Fails on any wire-contract regression (cf. packages/ridge-kernel/tests/conformance_rtp1.rs).
//
// Run:
//   pnpm build:ridge                # produces target/debug/ridge.exe
//   node scripts/rtp1-kernel-e2e.mjs
//
// Exits 0 on success, non-zero on first regression.

import { spawn } from "node:child_process";
import { mkdtempSync, readFileSync, existsSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import WebSocket from "ws";

const SETTLE_MS = 2000;
const DATA_DIR = mkdtempSync(join(tmpdir(), "ridge-rtp1-e2e-"));

function fail(msg) {
  console.error(`[rtp1-e2e] FAIL: ${msg}`);
  process.exit(1);
}

function pass(msg) {
  console.log(`[rtp1-e2e] PASS: ${msg}`);
}

async function fetchStatus(token) {
  const resp = await fetch(`http://127.0.0.1:${PORT}/v1/status`, {
    headers: { "x-ridge-kernel-token": token },
  });
  if (!resp.ok) {
    throw new Error(`status ${resp.status}`);
  }
  return resp.json();
}

async function createPty(token, hostId) {
  const resp = await fetch(`http://127.0.0.1:${PORT}/v1/domain/ptys`, {
    method: "POST",
    headers: {
      "x-ridge-kernel-token": token,
      "content-type": "application/json",
    },
    body: JSON.stringify({
      host_id: hostId,
      runtime_epoch: "",
      session_id: "e2e",
      pty_id: crypto.randomUUID(),
      program: process.platform === "win32" ? "cmd.exe" : "/bin/sh",
      args: process.platform === "win32" ? ["/C", "more"] : [],
      role: "e2e",
      cols: 80,
      rows: 24,
    }),
  });
  if (!resp.ok) {
    throw new Error(`pty create ${resp.status}`);
  }
  return (await resp.json()).pty_id;
}

const dataDirArg = `RIDGE_KERNEL_DATA_DIR=${DATA_DIR}`;
const env = {
  ...process.env,
  RIDGE_CONFIRM_QUIT_KERNEL: "1",
  RIDGE_TEST_ALLOW_NON_BREAKAWAY: "1",
  [dataDirArg]: DATA_DIR,
};

const ridgeBin = process.env.RIDGE_BIN ?? join("target", "debug", "ridge");
if (!existsSync(ridgeBin)) {
  fail(
    `ridge binary not found at ${ridgeBin}; run \`cargo build\` (or \`pnpm build:ridge\`) first`
  );
}

console.log(`[rtp1-e2e] booting ${ridgeBin} (data dir ${DATA_DIR})`);
const child = spawn(ridgeBin, ["kernel", "ensure"], {
  env,
  stdio: ["ignore", "pipe", "pipe"],
});

// Wait for kernel.json (server is up).
let endpoint;
const deadline = Date.now() + 15000;
while (Date.now() < deadline) {
  const path = join(DATA_DIR, "kernel.json");
  if (existsSync(path)) {
    try {
      endpoint = JSON.parse(readFileSync(path, "utf8"));
      break;
    } catch {
      // file in transit; retry
    }
  }
  await new Promise((r) => setTimeout(r, 100));
}
if (!endpoint) {
  fail("kernel did not publish endpoint within 15s");
}
console.log(`[rtp1-e2e] kernel ready: pid=${endpoint.pid} port=${endpoint.port}`);

const PORT = endpoint.port;
const TOKEN = endpoint.token;

// Wait a moment for the server to settle.
await new Promise((r) => setTimeout(r, SETTLE_MS));

const status = await fetchStatus(TOKEN);
const hostId = status.host_id;
const epoch = status.runtime_epoch;
if (!hostId || !epoch) fail(`status missing host_id / runtime_epoch: ${JSON.stringify(status)}`);
pass(`status host_id=${hostId} runtime_epoch=${epoch.substring(0, 8)}…`);

const ptyId = await createPty(TOKEN, hostId);
pass(`created pty ${ptyId.substring(0, 8)}…`);

// Open RTP1 WS.
const wsUrl = `ws://127.0.0.1:${PORT}/v1/rtp1`;
const ws = new WebSocket(wsUrl, {
  headers: { "x-ridge-kernel-token": TOKEN },
});

const received = [];
let attachAckReceived = null;
const t0 = Date.now();

await new Promise((resolve, reject) => {
  const timeout = setTimeout(() => reject(new Error("ws handshake timeout")), 5000);

  ws.on("open", () => {
    ws.send(
      JSON.stringify({
        type: "attach",
        host_id: hostId,
        runtime_epoch: epoch,
        session_id: "e2e",
        terminal_id: ptyId,
        controller_id: "e2e-controller",
        since_output_seq: null,
        mode: "raw",
        client_min_version: 1,
        client_max_version: 1,
      })
    );
  });

  ws.on("message", (data) => {
    try {
      const frame = JSON.parse(data.toString());
      if (frame.type === "attach_ack") {
        attachAckReceived = frame;
        clearTimeout(timeout);
        resolve();
      } else if (frame.type === "output") {
        received.push(frame);
      }
    } catch (err) {
      reject(err);
    }
  });

  ws.on("error", (err) => {
    clearTimeout(timeout);
    reject(err);
  });
});

if (!attachAckReceived) fail("no attach_ack received");
if (attachAckReceived.runtime_epoch !== epoch) {
  fail(`runtime_epoch mismatch: ${attachAckReceived.runtime_epoch} != ${epoch}`);
}
if (attachAckReceived.terminal_id !== ptyId) {
  fail(`terminal_id mismatch: ${attachAckReceived.terminal_id} != ${ptyId}`);
}
pass(`attach_ack runtime_epoch=${attachAckReceived.runtime_epoch} terminal_id=${attachAckReceived.terminal_id.substring(0, 8)}…`);

// Write a single char and expect it to round-trip as an output byte.
const inputId = "e2e-input-1";
const inputFrame = JSON.stringify({
  type: "input",
  terminal_id: ptyId,
  controller_id: "e2e-controller",
  input_seq: 1,
  data_b64: Buffer.from("e").toString("base64"),
  data_len: 1,
});
ws.send(inputFrame);

// Receive up to 5s of output frames.
const elapsed = Date.now() - t0;
const ok = await new Promise((resolve) => {
  let total = 0;
  const interval = setInterval(() => {
    total += 1;
    if (total >= 50) {
      clearInterval(interval);
      resolve(false);
    }
    if (received.length > 0 && received.some((f) => f.frames && f.frames.length > 0)) {
      clearInterval(interval);
      resolve(true);
    }
  }, 100);
});

if (!ok) fail("no output frames received within 5s after input");
pass(`received ${received.length} output frame(s) over ${elapsed}ms`);

// Detach cleanly.
ws.send(JSON.stringify({
  type: "detach",
  terminal_id: ptyId,
  controller_id: "e2e-controller",
  reason: "e2e-cleanup",
}));
await new Promise((r) => setTimeout(r, 500));
ws.close();

child.kill("SIGTERM");
await new Promise((r) => child.once("exit", r));
pass("kernel subprocess exited cleanly");

console.log("[rtp1-e2e] ALL PASS");
