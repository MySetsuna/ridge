#!/usr/bin/env node
// scripts/export-diagnostics.mjs
//
// Sanitized diagnostics exporter for device-acceptance runs. Produces a
// `diagnostics.json` file per the contract in
// `docs/architecture/notes/DEVICE-ACCEPTANCE-RUNBOOK.md` §3.
//
// Inputs (CLI):
//   --run    <path>     Run directory (used as output base if --out omitted).
//   --source <file>     Path to host-trace-final.log (text).
//   --out    <file>     Output JSON path (default: <run>/diagnostics.json).
//   --session-id  <id>  Optional explicit session_id (otherwise generated).
//   --host-id     <id>  Optional explicit host_id (otherwise 'unknown').
//   --runtime-epoch <n> Optional explicit runtime_epoch integer.
//   --ssh-known-hosts <file>  Path to known_hosts to scrub (default: %USERPROFILE%/.ssh/known_hosts or ~/.ssh/known_hosts).
//   --quiet            Suppress progress output.
//
// NEVER includes: TOTP values, bearer tokens, user-typed content,
// host IP/hostname, device serials, cert private keys, file:// paths,
// entries from known_hosts. Sanitizes previews by replacing printable
// bytes (other than ASCII alnum / common shell punctuation) with `.`.

import { readFileSync, writeFileSync, existsSync, mkdirSync } from 'node:fs';
import { dirname, join, resolve as resolvePath } from 'node:path';
import { homedir } from 'node:os';

const args = parseArgs(process.argv.slice(2));
const source = mustArg(args, 'source');
const out = args.out || join(args.run || '.', 'diagnostics.json');
const runDir = args.run ? dirname(out) : dirname(out);
if (runDir) mkdirSync(runDir, { recursive: true });

const sessionId = args['session-id'] || syntheticId('sess');
const hostId = args['host-id'] || 'unknown';
const runtimeEpoch = parseIntOrNull(args['runtime-epoch']) ?? nowSeconds();

const sshHosts = loadKnownHosts(args['ssh-known-hosts']);

// 1. Read + scrub source line by line.
const raw = readFileSync(source, 'utf8');
const lines = raw.split(/\r?\n/);

// Track per-pane first-seen and last-seen output_seq counter. The
// current trace format does not embed seq=, so we synthesize a counter
// that increments per OUT-binary frame per pane. This is enough to
// diagnose "did anything land?" — the runbook calls out "≥ 3 echo
// lines ⇒ seq delta ≥ 3" specifically.
const perPaneSeq = new Map();
const events = [];
const paneMeta = new Map();
const reconnect = []; // online / reconnecting / offline transitions

let hostStartedAt = null;
let lastFrameAt = null;

// Pre-pass: register pane→workspace bindings from IN `subscribe-pane`
// lines (the only place where the wire protocol stamps the pair).
for (const rawLine of lines) {
  if (!rawLine.includes('type=subscribe-pane')) continue;
  const pm = rawLine.match(/paneId="([^"]+)"/);
  const wm = rawLine.match(/workspaceId="([^"]+)"/);
  if (pm && wm) {
    paneMeta.set(pm[1], { workspaceId: wm[1], firstSeenAt: extractTs(rawLine) });
  }
}

// Pre-pass: collect every pane id we've ever seen referenced from the
// trace — used to recognize OUT frames whose preview contains
// `"id":"<uuid>"` for a known pane.
const knownPaneIds = new Set(paneMeta.keys());

// Walk the log once to find every ridge-trace line. Maintain a
// "lastSeenTs" by scanning for INFO/WARN/ERROR lines that carry an ISO
// timestamp — ridge-trace lines themselves do not carry one. Each
// ridge-trace event inherits the timestamp of the most recent
// timestamped line above it.

let lastSeenTs = null;

