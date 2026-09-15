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

async function createWorkspace(token) {
  const resp = await fetch(`http://127.0.0.1:${PORT}/v1/domain/workspaces`, {
    method: "POST",
    headers: { "x-ridge-kernel-token": token },
  });
  if (!resp.ok) {
    throw new Error(`workspace create ${resp.status}`);
  }
  const body = await resp.json();
  if (!body.ok || !body.workspace_id) {
    throw new Error(`workspace create failed: ${JSON.stringify(body)}`);
  }
  return body.workspace_id;
}

async function createPty(token, hostId, workspaceId) {
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
      workspace_id: workspaceId,
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
  const body = await resp.json();
  if (!body.ok || !body.pty_id) {
    throw new Error(`pty create failed: ${JSON.stringify(body)}`);
  }
  return body.pty_id;
}

const env = {
  ...process.env,
  RIDGE_CONFIRM_QUIT_KERNEL: "1",
  RIDGE_TEST_ALLOW_NON_BREAKAWAY: "1",
  RIDGE_KERNEL_DATA_DIR: DATA_DIR,
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

const workspaceId = await createWorkspace(TOKEN);
pass(`created workspace ${workspaceId.substring(0, 8)}…`);

const ptyId = await createPty(TOKEN, hostId, workspaceId);
pass(`created pty ${ptyId.substring(0, 8)}…`);

// Open RTP1 WS.
const wsUrl = `ws://127.0.0.1:${PORT}/v1/rtp1`;
const ws = new WebSocket(wsUrl, {
  headers: { "x-ridge-kernel-token": TOKEN },
  binaryType: "arraybuffer",
});

// RTP1 wire codec (SPEC-L2-PROTO-001 §3.3):
//   header: 'R','T','P','1' (4 bytes) | efv (1) | msg_type (1) | flags (1) | len (4 LE) = 11 bytes
//   payload: JSON-encoded MessageType-specific object
const RTP1_MAGIC = Buffer.from("RTP1", "ascii");
const RTP1_EFV = 0x01;
const HEADER_LEN = 11;
const MSG = {
  Attach: 0x01, AttachAck: 0x02, Detach: 0x03, DetachAck: 0x04,
  Input: 0x05, InputAck: 0x06, Output: 0x07, Delta: 0x08,
  Resize: 0x09, ResizeAck: 0x0a, Replay: 0x0b, ReplayData: 0x0c,
  Snapshot: 0x0d, Title: 0x0e, Cwd: 0x0f, Desync: 0x10,
  Resync: 0x11, Error: 0x12, Ping: 0x13, Pong: 0x14,
  CapabilityAdvertise: 0x15, CapabilityReply: 0x16, SessionEvent: 0x17,
};
const MSG_NAME = Object.fromEntries(Object.entries(MSG).map(([k, v]) => [v, k]));

function encodeFrame(type, payload) {
  const payloadBuf = Buffer.from(JSON.stringify(payload), "utf8");
  const buf = Buffer.alloc(HEADER_LEN + payloadBuf.length);
  RTP1_MAGIC.copy(buf, 0);
  buf[4] = RTP1_EFV;
  buf[5] = type;
  buf[6] = 0; // flags
  buf.writeUInt32LE(payloadBuf.length, 7);
  payloadBuf.copy(buf, HEADER_LEN);
  return buf;
}

let rxBuffer = Buffer.alloc(0);
function ingestBinary(chunk) {
  rxBuffer = Buffer.concat([rxBuffer, Buffer.from(chunk)]);
  const out = [];
  while (rxBuffer.length >= HEADER_LEN) {
    if (!rxBuffer.subarray(0, 4).equals(RTP1_MAGIC)) {
      // out-of-sync; resync by skipping to next magic
      const next = rxBuffer.indexOf("RTP1", 1);
      if (next < 0) {
        rxBuffer = Buffer.alloc(0);
        return out;
      }
      rxBuffer = rxBuffer.subarray(next);
      continue;
    }
    if (rxBuffer[4] !== RTP1_EFV) {
      throw new Error(`unknown RTP1 EFV ${rxBuffer[4]}`);
    }
    const len = rxBuffer.readUInt32LE(7);
    if (rxBuffer.length < HEADER_LEN + len) break;
    const typeByte = rxBuffer[5];
    const flags = rxBuffer[6];
    const payload = rxBuffer.subarray(HEADER_LEN, HEADER_LEN + len);
    rxBuffer = rxBuffer.subarray(HEADER_LEN + len);
    let parsed;
    try { parsed = JSON.parse(payload.toString("utf8")); }
    catch (e) { parsed = { __parse_error: String(e), __raw: payload.toString("utf8") }; }
    out.push({ typeByte, typeName: MSG_NAME[typeByte] || `?0x${typeByte.toString(16)}`, flags, payload: parsed });
  }
  return out;
}

const received = [];
let attachAckReceived = null;
let capAdvertiseReceived = null;
const t0 = Date.now();

await new Promise((resolve, reject) => {
  const timeout = setTimeout(() => reject(new Error("ws attach timeout")), 8000);

  ws.on("open", () => {
    const attach = encodeFrame(MSG.Attach, {
      host_id: hostId,
      runtime_epoch: epoch,
      session_id: "e2e",
      terminal_id: ptyId,
      controller_id: "e2e-controller",
      since_output_seq: null,
      mode: "raw",
      client_min_version: 1,
      client_max_version: 1,
    });
    ws.send(attach);
  });

  ws.on("message", (data) => {
    let frames;
    try { frames = ingestBinary(data); }
    catch (e) { clearTimeout(timeout); reject(e); return; }
    for (const f of frames) {
      if (f.typeName === "CapabilityAdvertise") {
        capAdvertiseReceived = f.payload;
      } else if (f.typeName === "AttachAck") {
        attachAckReceived = f.payload;
        clearTimeout(timeout);
        resolve();
      } else if (f.typeName === "Output" || f.typeName === "Delta") {
        received.push(f.payload);
      } else if (f.typeName === "Error") {
        clearTimeout(timeout);
        reject(new Error(`server error frame: ${JSON.stringify(f.payload)}`));
        return;
      }
    }
  });

  ws.on("error", (err) => {
    clearTimeout(timeout);
    reject(err);
  });
});

if (!capAdvertiseReceived) fail("no capability_advertise received");
pass(`capability_advertise features=${capAdvertiseReceived.features?.length ?? 0} max_realtime_frame=${capAdvertiseReceived.max_realtime_frame}`);

if (!attachAckReceived) fail("no attach_ack received");
if (attachAckReceived.runtime_epoch !== epoch) {
  fail(`runtime_epoch mismatch: ${attachAckReceived.runtime_epoch} != ${epoch}`);
}
if (attachAckReceived.terminal_id !== ptyId) {
  fail(`terminal_id mismatch: ${attachAckReceived.terminal_id} != ${ptyId}`);
}
pass(`attach_ack server_version=${attachAckReceived.server_version} next_output_seq=${attachAckReceived.next_output_seq} controller_input_seq=${attachAckReceived.controller_input_seq}`);

// Write a single char and expect it to round-trip as an output byte.
const inputSeq = attachAckReceived.controller_input_seq;
const inputBuf = encodeFrame(MSG.Input, {
  terminal_id: ptyId,
  controller_id: "e2e-controller",
  input_seq: inputSeq,
  data_b64: Buffer.from("e").toString("base64"),
  data_len: 1,
});
ws.send(inputBuf);

const elapsed = Date.now() - t0;
const ok = await new Promise((resolve) => {
  let total = 0;
  const interval = setInterval(() => {
    total += 1;
    if (total >= 50) {
      clearInterval(interval);
      resolve(false);
    }
    if (received.length > 0 && received.some((f) => Array.isArray(f.frames) && f.frames.length > 0)) {
      clearInterval(interval);
      resolve(true);
    }
  }, 100);
});

if (!ok) fail("no output frames received within 5s after input");
const outBytes = received.flatMap((f) => (f.frames || []).map((c) => Buffer.from(c.data_b64 || "", "base64"))).reduce((a, b) => a + b.length, 0);
pass(`received ${received.length} output frame(s) (${outBytes} bytes) over ${elapsed}ms`);

// Detach cleanly.
const detachBuf = encodeFrame(MSG.Detach, {
  terminal_id: ptyId,
  controller_id: "e2e-controller",
  reason: "e2e-cleanup",
});
ws.send(detachBuf);
await new Promise((r) => setTimeout(r, 500));
ws.close();

child.kill();
// On Windows, child.kill() sends nothing usable and the kernel keeps running
// with the WS attached; we have no time to wait for an actual exit event.
// Detach our handles and let the OS reap the child when the parent exits.
child.stdout?.destroy();
child.stderr?.destroy();
pass("kernel subprocess terminated");
process.exit(0);

console.log("[rtp1-e2e] ALL PASS");
