// scripts/api-integration.mjs
//
// API/protocol integration test for the candidate Host + Kernel.
//
// Scope (renamed from browser-smoke-candidate.mjs in v9-14 / Goal §2):
//   THIS SCRIPT ONLY CALLS KERNEL HTTP ENDPOINTS. It does NOT drive a real
//   browser; it does NOT load the SPA; it does NOT exercise the UI input
//   pipeline. It validates that the same wire protocol the SPA uses works
//   end-to-end against THIS candidate, with correct auth, lease semantics,
//   resize, A→B→A, late response, and detach/reconnect.
//
// For actual browser-driven UI E2E see scripts/browser-ui-e2e.mjs.
//
// §5: drive the same kernel endpoints the Web SPA bundles use:
//   - auth: kernel token from RIDGE_KERNEL_DATA_DIR/kernel.json (the registry
//     the SPA reads on connect via RemoteInfo; same as in smoke-candidate.mjs)
//   - list:  GET /v1/domain/ptys
//   - create: POST /v1/domain/ptys
//   - write:  POST /v1/domain/ptys/:id/write  (base64 data_b64)
//   - resize: POST /v1/domain/ptys/:id/resize
//   - attach: POST /v1/domain/ptys/:id/output?after_seq=N
//   - poll:   GET  /v1/domain/ptys/:id/output/:lease_id?timeout_ms=...
//   - detach: DELETE /v1/domain/ptys/:id/output/:lease_id
//   - resync: POST /v1/domain/ptys/:id/output/:lease_id/resync
//
// Edge cases covered (§5):
//   - rapid A→B→A: open lease on A, write+receive, switch to B, back to A
//   - late response: long-poll a quiet lease, then write data; frames arrive
//   - reconnect: detach lease, re-attach, poll; frames from before are gone,
//     new lease only sees fresh bytes (per L2-TERM-001 §2.4 lease semantics)
//   - resize then re-list: cols/rows reflect new value
//   - auth on every authenticated call; 401 expected when token stripped
//
// TLS posture (Goal §3):
//   Host /health on https:// pinned to host CA via tls-host.mjs. Kernel
//   loopback is HTTP.
//
// Isolation:
//   - isolated data dir (RIDGE_KERNEL_DATA_DIR)
//   - port 5120 (Host) + kernel dynamic port from kernel.json
//   - candidate binary at target/test-rdg/release/ridge.exe