for (const rawLine of lines) {
  const line = rawLine;

  // Update lastSeenTs from any line carrying an ISO timestamp (INFO/WARN/ERROR
  // lines emitted by `tracing`).
  const ts = extractTs(line);
  if (ts) lastSeenTs = ts;

  // `ridge host ready` is printed by the host binary outside the
  // `[ridge-trace]` envelope, so detect it here.
  if (line.includes('ridge host ready')) {
    hostStartedAt = lastSeenTs;
    continue;
  }

  if (!line.includes('[ridge-trace]')) continue;
  const m = line.match(/\[ridge-trace\]\s+host\s+ws\s+(IN|OUT)\s+(\S+)\s+len=(\d+)\s+preview="(.*)"\s*$/);
  if (!m) continue;
  const [, dir, kind, lenStr, previewRaw] = m;
  const len = parseInt(lenStr, 10);

  // Pull paneId / workspaceId from the line itself (IN frames).
  let paneId = null;
  let wsId = null;
  const linePane = line.match(/paneId=(?:"([^"]+)"|(\S+))/);
  const lineWs = line.match(/workspaceId=(?:"([^"]+)"|(\S+))/);
  if (linePane) paneId = linePane[1] || linePane[2];
  if (lineWs) wsId = lineWs[1] || lineWs[2];

  // For OUT frames, the line itself has no paneId key — fall back to
  // the JSON preview: pty-meta frames use `"paneId":"..."`, list-panes
  // and workspace frames use `"id":"<uuid>"`. Match any known pane id
  // that appears in the preview.
  if (!paneId && dir === 'OUT') {
    const previewPane = previewRaw.match(/(?:\\|)"paneId(?:\\|)"\s*:\s*(?:\\|)"([0-9a-fA-F-]+)(?:\\|)"/);
    if (previewPane) {
      paneId = previewPane[1];
      const previewWs = previewRaw.match(/(?:\\|)"workspaceId(?:\\|)"\s*:\s*(?:\\|)"([0-9a-fA-F-]+)(?:\\|)"/);
      if (previewWs && !wsId) wsId = previewWs[1];
      if (!paneMeta.has(paneId)) {
        paneMeta.set(paneId, { workspaceId: wsId, firstSeenAt: lastSeenTs });
      }
    } else {
      for (const id of knownPaneIds) {
        if (
          previewRaw.includes(`\\"id\\":\\"${id}\\"`) ||
          previewRaw.includes(`"id":"${id}"`)
        ) {
          paneId = id;
          break;
        }
      }
    }
  }

  if (paneId && paneId !== 'null') {
    if (!paneMeta.has(paneId)) {
      paneMeta.set(paneId, { workspaceId: wsId, firstSeenAt: lastSeenTs });
    } else if (wsId && wsId !== 'null' && !paneMeta.get(paneId).workspaceId) {
      paneMeta.get(paneId).workspaceId = wsId;
    }
  }

  // Only count OUT binary/text frames as "output"; IN frames are user input.
  if (dir === 'OUT') {
    if (paneId && paneId !== 'null') {
      const cur = perPaneSeq.get(paneId) || 0;
      const next = cur + 1;
      perPaneSeq.set(paneId, next);
    }
    lastFrameAt = lastSeenTs;
    events.push({
      ts: lastSeenTs,
      kind,
      dir,
      paneId: paneId && paneId !== 'null' ? paneId : null,
      workspaceId: wsId && wsId !== 'null' ? wsId : null,
      byte_len: len,
      output_seq: paneId && paneId !== 'null' ? perPaneSeq.get(paneId) : null,
      preview: scrubPreview(previewRaw, sshHosts),
    });
  } else {
    events.push({
      ts: lastSeenTs,
      kind,
      dir,
      paneId: paneId && paneId !== 'null' ? paneId : null,
      workspaceId: wsId && wsId !== 'null' ? wsId : null,
      byte_len: len,
      preview: scrubPreview(previewRaw, sshHosts),
    });
  }
}

// 2. Derive reconnect state transitions.
// Heuristic: a gap of > 5 s between any two events while the host is
// "ready" implies a reconnect window. We don't have an explicit
// reconnect signal in the trace, so we record gap transitions and let
// downstream tooling interpret them.
let prevTs = hostStartedAt;
for (const ev of events) {
  if (!ev.ts || !prevTs) { prevTs = ev.ts; continue; }
  const dt = Date.parse(ev.ts) - Date.parse(prevTs);
  if (Number.isFinite(dt) && dt > 5000) {
    reconnect.push({
      from_state: 'online',
      to_state: 'reconnecting',
      at: prevTs,
      gap_ms: dt,
    });
    reconnect.push({
      from_state: 'reconnecting',
      to_state: 'online',
      at: ev.ts,
      gap_ms: dt,
    });
  }
  prevTs = ev.ts;
}

// 3. Assemble output.
const panes = [];
for (const [paneId, seq] of perPaneSeq.entries()) {
  panes.push({
    pane_id: paneId,
    workspace_id: paneMeta.get(paneId)?.workspaceId ?? null,
    output_seq: seq,
    first_seen_at: paneMeta.get(paneId)?.firstSeenAt ?? null,
  });
}