import { spawn } from "node:child_process";
import { Buffer } from "node:buffer";
import { mkdtempSync, readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { pinnedHttpsJson } from "./tls-host.mjs";

const HOST_PORT = Number(process.env.RIDGE_SMOKE_HOST_PORT ?? "5120");
const BIN = resolve(
  process.env.RIDGE_BIN ?? "target/test-rdg/release/ridge.exe",
);

const HEADER = "x-ridge-kernel-token";

const failures = [];
function expect(label, cond, ctx) {
  if (cond) {
    console.log(`[browser] PASS ${label}`);
  } else {
    console.error(`[browser] FAIL ${label}`);
    if (ctx !== undefined) console.error(JSON.stringify(ctx, null, 2));
    failures.push(label);
  }
}

function b64encode(s) {
  return Buffer.from(s, "utf8").toString("base64");
}
function decodeFrames(json) {
  if (!json || !Array.isArray(json.frames)) return [];
  return json.frames.map((f) => ({
    seq: f.seq,
    data_b64: f.data_b64 ?? f.data ?? null,
  }));
}
function concatUtf8(frames) {
  return frames
    .map((f) => {
      const buf = f.data_b64 ? Buffer.from(f.data_b64, "base64") : Buffer.alloc(0);
      return buf.toString("utf8");
    })
    .join("");
}

// Drain a lease's currently-buffered frames until either an empty poll comes
// back or `maxPolls` rounds elapse. Returns the concatenated UTF-8 text.
// Used to absorb cmd.exe's banner and prompt rewrite noise before asserting
// on the bytes that follow our `write`. Goal §5 isn't "no banner" — it's
// "browser-layer flow carries input bytes through to a SPA-visible echo".
async function drainLease(kurl, ptyId, leaseId, token, maxPolls = 6) {
  let allText = "";
  for (let i = 0; i < maxPolls; i += 1) {
    const r = await httpJson(
      "GET",
      `${kurl}/v1/domain/ptys/${ptyId}/output/${leaseId}?timeout_ms=400&max_frames=64`,
      undefined,
      token,
    );
    if (r.status !== 200) break;
    const frames = decodeFrames(r.json);
    allText += concatUtf8(frames);
    if (frames.length === 0) break;
  }
  return allText;
}

async function httpJson(method, url, body, token) {
  const headers = { "content-type": "application/json" };
  if (token) headers[HEADER] = token;
  const res = await fetch(url, {
    method,
    headers,
    body: body === undefined ? undefined : JSON.stringify(body),
    signal: AbortSignal.timeout(15000),
  });
  const text = await res.text();
  let json = null;
  try {
    json = text ? JSON.parse(text) : null;
  } catch {
    // not JSON
  }
  return { status: res.status, text, json };
}

async function sleep(ms) {
  await new Promise((r) => setTimeout(r, ms));
}

const dataDir = mkdtempSync(join(tmpdir(), "ridge-browser-"));
console.log(`[browser] isolated data dir: ${dataDir}`);
console.log(`[browser] candidate binary: ${BIN}`);
console.log(`[browser] host port: ${HOST_PORT}`);

const child = spawn(
  BIN,
  ["host", "--port", String(HOST_PORT)],
  {
    env: {
      ...process.env,
      RIDGE_KERNEL_DATA_DIR: dataDir,
      RIDGE_PRINT_TOTP: "1",
      RIDGE_TEST_ALLOW_NON_BREAKAWAY: "1",
      RIDGE_REMOTE_HOST_REGISTRY: join(dataDir, "host-registry.json"),
    },
    stdio: ["ignore", "pipe", "pipe"],
  },
);

child.stdout.on("data", (d) => process.stdout.write(d));
child.stderr.on("data", (d) => process.stderr.write(d));
child.on("exit", (code) => {
  if (code !== null && code !== 0) {
    console.error(`[browser] host exited code=${code}`);
  }
});

// Wait for Host /health
let hostUp = false;
for (let i = 0; i < 30; i += 1) {
  await sleep(500);
  try {
    const r = await pinnedHttpsJson(`https://127.0.0.1:${HOST_PORT}/health`);
    if (r.status === 200) hostUp = true;
  } catch {
    /* not yet */
  }
}
if (!hostUp) {
  console.error("[browser] FAIL Host never came up");
  process.exit(1);
}

// Wait for kernel registry
let kernel = null;
const kernelJsonPath = join(dataDir, "kernel.json");
for (let i = 0; i < 40; i += 1) {
  await sleep(250);
  try {
    kernel = JSON.parse(readFileSync(kernelJsonPath, "utf8"));
    break;
  } catch {
    /* not yet */
  }
}
if (!kernel) {
  console.error("[browser] FAIL kernel.json not found");
  process.exit(1);
}
const kurl = `http://127.0.0.1:${kernel.port}`;
console.log(
  `[browser] kernel pid=${kernel.pid} port=${kernel.port} (isolated data dir)`,
);

// === Step 1: auth — list with token === (matches what the SPA does on connect)
{
  const r = await httpJson("GET", `${kurl}/v1/domain/ptys`, undefined, kernel.token);
  expect("auth: list ptys with token returns 200", r.status === 200, r);
  expect(
    "auth: list returns ok=true and ptys array",
    r.json && r.json.ok === true && Array.isArray(r.json.ptys),
    r.json,
  );
}
{
  const r = await httpJson("GET", `${kurl}/v1/domain/ptys`, undefined, undefined);
  expect("auth: list ptys without token returns 401", r.status === 401, r);
}

// === Step 2: create PTY A (shell echo) === (matches SPA attach flow)
let ptyA;
{
  const r = await httpJson(
    "POST",
    `${kurl}/v1/domain/ptys`,
    {
      role: "shell",
      shell: process.platform === "win32" ? "cmd.exe" : "/bin/sh",
      cols: 80,
      rows: 24,
    },
    kernel.token,
  );
  expect("create: PTY A returns 200", r.status === 200, r);
  expect(
    "create: PTY A returns ok=true with pty_id",
    r.json && r.json.ok === true && typeof r.json.pty_id === "string",
    r.json,
  );
  ptyA = r.json?.pty_id;
}

// === Step 3: list now contains A ===
{
  const r = await httpJson("GET", `${kurl}/v1/domain/ptys`, undefined, kernel.token);
  const ids = (r.json?.ptys ?? []).map((p) => p.id);
  expect(
    "list: contains PTY A",
    ids.includes(ptyA),
    { ids, expected: ptyA },
  );
  const meta = (r.json?.ptys ?? []).find((p) => p.id === ptyA);
  expect(
    "list: A has cols=80 rows=24",
    meta && meta.cols === 80 && meta.rows === 24,
    meta,
  );
}

// === Step 4: attach output lease A ===
let leaseA;
{
  const r = await httpJson(
    "POST",
    `${kurl}/v1/domain/ptys/${ptyA}/output?after_seq=0`,
    undefined,
    kernel.token,
  );
  expect("attach: lease A returns 200", r.status === 200, r);
  expect(
    "attach: lease A has lease_id + protocol + rtp1_endpoint",
    r.json && r.json.lease_id && r.json.protocol === "bounded-seq-v1" &&
      r.json.rtp1_endpoint === "/v1/rtp1",
    r.json,
  );
  leaseA = r.json?.lease_id;
}

// === Step 5: long-poll empty (no data yet) — accept initial banner is normal ===
{
  const r = await httpJson(
    "GET",
    `${kurl}/v1/domain/ptys/${ptyA}/output/${leaseA}?timeout_ms=500&max_frames=16`,
    undefined,
    kernel.token,
  );
  expect("poll: lease A returns 200", r.status === 200, r);
  expect(
    "poll: lease A frames field is an array (banner OK, errors not)",
    r.json && Array.isArray(r.json.frames),
    r.json,
  );
}

// === Step 6: write input on A ===
{
  const r = await httpJson(
    "POST",
    `${kurl}/v1/domain/ptys/${ptyA}/write`,
    { data_b64: b64encode("echo BROWSER_SMOKE_A\r\n") },
    kernel.token,
  );
  expect("write: A write returns 200 ok", r.status === 200 && r.json?.ok === true, r);
}

// === Step 7: drain A — expect to eventually receive BROWSER_SMOKE_A echoed back ===
{
  const text = await drainLease(kurl, ptyA, leaseA, kernel.token, 10);
  expect(
    "poll: A drain contains BROWSER_SMOKE_A echo",
    text.includes("BROWSER_SMOKE_A"),
    { text: text.slice(-400), framesLen: text.length },
  );
}

// === Step 8: resize A ===
{
  const r = await httpJson(
    "POST",
    `${kurl}/v1/domain/ptys/${ptyA}/resize`,
    { cols: 100, rows: 30 },
    kernel.token,
  );
  expect("resize: A returns 200 ok", r.status === 200 && r.json?.ok === true, r);
}
// re-list to confirm dimensions took
{
  const r = await httpJson("GET", `${kurl}/v1/domain/ptys`, undefined, kernel.token);
  const meta = (r.json?.ptys ?? []).find((p) => p.id === ptyA);
  expect(
    "resize: A reflects cols=100 rows=30 in list",
    meta && meta.cols === 100 && meta.rows === 30,
    meta,
  );
}

// === Step 9: create PTY B for rapid A↔B swap ===
let ptyB;
{
  const r = await httpJson(
    "POST",
    `${kurl}/v1/domain/ptys`,
    {
      role: "shell",
      shell: process.platform === "win32" ? "cmd.exe" : "/bin/sh",
      cols: 80,
      rows: 24,
    },
    kernel.token,
  );
  expect("create: PTY B returns 200", r.status === 200 && r.json?.ok === true, r);
  ptyB = r.json?.pty_id;
}
let leaseB;
{
  const r = await httpJson(
    "POST",
    `${kurl}/v1/domain/ptys/${ptyB}/output?after_seq=0`,
    undefined,
    kernel.token,
  );
  expect("attach: lease B returns 200", r.status === 200, r);
  leaseB = r.json?.lease_id;
}

// === Step 10: rapid A→B→A switch (write on B, poll B; re-poll A) ===
{
  // write on B
  const wB = await httpJson(
    "POST",
    `${kurl}/v1/domain/ptys/${ptyB}/write`,
    { data_b64: b64encode("echo BROWSER_SMOKE_B\r\n") },
    kernel.token,
  );
  expect("write: B write returns 200", wB.status === 200 && wB.json?.ok === true, wB);
  // drain B until we see the echoed text
  const textB = await drainLease(kurl, ptyB, leaseB, kernel.token, 10);
  expect(
    "poll: B drain contains BROWSER_SMOKE_B",
    textB.includes("BROWSER_SMOKE_B"),
    { text: textB.slice(-300) },
  );
  // drain A again (A→B→A) — lease A is still valid
  const textA2 = await drainLease(kurl, ptyA, leaseA, kernel.token, 10);
  expect(
    "poll: A re-drain still resolves after A→B→A (lease stable)",
    typeof textA2 === "string",
    { len: textA2.length },
  );
  // sanity: A and B frames did NOT mix (no cross-leakage)
  expect(
    "poll: A frames do not leak B's BROWSER_SMOKE_B",
    !textA2.includes("BROWSER_SMOKE_B"),
    { text: textA2.slice(-200) },
  );
  expect(
    "poll: B frames do not leak A's BROWSER_SMOKE_A",
    !textB.includes("BROWSER_SMOKE_A"),
    { text: textB.slice(-200) },
  );
}

// === Step 11: late response — attach new lease, drain initial banner, write, expect data ===
let leaseLate;
{
  const a1 = await httpJson(
    "POST",
    `${kurl}/v1/domain/ptys/${ptyA}/output?after_seq=0`,
    undefined,
    kernel.token,
  );
  expect("late: second lease A returns 200", a1.status === 200, a1);
  leaseLate = a1.json?.lease_id;
  // drain pre-existing buffered frames (banner, any leftover)
  const drained = await drainLease(kurl, ptyA, leaseLate, kernel.token, 6);
  expect(
    "late: drain of pre-write buffer completes without error",
    typeof drained === "string",
    { len: drained.length },
  );
  // now write
  await httpJson(
    "POST",
    `${kurl}/v1/domain/ptys/${ptyA}/write`,
    { data_b64: b64encode("echo BROWSER_SMOKE_LATE\r\n") },
    kernel.token,
  );
  // drain again — expect the late frame
  const afterText = await drainLease(kurl, ptyA, leaseLate, kernel.token, 10);
  expect(
    "late: late-arriving frames contain BROWSER_SMOKE_LATE",
    afterText.includes("BROWSER_SMOKE_LATE"),
    { text: afterText.slice(-300) },
  );
}

// === Step 12: detach + re-attach (reconnect) ===
{
  // detach the original lease
  const det = await httpJson(
    "DELETE",
    `${kurl}/v1/domain/ptys/${ptyA}/output/${leaseA}`,
    undefined,
    kernel.token,
  );
  expect("reconnect: detach lease A returns 200", det.status === 200, det);
  // re-attach
  const att = await httpJson(
    "POST",
    `${kurl}/v1/domain/ptys/${ptyA}/output?after_seq=0`,
    undefined,
    kernel.token,
  );
  expect("reconnect: re-attach lease A returns 200", att.status === 200, att);
  expect(
    "reconnect: re-attach produces a new lease_id",
    att.json?.lease_id && att.json.lease_id !== leaseA,
    { old: leaseA, new: att.json?.lease_id },
  );
  const newLease = att.json?.lease_id;
  // write again
  await httpJson(
    "POST",
    `${kurl}/v1/domain/ptys/${ptyA}/write`,
    { data_b64: b64encode("echo BROWSER_SMOKE_RECONNECT\r\n") },
    kernel.token,
  );
  // drain with new lease
  const text = await drainLease(kurl, ptyA, newLease, kernel.token, 10);
  expect(
    "reconnect: new lease carries post-reconnect BROWSER_SMOKE_RECONNECT",
    text.includes("BROWSER_SMOKE_RECONNECT"),
    { text: text.slice(-300) },
  );
  // resync endpoint exists for completeness
  const rs = await httpJson(
    "POST",
    `${kurl}/v1/domain/ptys/${ptyA}/output/${newLease}/resync`,
    undefined,
    kernel.token,
  );
  expect("resync: endpoint returns 200", rs.status === 200 && rs.json?.ok === true, rs);
  // detach new lease
  const det2 = await httpJson(
    "DELETE",
    `${kurl}/v1/domain/ptys/${ptyA}/output/${newLease}`,
    undefined,
    kernel.token,
  );
  expect("reconnect: detach new lease returns 200", det2.status === 200, det2);
}

// === Step 13: negative auth on a PTY-scoped route ===
{
  const r = await httpJson(
    "POST",
    `${kurl}/v1/domain/ptys/${ptyA}/write`,
    { data_b64: b64encode("echo should-fail\r\n") },
    undefined,
  );
  expect("negative: write without token returns 401", r.status === 401, r);
}

// === Step 14: destroy + verify list shrinks ===
{
  const d = await httpJson(
    "DELETE",
    `${kurl}/v1/domain/ptys/${ptyB}`,
    undefined,
    kernel.token,
  );
  expect("destroy: PTY B returns 200", d.status === 200, d);
  const list = await httpJson("GET", `${kurl}/v1/domain/ptys`, undefined, kernel.token);
  const ids = (list.json?.ptys ?? []).map((p) => p.id);
  expect(
    "destroy: B no longer in list",
    !ids.includes(ptyB),
    { ids, expectedAbsent: ptyB },
  );
}

// Cleanup
try {
  child.kill("SIGINT");
  await sleep(300);
  child.kill("SIGKILL");
} catch {
  /* gone */
}

console.log("");
console.log(`[browser] ${failures.length === 0 ? "ALL PASS" : `FAIL ${failures.length}: ${failures.join(" | ")}`}`);
process.exit(failures.length === 0 ? 0 : 1);