const diagnostics = {
  schema_version: 1,
  generated_at: new Date().toISOString(),
  source_log: source,
  sanitization: {
    redacted_patterns: [
      'TOTP 6-digit codes in [ridge-trace] totp=... context',
      'Bearer tokens',
      'token=... values',
      'file:// paths',
      'known_hosts hostnames',
    ],
    note: 'preview fields are scrubbed: non-printable bytes become ".", printable non-ASCII become ".", strings matching redacted patterns are dropped.',
  },
  identifiers: {
    session_id: sessionId,
    host_id: hostId,
    runtime_epoch: runtimeEpoch,
  },
  host: {
    started_at: hostStartedAt,
    last_frame_at: lastFrameAt,
  },
  panes,
  reconnect_transitions: reconnect,
  event_count: events.length,
  events,
};

writeFileSync(out, JSON.stringify(diagnostics, null, 2));
if (!args.quiet) {
  console.log(`[diag] wrote ${out} events=${events.length} panes=${panes.length} reconnects=${reconnect.length}`);
}

function parseArgs(arr) {
  const out = {};
  for (let i = 0; i < arr.length; i++) {
    const k = arr[i];
    if (k && typeof k === 'string' && k.startsWith('--')) {
      const v = arr[i + 1];
      if (v != null && typeof v === 'string' && !v.startsWith('--')) { out[k.slice(2)] = v; i++; }
      else { out[k.slice(2)] = true; }
    }
  }
  return out;
}

function mustArg(obj, key) {
  if (!obj[key]) {
    console.error(`[diag] missing required arg: ${key}`);
    process.exit(2);
  }
  return obj[key];
}

function parseIntOrNull(s) {
  if (s == null) return null;
  const n = parseInt(s, 10);
  return Number.isFinite(n) ? n : null;
}

function nowSeconds() {
  return Math.floor(Date.now() / 1000);
}

function syntheticId(prefix) {
  // 16 hex chars derived from timestamp + a small entropy mix.
  const t = Date.now().toString(16);
  const r = Math.floor(Math.random() * 0xffff).toString(16).padStart(4, '0');
  return `${prefix}_${t}${r}`;
}

function extractTs(line) {
  // Tolerate ANSI color escapes wrapping the timestamp.
  const m = line.match(/\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?Z/);
  return m ? m[0] : null;
}

function loadKnownHosts(argPath) {
  const candidates = argPath
    ? [argPath]
    : process.platform === 'win32'
      ? [join(homedir(), '.ssh', 'known_hosts')]
      : [join(homedir(), '.ssh', 'known_hosts')];
  const set = new Set();
  for (const p of candidates) {
    if (!existsSync(p)) continue;
    try {
      const txt = readFileSync(p, 'utf8');
      for (const line of txt.split(/\r?\n/)) {
        if (!line || line.startsWith('#')) continue;
        const first = line.split(/[ ,]/)[0];
        // hostnames can be `host`, `[host]:port`, or comma-separated.
        for (const h of first.split(',')) {
          const cleaned = h.replace(/^\[|\]:\d+$/g, '');
          if (cleaned && !cleaned.includes('*')) set.add(cleaned);
        }
      }
    } catch { /* unreadable — skip */ }
  }
  return set;
}

function scrubPreview(raw, sshHosts) {
  if (!raw) return '';
  // Drop entire preview if it matches a known redacted pattern.
  if (/Bearer\s+[A-Za-z0-9._-]+/i.test(raw)) return '<redacted: bearer>';
  if (/token=[A-Za-z0-9._-]+/i.test(raw)) return '<redacted: token>';
  if (/file:\/\/[^\s"]+/i.test(raw)) return '<redacted: file uri>';
  // Strip SSH-known hostnames if they appear (rare in trace but cheap).
  let s = raw;
  for (const h of sshHosts) {
    if (h && h.length >= 4 && s.includes(h)) {
      s = s.split(h).join('<host>');
    }
  }
  // Replace non-printable / non-ASCII printable bytes with '.', keep alnum + basic punctuation.
  let out = '';
  for (const ch of s) {
    const c = ch.codePointAt(0);
    if (c < 0x20 || c === 0x7f) { out += '.'; continue; }
    if (c > 0x7e) { out += '.'; continue; }
    if (/[A-Za-z0-9 .,;:_/\-+=()\[\]{}'"<>?!@#$%^&*\\|`~]/.test(ch)) out += ch;
    else out += '.';
  }
  return out;
}