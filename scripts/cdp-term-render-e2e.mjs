#!/usr/bin/env node
// Real WebView2 acceptance probe for Codex TUI rendering and terminal input.
// Uses Ridge's DEV-only PTY hooks, so every byte still crosses the native PTY
// and returns through the production parser/renderer path.

import crypto from 'node:crypto';
import fs from 'node:fs';
import http from 'node:http';
import path from 'node:path';
import sharp from 'sharp';
import { DEV_USER_DATA_DIR, resolveCdpPort } from './cdp-port.mjs';
import { isRidgeCdpTarget } from './lib/cdpTarget.mjs';

const port = resolveCdpPort();
const expectedDevOrigin = (() => {
  try {
    const config = JSON.parse(fs.readFileSync(path.join(DEV_USER_DATA_DIR, 'tauri-dev-cdp.config.json'), 'utf8'));
    return new URL(config.build.devUrl).origin;
  } catch {
    return null;
  }
})();
const requestedDpr = Number.parseFloat(process.env.RIDGE_TERM_E2E_DPR || '');
const emulatedDpr = Number.isFinite(requestedDpr) && requestedDpr > 0 ? requestedDpr : null;
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const timeoutMs = Number.parseInt(process.env.RIDGE_TERM_E2E_TIMEOUT_MS || '240000', 10);
const artifactDir = path.resolve(process.env.RIDGE_TERM_E2E_ARTIFACT_DIR || '.iteration/artifacts/term-render');
const fixtureOnly = process.env.RIDGE_TERM_E2E_FIXTURE_ONLY === '1';
const expectWebgpuFailure = process.env.RIDGE_TERM_E2E_EXPECT_WEBGPU_FAILURE === '1';
const expectWebglFallback = process.env.RIDGE_TERM_E2E_EXPECT_WEBGL_FALLBACK === '1';
const requestedBurstLines = Number.parseInt(process.env.RIDGE_TERM_E2E_BURST_LINES || '120', 10);
const burstLines = Number.isFinite(requestedBurstLines)
  ? Math.min(Math.max(requestedBurstLines, 0), 10_000)
  : 0;
const requestedBurstP95MaxMs = Number.parseInt(process.env.RIDGE_TERM_E2E_BURST_P95_MAX_MS || '50', 10);
const burstP95MaxMs = Number.isFinite(requestedBurstP95MaxMs)
  ? Math.max(requestedBurstP95MaxMs, 1)
  : 50;
const burstMode = process.env.RIDGE_TERM_E2E_BURST_MODE === 'lines' ? 'lines' : 'tui';
const requestedBurstIntervalMs = Number.parseInt(process.env.RIDGE_TERM_E2E_BURST_INTERVAL_MS || '16', 10);
const burstIntervalMs = Number.isFinite(requestedBurstIntervalMs)
  ? Math.min(Math.max(requestedBurstIntervalMs, 0), 500)
  : 0;
const requestedBurstFrameP95MaxMs = Number.parseInt(process.env.RIDGE_TERM_E2E_BURST_FRAME_P95_MAX_MS || '25', 10);
const burstFrameP95MaxMs = Number.isFinite(requestedBurstFrameP95MaxMs)
  ? Math.max(requestedBurstFrameP95MaxMs, 1)
  : 25;
const requestedGlobalEventLoopP95MaxMs = Number.parseInt(process.env.RIDGE_TERM_E2E_GLOBAL_EVENT_LOOP_P95_MAX_MS || '25', 10);
const globalEventLoopP95MaxMs = Number.isFinite(requestedGlobalEventLoopP95MaxMs)
  ? Math.max(requestedGlobalEventLoopP95MaxMs, 0)
  : 25;
const requestedGlobalLongTaskMaxMs = Number.parseInt(process.env.RIDGE_TERM_E2E_GLOBAL_LONG_TASK_MAX_MS || '100', 10);
const globalLongTaskMaxMs = Number.isFinite(requestedGlobalLongTaskMaxMs)
  ? Math.max(requestedGlobalLongTaskMaxMs, 0)
  : 100;
const requestedGlobalLongTask50msCountMax = Number.parseInt(process.env.RIDGE_TERM_E2E_GLOBAL_LONG_TASK_50MS_COUNT_MAX || '0', 10);
const globalLongTask50msCountMax = Number.isFinite(requestedGlobalLongTask50msCountMax)
  ? Math.max(requestedGlobalLongTask50msCountMax, 0)
  : 0;
const requestedSoakRounds = Number.parseInt(process.env.RIDGE_TERM_E2E_CODEX_SOAK_ROUNDS || '0', 10);
const soakRounds = Number.isFinite(requestedSoakRounds)
  ? Math.min(Math.max(requestedSoakRounds, 0), 500)
  : 0;
const requestedSoakDurationMs = Number.parseInt(process.env.RIDGE_TERM_E2E_CODEX_SOAK_DURATION_MS || '0', 10);
const soakDurationMs = Number.isFinite(requestedSoakDurationMs)
  ? Math.min(Math.max(requestedSoakDurationMs, 0), 1_800_000)
  : 0;
const soakEnabled = soakRounds > 0 || soakDurationMs > 0;
const requestedCtrlCCount = Number.parseInt(process.env.RIDGE_TERM_E2E_CODEX_CTRL_C_COUNT || '0', 10);
const ctrlCCount = Number.isFinite(requestedCtrlCCount)
  ? Math.min(Math.max(requestedCtrlCCount, 0), 5_000)
  : 0;
const requestedCtrlCIntervalMs = Number.parseInt(process.env.RIDGE_TERM_E2E_CODEX_CTRL_C_INTERVAL_MS || '25', 10);
const ctrlCIntervalMs = Number.isFinite(requestedCtrlCIntervalMs)
  ? Math.min(Math.max(requestedCtrlCIntervalMs, 0), 1_000)
  : 25;
const ctrlCEnabled = ctrlCCount > 0;
const nonce = `${Date.now()}-${crypto.randomBytes(6).toString('hex')}`;
const expected = `RIDGE_CODEX_RESULT_${crypto.createHash('sha256').update(nonce).digest('hex').slice(0, 20).toUpperCase()}`;
const challenge = expected.toLowerCase();
const selectionMarker = `RIDGE_MOUSE_SELECTION_${crypto.randomBytes(8).toString('hex')}`;
fs.mkdirSync(artifactDir, { recursive: true });

const codexHome = process.env.RIDGE_TERM_E2E_CODEX_HOME;
const codexHomePrefix = codexHome
  ? `$env:CODEX_HOME='${codexHome.replaceAll("'", "''")}'; `
  : '';
const codexLaunch = codexHomePrefix + [
  'codex --yolo --disable apps',
  "-c 'model_reasoning_effort=\"low\"'",
  // Keep the acceptance test deterministic on networks whose TLS proxy
  // breaks Codex WebSockets while HTTPS/SSE remains healthy.
  "-c 'model_provider=\"chatgpt_http\"'",
  "-c 'model_providers.chatgpt_http.name=\"ChatGPT HTTP\"'",
  "-c 'model_providers.chatgpt_http.base_url=\"https://chatgpt.com/backend-api/codex\"'",
  "-c 'model_providers.chatgpt_http.wire_api=\"responses\"'",
  "-c 'model_providers.chatgpt_http.requires_openai_auth=true'",
  "-c 'model_providers.chatgpt_http.supports_websockets=false'",
  ...(codexHome ? [] : [
    "-c 'mcp_servers.codegraph.enabled=false'",
    "-c 'mcp_servers.node_repl.enabled=false'",
    "-c 'mcp_servers.notebooklm-mcp.enabled=false'",
    "-c 'mcp_servers.ridge.enabled=false'",
  ]),
].join(' ');

function httpJson(route) {
  return new Promise((resolve, reject) => {
    http.get({ host: '127.0.0.1', port, path: route, timeout: 3000 }, (response) => {
      let body = '';
      response.on('data', (chunk) => (body += chunk));
      response.on('end', () => {
        try { resolve(JSON.parse(body)); } catch (error) { reject(error); }
      });
    }).on('error', reject);
  });
}

class Cdp {
  constructor(url) {
    this.ws = new WebSocket(url);
    this.nextId = 0;
    this.pending = new Map();
    this.events = [];
    this.connectionError = null;
    this.closed = false;
    this.closePromise = new Promise((resolve) => { this.resolveClose = resolve; });
  }

  async open() {
    await new Promise((resolve, reject) => {
      let opened = false;
      const failOpen = (error) => {
        if (!opened) reject(error);
      };
      this.ws.onopen = () => {
        opened = true;
        resolve();
      };
      this.ws.onerror = (event) => {
        const error = this.transportError(`CDP WebSocket error: ${event?.message || event?.error?.message || event?.type || 'unknown'}`);
        this.fail(error);
        failOpen(error);
      };
      this.ws.onclose = (event) => {
        const reason = JSON.stringify(String(event?.reason || ''));
        const error = this.transportError(`CDP WebSocket closed: code=${event?.code ?? 'unknown'} reason=${reason}`);
        this.closed = true;
        this.fail(error, true);
        this.resolveClose(error);
        failOpen(error);
      };
      this.ws.onmessage = (event) => {
        try {
          const message = JSON.parse(event.data);
          const pending = this.pending.get(message.id);
          if (pending) {
            this.pending.delete(message.id);
            if (message.error) pending.reject(new Error(`${pending.method}: ${JSON.stringify(message.error)}`));
            else pending.resolve(message);
          } else if (message.method) {
            if (this.events.length >= 4096) this.events.shift();
            this.events.push(message);
          }
        } catch (error) {
          this.fail(this.transportError(`CDP WebSocket message failed: ${error.message || error}`));
        }
      };
    });
  }

  send(method, params = {}) {
    if (this.connectionError) return Promise.reject(this.connectionError);
    if (this.ws.readyState !== WebSocket.OPEN) {
      const error = this.transportError(`CDP WebSocket is not open for ${method}: readyState=${this.ws.readyState}`);
      this.fail(error);
      return Promise.reject(error);
    }
    const id = ++this.nextId;
    return new Promise((resolve, reject) => {
      this.pending.set(id, { method, resolve, reject });
      try {
        this.ws.send(JSON.stringify({ id, method, params }));
      } catch (error) {
        this.pending.delete(id);
        const failure = this.transportError(`CDP WebSocket send failed for ${method}: ${error.message || error}`);
        this.fail(failure);
        reject(failure);
      }
    });
  }

  transportError(message) {
    const error = new Error(message);
    error.code = 'ERR_CDP_TRANSPORT';
    Object.defineProperty(error, 'cdp', { value: this });
    return error;
  }

  fail(error, replace = false) {
    if (replace || !this.connectionError) this.connectionError = error;
    for (const pending of this.pending.values()) pending.reject(this.connectionError);
    this.pending.clear();
  }

  async terminalError() {
    if (!this.closed) await Promise.race([this.closePromise, sleep(50)]);
    return this.connectionError;
  }

  async evaluate(expression) {
    const response = await this.send('Runtime.evaluate', {
      expression,
      awaitPromise: true,
      returnByValue: true,
    });
    if (response.result?.exceptionDetails) {
      throw new Error(`CDP evaluation failed: ${JSON.stringify(response.result.exceptionDetails).slice(0, 1200)}`);
    }
    return response.result?.result?.value;
  }

  close() {
    try { this.ws.close(); } catch { /* already closed */ }
  }
}

const isCdpTransportError = (error) => error?.code === 'ERR_CDP_TRANSPORT';

async function findTarget() {
  const deadline = Date.now() + 120_000;
  let lastError = '';
  while (Date.now() < deadline) {
    try {
      const target = (await httpJson('/json/list')).find((candidate) => isRidgeCdpTarget(candidate, expectedDevOrigin));
      if (target) return target;
    } catch (error) {
      lastError = error.message;
    }
    await sleep(500);
  }
  throw new Error(`no Ridge CDP target on 127.0.0.1:${port} for ${expectedDevOrigin || 'any current app origin'}: ${lastError}`);
}

async function waitUntil(probe, description, maxMs = timeoutMs) {
  const deadline = Date.now() + maxMs;
  let last;
  let lastError;
  while (Date.now() < deadline) {
    try {
      last = await probe();
      if (last) return last;
    } catch (error) {
      if (isCdpTransportError(error)) throw await error.cdp?.terminalError?.() || error;
      lastError = error;
    }
    await sleep(200);
  }
  try {
    last = await probe();
    if (last) return last;
  } catch (error) {
    if (isCdpTransportError(error)) throw await error.cdp?.terminalError?.() || error;
    lastError = error;
  }
  throw new Error(`${description} timed out after ${maxMs}ms; last=${JSON.stringify(last)}${lastError ? `; error=${lastError.message}` : ''}`);
}

const invoke = (cdp, command, args = {}) => cdp.evaluate(
  `window.__TAURI__.core.invoke(${JSON.stringify(command)}, ${JSON.stringify(args)})`,
);

const compact = (rows) => (rows ?? []).join('').replace(/\s+/g, '');
const hashRows = (rows) => crypto.createHash('sha256').update((rows ?? []).join('\n')).digest('hex');

/** Presentation freeze (not hide): kernel may walk; presented cell stays put; grid still changes. */
function assertPresentationCursorFreeze(frames, label) {
  if (!Array.isArray(frames) || frames.length < 2) {
    throw new Error(`${label}: need at least two rewind frames`);
  }
  const presented = frames.map((frame) => frame.presented);
  const uniquePresented = new Set(presented.map((cursor) => JSON.stringify(cursor)));
  const uniqueGrids = new Set(frames.map((frame) => frame.grid)).size;
  const uniqueKernel = new Set(frames.map((frame) => JSON.stringify(frame.kernel))).size;
  if (presented.some((cursor) => cursor == null)) {
    throw new Error(`${label}: freeze hid the presented cursor: ${JSON.stringify(presented)}`);
  }
  if (uniquePresented.size !== 1) {
    throw new Error(`${label}: presented cursor moved during rewind: ${JSON.stringify(presented)}`);
  }
  if (uniqueGrids < 2) {
    throw new Error(`${label}: grid must still paint during freeze`);
  }
  if (uniqueKernel < 2) {
    throw new Error(`${label}: rewind fixture did not move the kernel cursor`);
  }
}

function runCursorFreezeUnit() {
  const frames = [
    { grid: 'seed', kernel: { row: 0, col: 4 }, presented: { row: 0, col: 4 } },
    { grid: 'A', kernel: { row: 1, col: 2 }, presented: { row: 0, col: 4 } },
    { grid: 'AB', kernel: { row: 2, col: 5 }, presented: { row: 0, col: 4 } },
    { grid: 'ABC', kernel: { row: 1, col: 8 }, presented: { row: 0, col: 4 } },
  ];
  assertPresentationCursorFreeze(frames, 'rewind-freeze');
  const hideFrames = frames.map((frame, index) => ({
    ...frame,
    presented: index === 0 ? frame.presented : null,
  }));
  let hid = false;
  try {
    assertPresentationCursorFreeze(hideFrames, 'rewind-hide');
  } catch (error) {
    hid = /hid the presented cursor/.test(error.message);
  }
  if (!hid) throw new Error('hide fixture must fail freeze assertion');
  console.log(JSON.stringify({ ok: true, unit: 'cursor-freeze' }));
}

if (process.argv.includes('--cursor-freeze-unit')) {
  runCursorFreezeUnit();
  process.exit(0);
}

function hookCall(cdp, method, ...args) {
  return cdp.evaluate(
    `window.__windE2E.${method}(${args.map((arg) => JSON.stringify(arg)).join(',')})`,
  );
}

async function visibleRows(cdp, paneId) {
  return hookCall(cdp, 'visibleText', paneId);
}

async function writePty(cdp, workspaceId, paneId, data) {
  await invoke(cdp, 'write_to_pty', { workspaceId, paneId, data });
}

async function testRoundedBoxDecstbmFixture(cdp, workspaceId, paneId) {
  const command = '$e=[char]27;$nw=[char]0x256D;$ne=[char]0x256E;$sw=[char]0x2570;$se=[char]0x256F;$v=[char]0x2502;[Console]::Write($e+"[2J"+$e+"[H"+$e+"[2;1H"+"RIDGE_DECSTBM_OUTSIDE"+$e+"[9;1H"+"RIDGE_DECSTBM_BELOW"+$e+"[3;8r"+$e+"[3;1H"+"RIDGE_SCROLL_A`r`n"+"RIDGE_SCROLL_B`r`n"+"RIDGE_SCROLL_C`r`n"+"RIDGE_SCROLL_D`r`n"+"RIDGE_SCROLL_E`r`n"+"RIDGE_SCROLL_F`r`n"+"RIDGE_SCROLL_G`r`n"+$e+"[r"+$e+"[10;1H"+"RIDGE_DECSTBM_RESET`r`n"+$e+"[12;1H"+$nw+"──────────────"+$ne+"`r`n"+$v+" RIDGE FONT "+$v+"`r`n"+$sw+"──────────────"+$se+"`r`n"+"RIDGE_BOX_FINAL`r`n")';
  await writePty(cdp, workspaceId, paneId, `${command}\r`);
  const markers = [
    'RIDGE_DECSTBM_OUTSIDE',
    'RIDGE_DECSTBM_BELOW',
    'RIDGE_DECSTBM_RESET',
    'RIDGE_SCROLL_G',
    'RIDGE_BOX_FINAL',
  ];
  const hasMarkers = (rows) => {
    const text = (rows ?? []).join('\n');
    const trimmed = (rows ?? []).map((line) => line.trim());
    return markers.every((marker) => text.includes(marker))
      && ['╭', '╮', '╯', '╰'].every((glyph) => text.includes(glyph))
      && trimmed.some((line) => /^╭──────────────╮$/.test(line))
      && trimmed.some((line) => /^│ RIDGE FONT │$/.test(line))
      && trimmed.some((line) => /^╰──────────────╯$/.test(line));
  };
  await waitUntil(async () => {
    const rows = await visibleRows(cdp, paneId);
    return hasMarkers(rows) ? rows : null;
  }, 'rounded box DECSTBM fixture', 10_000);
  const rows = await waitUntil(async () => {
    const nextRows = await visibleRows(cdp, paneId);
    if (!hasMarkers(nextRows)) return null;
    await cdp.evaluate('new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)))');
    const presentedRows = await visibleRows(cdp, paneId);
    return hasMarkers(presentedRows) ? presentedRows : null;
  }, 'stable rounded box DECSTBM markers', 5_000);
  const text = rows.join('\n');
  for (const glyph of ['╭', '╮', '╯', '╰']) {
    if (!text.includes(glyph)) throw new Error(`rounded glyph missing from fixture: ${glyph}`);
  }
  const outsideRow = rows.findIndex((line) => line.includes('RIDGE_DECSTBM_OUTSIDE'));
  const belowRow = rows.findIndex((line) => line.includes('RIDGE_DECSTBM_BELOW'));
  const trimmed = rows.map((line) => line.trim());
  const frameRows = {
    top: trimmed.findIndex((line) => /^╭──────────────╮$/.test(line)),
    middle: trimmed.findIndex((line) => /^│ RIDGE FONT │$/.test(line)),
    bottom: trimmed.findIndex((line) => /^╰──────────────╯$/.test(line)),
  };
  if (outsideRow !== 1 || belowRow !== 8 || frameRows.top < 11
    || frameRows.middle !== frameRows.top + 1 || frameRows.bottom !== frameRows.top + 2) {
    throw new Error(`DECSTBM sentinels or final box moved: ${JSON.stringify({ outsideRow, belowRow, frameRows })}`);
  }
  if (text.includes('RIDGE_SCROLL_A')) throw new Error('DECSTBM region did not scroll its first marker');
  return {
    markers,
    glyphs: ['╭', '╮', '╯', '╰'],
    outsideRow,
    belowRow,
    frameRows,
    resetMargins: true,
  };
}

async function testNativeGlyphFixture(cdp, workspaceId, paneId) {
  const command = [
    '$e=[char]27',
    '[Console]::OutputEncoding=[System.Text.UTF8Encoding]::new($false)',
    '$zh=[string][char]0x4E2D+[string][char]0x6587+[string][char]0x6548+[string][char]0x679C',
    '$face=[char]::ConvertFromUtf32(0x1F600)',
    '$cake=[char]::ConvertFromUtf32(0x1F382)',
    '$rocket=[char]::ConvertFromUtf32(0x1F680)',
    '$heart=[string][char]0x2764+[string][char]0xFE0F',
    '$keycap="1"+[string][char]0xFE0F+[string][char]0x20E3',
    '$woman=[char]::ConvertFromUtf32(0x1F469)',
    '$laptop=[char]::ConvertFromUtf32(0x1F4BB)',
    '$womanDev=$woman+[string][char]0x200D+$laptop',
    '$symbol=[string][char]0x2194+" "+[string][char]0x2713',
    '[Console]::Write($e+"[2J"+$e+"[H"+"RIDGE_NATIVE_GLYPH`r`n"+"CJK: "+$zh+" |`r`n"+"EMOJI: "+$face+$cake+$rocket+"|`r`n"+"CLUSTER: "+$heart+" "+$keycap+" "+$womanDev+"|`r`n"+"SYMBOL: "+$symbol+"|`r`n")',
  ].join(';');
  await writePty(cdp, workspaceId, paneId, `${command}\r`);

  const expected = {
    marker: 'RIDGE_NATIVE_GLYPH',
    cjk: 'CJK: \u4e2d\u6587\u6548\u679c |',
    emoji: 'EMOJI: \u{1f600}\u{1f382}\u{1f680}|',
    // visibleText exposes the base cell; variation selectors and ZWJ tails
    // remain in the grapheme sidecar consumed by the rasterizer.
    cluster: 'CLUSTER: \u2764 1 \u{1f469}|',
    symbol: 'SYMBOL: \u2194 \u2713|',
  };
  const rows = await waitUntil(async () => {
    const nextRows = await visibleRows(cdp, paneId);
    const text = (nextRows ?? []).join('\n');
    return Object.values(expected).every((value) => text.includes(value)) ? nextRows : null;
  }, 'native CJK and emoji glyph fixture', 10_000);

  const rowIndices = Object.fromEntries(Object.entries(expected).map(([key, value]) => [
    key,
    rows.findIndex((line) => line.includes(value)),
  ]));
  if (Object.values(rowIndices).some((row) => row < 0)) {
    throw new Error(`native glyph fixture rows missing: ${JSON.stringify(rowIndices)}`);
  }
  return { expected, rowIndices };
}

async function foreground(cdp, workspaceId, paneId) {
  return invoke(cdp, 'get_pane_foreground_process', { workspaceId, paneId });
}

function codexVisible(rows) {
  return (rows ?? []).some((line) => /(?:OpenAI Codex|RidgeCode)/i.test(line));
}

function codexUpdatePromptVisible(rows) {
  const text = (rows ?? []).join('\n');
  return /Update available![ \t]+\d+(?:\.\d+){2}[ \t]*->[ \t]*\d+(?:\.\d+){2}/i.test(text)
    && /(?:^|\n)[ \t]*(?:[>❯▸›][ \t]*)?1\.[ \t]*Update now(?: \(runs (?:npm install -g @openai\/codex|`npm install -g @openai\/codex`)\))?[ \t]*$/im.test(text)
    && /(?:^|\n)[ \t]*(?:[>❯▸›][ \t]*)?2\.[ \t]*Skip[ \t]*$/im.test(text)
    && /(?:^|\n)[ \t]*Press enter to continue[ \t]*$/im.test(text);
}

function latestCodexSegment(rows) {
  const values = rows ?? [];
  let start = -1;
  for (let index = 0; index < values.length; index++) {
    if (/(?:OpenAI Codex|RidgeCode)/i.test(values[index])) start = index;
  }
  return start >= 0 ? values.slice(start) : [];
}

function shellPromptVisible(rows, cursor = null) {
  const isPrompt = (line) => /^PS\s+[A-Za-z]:\\.*>\s*$/.test(line.trim());
  if (cursor && Number.isInteger(cursor.row) && isPrompt(rows[cursor.row] || '')) return true;
  const last = [...(rows ?? [])].reverse().find((line) => line.trim().length > 0) ?? '';
  return isPrompt(last);
}

function codexOwnerState(rows, cursor = null) {
  if (shellPromptVisible(rows, cursor)) return 'shell';
  if (codexUpdatePromptVisible(rows)) return 'update-prompt';
  return codexVisible(rows) ? 'codex' : null;
}

async function skipCodexUpdatePrompt(cdp, workspaceId, paneId, promptSummary = null) {
  const rows = await visibleRows(cdp, paneId);
  if (!codexUpdatePromptVisible(rows)) {
    throw new Error('refusing to send update choice: exact Codex update prompt is no longer visible');
  }
  if (promptSummary) Object.assign(promptSummary, { detected: true, handled: false, choice: 'Skip', key: '2' });
  // Exact page rechecked above; send only Skip, never Update now.
  await writePty(cdp, workspaceId, paneId, '2\r');
  const owner = await waitUntil(async () => {
    const nextRows = await visibleRows(cdp, paneId);
    const cursor = await hookCall(cdp, 'kernelCursor', paneId);
    const state = codexOwnerState(nextRows, cursor);
    return state === 'update-prompt' ? null : state;
  }, 'Codex TUI after skipping update prompt', 30_000);
  if (owner !== 'codex') throw new Error(`Codex update Skip returned unexpected owner: ${owner}`);
  if (promptSummary) promptSummary.handled = true;
  return owner;
}

async function paneProbe(cdp, wantedPaneId) {
  return cdp.evaluate(`(() => {
    const panes = [...document.querySelectorAll('.rg-pane-container')]
      .map((element) => {
        const rect = element.getBoundingClientRect();
        return { element, rect, paneId: element.dataset.rgPaneId || '' };
      })
      .filter((item) => item.rect.width > 20 && item.rect.height > 20);
    const item = panes.find((candidate) => candidate.paneId === ${JSON.stringify(wantedPaneId || '')}) || panes[0];
    if (!item || !window.__windE2E) return null;
    const anchor = window.__windE2E.inputAnchorResolved(item.paneId);
    return {
      paneId: item.paneId,
      rect: { left: item.rect.left, top: item.rect.top, width: item.rect.width, height: item.rect.height },
      rows: window.__windE2E.rows(item.paneId),
      cols: window.__windE2E.cols(item.paneId),
      anchor,
      backend: window.__windE2E.backendName(item.paneId) || item.element.dataset.rgBackend || null,
    };
  })()`);
}

async function visiblePaneIds(cdp) {
  return cdp.evaluate(`([...document.querySelectorAll('.rg-pane-container')]
    .filter((element) => {
      const rect = element.getBoundingClientRect();
      return rect.width > 20 && rect.height > 20;
    })
    .map((element) => element.dataset.rgPaneId || '')
    .filter(Boolean))`);
}

async function testPaneLastRowWrapIsolation(cdp, workspaceId, sourcePaneId) {
  const beforePaneIds = await visiblePaneIds(cdp);
  let testPaneId = null;
  try {
    const split = await invoke(cdp, 'split_pane', { paneId: sourcePaneId, direction: 'horizontal' });
    testPaneId = split?.pane_id ?? split?.paneId ?? null;
    if (!testPaneId) throw new Error(`split_pane returned no pane id: ${JSON.stringify(split)}`);
    const paneIds = await waitUntil(async () => {
      const ids = await visiblePaneIds(cdp);
      return ids.includes(testPaneId) && ids.length > beforePaneIds.length ? ids : null;
    }, 'wrap-isolation split pane mount', 15_000);
    await waitUntil(async () => {
      const rows = await visibleRows(cdp, testPaneId);
      return shellPromptVisible(rows, await hookCall(cdp, 'kernelCursor', testPaneId));
    }, 'wrap-isolation PowerShell prompt', 30_000);
    const geometry = await paneProbe(cdp, testPaneId);
    if (!geometry || geometry.rows < 3 || geometry.cols < 8) {
      throw new Error(`wrap-isolation pane geometry unavailable: ${JSON.stringify(geometry)}`);
    }
    await dispatchClick(cdp, geometry);
    await waitUntil(() => cdp.evaluate(
      `document.querySelector(${JSON.stringify(`.rg-pane-container[data-rg-pane-id="${testPaneId}"]`)})?.dataset.rgPaneActive === 'true'`,
    ), 'wrap-isolation active pane', 5_000);
    // Let the focus transition remove the old pane cursor before render counts
    // start; the measured window then contains only the bottom-row wrap.
    await sleep(600);
    const readyMarker = `RIDGE_WRAP_READY_${crypto.randomBytes(4).toString('hex').toUpperCase()}`;
    await writePty(cdp, workspaceId, testPaneId, `Write-Output '${readyMarker}'\r`);
    await waitUntil(async () => {
      const visible = await visibleRows(cdp, testPaneId);
      return visible.some((line) => line.trim() === readyMarker) ? true : null;
    }, 'wrap-isolation PTY readiness', 10_000);

    const marker = `RIDGE_WRAP_LATEST_${crypto.randomBytes(6).toString('hex').toUpperCase()}`;
    const scrollbackBefore = await hookCall(cdp, 'scrollbackLen', testPaneId);
    await cdp.evaluate(`(() => {
      performance.clearMeasures();
      window.__ridgeTermProbe.reset();
      window.__RIDGE_PERF_TRACE = true;
    })()`);
    const startedAt = performance.now();
    const command = `$e=[char]27;[Console]::Out.Write($e+"[?1049h"+$e+"[2J"+$e+"[H"+$e+"[${geometry.rows};${geometry.cols}H"+"A"+"${marker}");Start-Sleep -Milliseconds 3000`;
    await writePty(cdp, workspaceId, testPaneId, `${command}\r`);
    let observedRows = [];
    let rows;
    try {
      rows = await waitUntil(async () => {
        const visible = await visibleRows(cdp, testPaneId);
        observedRows = visible;
        const tail = visible.slice(-4).join('').replace(/\s+/g, '');
        return tail.includes(`A${marker}`) ? visible : null;
      }, 'latest text after bottom-row wrap', 10_000);
    } catch (error) {
      const diagnostic = {
        geometry,
        cursor: await hookCall(cdp, 'kernelCursor', testPaneId),
        dec: await hookCall(cdp, 'kernelDecState', testPaneId),
        tailRows: observedRows.slice(-4),
        screenshot: await capture(cdp, '10-pane-last-row-wrap-failure.png'),
      };
      throw new Error(`${error.message}; diagnostic=${JSON.stringify(diagnostic)}`);
    }
    const elapsedMs = Math.round((performance.now() - startedAt) * 10) / 10;
    const measurements = await cdp.evaluate(`(() => {
      const prefix = 'rg.terminal.render.pane.';
      const counts = {};
      for (const entry of performance.getEntriesByType('measure')) {
        if (!entry.name.startsWith(prefix)) continue;
        const paneId = entry.name.slice(prefix.length);
        counts[paneId] = (counts[paneId] || 0) + 1;
      }
      return { counts, eventLoop: window.__ridgeTermProbe.read() };
    })()`);
    const siblingPaneIds = paneIds.filter((paneId) => paneId !== testPaneId);
    const siblingRenderCounts = Object.fromEntries(
      siblingPaneIds.map((paneId) => [paneId, measurements.counts[paneId] ?? 0]),
    );
    const targetRenderCount = measurements.counts[testPaneId] ?? 0;
    if (targetRenderCount < 1 || Object.values(siblingRenderCounts).some((count) => count !== 0)) {
      throw new Error(`bottom-row wrap repainted the wrong pane set: ${JSON.stringify({
        testPaneId, targetRenderCount, siblingRenderCounts,
      })}`);
    }
    if (measurements.eventLoop.longtask50msCount !== 0 || measurements.eventLoop.p95 > 25) {
      throw new Error(`bottom-row wrap stalled the event loop: ${JSON.stringify(measurements.eventLoop)}`);
    }
    const screenshot = await capture(cdp, '10-pane-last-row-wrap.png');
    return {
      testPaneId,
      siblingPaneIds,
      targetRenderCount,
      siblingRenderCounts,
      latestVisibleWithoutScroll: true,
      elapsedMs,
      eventLoop: measurements.eventLoop,
      scrollbackBefore,
      scrollbackAfter: await hookCall(cdp, 'scrollbackLen', testPaneId),
      tailRows: rows.slice(-2),
      screenshot,
    };
  } finally {
    await cdp.evaluate(`(() => {
      window.__RIDGE_PERF_TRACE = false;
      performance.clearMeasures();
    })()`).catch(() => {});
    if (testPaneId) {
      await invoke(cdp, 'close_pane', { paneId: testPaneId });
      await waitUntil(async () => !(await visiblePaneIds(cdp)).includes(testPaneId), 'wrap-isolation pane cleanup', 10_000);
    }
    await sleep(300);
    await hookCall(cdp, 'installPtyWriteSpy', sourcePaneId).catch(() => {});
    await cdp.evaluate('window.__ridgeTermProbe.reset()').catch(() => {});
  }
}

async function capture(cdp, name, clip = null) {
  const response = await cdp.send('Page.captureScreenshot', {
    format: 'png',
    ...(clip ? { clip } : {}),
  });
  if (!response.result?.data) throw new Error(`no screenshot data for ${name}`);
  const target = path.join(artifactDir, name);
  fs.writeFileSync(target, Buffer.from(response.result.data, 'base64'));
  return target;
}

async function measureMonochromeSharpness(file, clip) {
  const { data, info } = await sharp(file)
    .extract({
      left: Math.max(0, Math.round(clip.x)),
      top: Math.max(0, Math.round(clip.y)),
      width: Math.max(1, Math.round(clip.width)),
      height: Math.max(1, Math.round(clip.height)),
    })
    .raw()
    .toBuffer({ resolveWithObject: true });
  let total = 0;
  let low = 0;
  let middle = 0;
  let solid = 0;
  for (let offset = 0; offset < data.length; offset += info.channels) {
    const red = data[offset];
    const green = data[offset + 1];
    const blue = data[offset + 2];
    if (Math.max(red, green, blue) - Math.min(red, green, blue) > 4) continue;
    const luminance = Math.round((red + green + blue) / 3);
    if (luminance === 0) continue;
    total += 1;
    if (luminance < 56) low += 1;
    else if (luminance < 168) middle += 1;
    else solid += 1;
  }
  return {
    total,
    low,
    middle,
    solid,
    solidRatio: total > 0 ? solid / total : 0,
    transitionRatio: total > 0 ? (low + middle) / total : 1,
  };
}

async function forceWebgpuUnavailable(cdp, { disableWebgl = false } = {}) {
  const response = await cdp.send('Page.addScriptToEvaluateOnNewDocument', { source: `(() => {
    Object.defineProperty(Navigator.prototype, 'gpu', {
      configurable: true,
      get: () => undefined,
    });
    const originalGetContext = HTMLCanvasElement.prototype.getContext;
    window.__ridge2dContextCalls = [];
    window.__ridgeGpuContextCalls = [];
    HTMLCanvasElement.prototype.getContext = function(type, ...args) {
      const normalized = String(type).toLowerCase();
      window.__ridgeGpuContextCalls.push(normalized);
      if (normalized === '2d') {
        window.__ridge2dContextCalls.push({
          width: this.width,
          height: this.height,
          stack: new Error('Canvas2D context requested').stack ?? '',
        });
      }
      if (${JSON.stringify(disableWebgl)} && (normalized === 'webgl' || normalized === 'webgl2')) return null;
      return originalGetContext.call(this, type, ...args);
    };
  })();` });
  cdp.events.length = 0;
  await cdp.send('Page.reload', { ignoreCache: true });
  return response.result?.identifier ?? null;
}

async function testWebgpuUnavailable(cdp) {
  const state = await waitUntil(async () => cdp.evaluate(`(() => {
    const alert = [...document.querySelectorAll('[role="alert"]')]
      .find((element) => element.textContent?.includes('WEBGPU_INIT_FAILED'));
    if (!alert || alert.getBoundingClientRect().width <= 0 || alert.getBoundingClientRect().height <= 0) return null;
    const paneIds = [...document.querySelectorAll('[data-rg-pane-id]')]
      .map((element) => element.dataset.rgPaneId)
      .filter(Boolean);
    return {
      alert: alert.textContent?.trim() ?? '',
      navigatorGpuAvailable: Boolean(navigator.gpu),
      canvas2dCalls: window.__ridge2dContextCalls ?? [],
      gpuContextCalls: window.__ridgeGpuContextCalls ?? [],
      backendNames: paneIds.map((paneId) => window.__windE2E?.backendName?.(paneId) ?? null),
      backendAttributes: [...document.querySelectorAll('[data-rg-backend]')]
        .map((element) => element.dataset.rgBackend ?? null),
      canvasCount: document.querySelectorAll('canvas').length,
    };
  })()`), 'visible WebGPU initialization failure', 60_000);
  await sleep(250);
  const runtimeExceptions = cdp.events
    .filter((event) => event.method === 'Runtime.exceptionThrown')
    .map((event) => event.params?.exceptionDetails?.exception?.description
      || event.params?.exceptionDetails?.text
      || 'unknown runtime exception');
  if (state.navigatorGpuAvailable
    || state.canvas2dCalls.length > 0
    || state.backendNames.some(Boolean)
    || state.backendAttributes.some((backend) => backend?.toLowerCase() === 'canvas2d')
    || runtimeExceptions.length > 0) {
    throw new Error(`WebGPU failure fixture violated fail-closed contract: ${JSON.stringify({ state, runtimeExceptions })}`);
  }
  return { ...state, runtimeExceptions };
}

async function dispatchClick(cdp, probe) {
  const x = Math.round(probe.rect.left + probe.rect.width * 0.5);
  const y = Math.round(probe.rect.top + probe.rect.height * 0.5);
  await cdp.send('Input.dispatchMouseEvent', { type: 'mousePressed', x, y, button: 'left', buttons: 1, clickCount: 1 });
  await cdp.send('Input.dispatchMouseEvent', { type: 'mouseReleased', x, y, button: 'left', buttons: 0, clickCount: 1 });
}

async function focusTerminalInput(cdp, paneId) {
  const focused = await cdp.evaluate(`(() => {
    const pane = document.querySelector(${JSON.stringify(`.rg-pane-container[data-rg-pane-id="${paneId}"]`)});
    const target = pane?.querySelector('.rg-ime-helper') || pane;
    if (!target) return false;
    target.focus();
    return document.activeElement === target;
  })()`);
  if (!focused) throw new Error('terminal input target could not be focused');
}

async function typeTerminalPrompt(cdp, paneId, text) {
  await focusTerminalInput(cdp, paneId);
  await cdp.send('Input.insertText', { text });
  await sleep(100);
  await cdp.send('Input.dispatchKeyEvent', {
    type: 'keyDown',
    key: 'Enter',
    code: 'Enter',
    windowsVirtualKeyCode: 13,
    nativeVirtualKeyCode: 13,
  });
  await cdp.send('Input.dispatchKeyEvent', {
    type: 'keyUp',
    key: 'Enter',
    code: 'Enter',
    windowsVirtualKeyCode: 13,
    nativeVirtualKeyCode: 13,
  });
}

async function dispatchCtrlCRepeat(cdp, count, intervalMs) {
  const key = {
    key: 'c',
    code: 'KeyC',
    modifiers: 2,
    windowsVirtualKeyCode: 67,
    nativeVirtualKeyCode: 67,
  };
  let keyDown = false;
  try {
    for (let index = 0; index < count; index++) {
      await cdp.send('Input.dispatchKeyEvent', {
        ...key,
        type: 'keyDown',
        autoRepeat: index > 0,
      });
      keyDown = true;
      if (index + 1 < count && intervalMs > 0) await sleep(intervalMs);
    }
  } finally {
    if (keyDown) await cdp.send('Input.dispatchKeyEvent', { ...key, type: 'keyUp' }).catch(() => {});
  }
}

async function dragRow(cdp, probe, row) {
  const cellW = probe.anchor?.cellW || probe.rect.width / Math.max(1, probe.cols);
  const cellH = probe.anchor?.cellH || probe.rect.height / Math.max(1, probe.rows);
  const startX = Math.round(probe.rect.left + cellW * 0.5);
  const endX = Math.round(probe.rect.left + cellW * Math.max(2, probe.cols - 1.5));
  const y = Math.round(probe.rect.top + cellH * (row + 0.5));
  await cdp.send('Input.dispatchMouseEvent', { type: 'mousePressed', x: startX, y, button: 'left', buttons: 1, clickCount: 1 });
  await cdp.send('Input.dispatchMouseEvent', { type: 'mouseMoved', x: endX, y, button: 'left', buttons: 1 });
  await sleep(100);
  await cdp.send('Input.dispatchMouseEvent', { type: 'mouseReleased', x: endX, y, button: 'left', buttons: 0, clickCount: 1 });
}

async function installPerformanceProbe(cdp) {
  await cdp.evaluate(`(() => {
    window.__ridgeTermProbe?.stop?.();
    const state = { samples: [], longtasks: [], interval: 0, observer: null };
    const maxSamples = 4096;
    const append = (values, value) => {
      if (values.length >= maxSamples) values.shift();
      values.push(value);
    };
    const tick = 50;
    let last = performance.now();
    state.interval = setInterval(() => {
      const now = performance.now();
      append(state.samples, Math.max(0, now - last - tick));
      last = now;
    }, tick);
    try {
      state.observer = new PerformanceObserver((list) => {
        for (const entry of list.getEntries()) append(state.longtasks, Math.round(entry.duration));
      });
      state.observer.observe({ entryTypes: ['longtask'] });
    } catch {}
    window.__ridgeTermProbe = {
      reset() {
        state.samples.length = 0;
        state.longtasks.length = 0;
        last = performance.now();
      },
      read() {
        const sorted = state.samples.slice().sort((a, b) => a - b);
        const quantile = (p) => sorted.length ? Math.round(sorted[Math.floor((sorted.length - 1) * p)]) : 0;
        return {
          samples: sorted.length,
          p50: quantile(0.5),
          p95: quantile(0.95),
          max: Math.round(sorted[sorted.length - 1] || 0),
          longtasks: state.longtasks.length,
          longtaskMax: Math.max(0, ...state.longtasks),
          longtask50msCount: state.longtasks.filter((duration) => duration >= 50).length,
        };
      },
      stop() { clearInterval(state.interval); state.observer?.disconnect?.(); },
    };
    return true;
  })()`);
}

async function rotateTheme(cdp, paneId) {
  const before = await hookCall(cdp, 'themeSnapshot');
  if (!before?.background) throw new Error(`terminal theme unavailable: ${JSON.stringify(before)}`);
  const probeTheme = { ...before, background: '#123456', foreground: '#f5f7ff', cursor: '#ffcc00' };
  await hookCall(cdp, 'setTheme', probeTheme);
  const changed = await waitUntil(async () => {
    const value = await hookCall(cdp, 'kernelThemeProbe', paneId);
    return value?.bg?.toLowerCase() === '#123456ff' ? value : null;
  }, 'kernel theme rotation', 15_000);
  await hookCall(cdp, 'setTheme', before);
  const restored = await waitUntil(async () => {
    const value = await hookCall(cdp, 'kernelThemeProbe', paneId);
    return value?.bg?.toLowerCase() !== '#123456ff' ? value : null;
  }, 'kernel theme restoration', 15_000);
  return { changed, restored };
}

async function resizePane(cdp, paneId) {
  const before = await paneProbe(cdp, paneId);
  await cdp.evaluate(`(() => {
    const element = document.querySelector(${JSON.stringify(`.rg-pane-container[data-rg-pane-id="${paneId}"]`)});
    if (!element) return false;
    window.__ridgeTermResizeRestore = {
      element,
      width: element.style.width,
      maxWidth: element.style.maxWidth,
      height: element.style.height,
      maxHeight: element.style.maxHeight,
    };
    const rect = element.getBoundingClientRect();
    element.style.width = Math.max(320, rect.width - 120) + 'px';
    element.style.maxWidth = Math.max(320, rect.width - 120) + 'px';
    element.style.height = Math.max(240, rect.height - 80) + 'px';
    element.style.maxHeight = Math.max(240, rect.height - 80) + 'px';
    return true;
  })()`);
  const shrunk = await waitUntil(async () => {
    const value = await paneProbe(cdp, paneId);
    return value && (value.cols < before.cols || value.rows < before.rows) ? value : null;
  }, 'terminal pane shrink', 20_000);
  await cdp.evaluate(`(() => {
    const saved = window.__ridgeTermResizeRestore;
    if (!saved?.element) return false;
    saved.element.style.width = saved.width;
    saved.element.style.maxWidth = saved.maxWidth;
    saved.element.style.height = saved.height;
    saved.element.style.maxHeight = saved.maxHeight;
    delete window.__ridgeTermResizeRestore;
    return true;
  })()`);
  const restored = await waitUntil(async () => {
    const value = await paneProbe(cdp, paneId);
    return value && value.cols >= before.cols && value.rows >= before.rows ? value : null;
  }, 'terminal pane resize restoration', 20_000);
  return { before, shrunk, restored };
}

async function switchWorkspaceRoundTrip(cdp, originalWorkspaceId, paneId) {
  let tabs = await cdp.evaluate(`[...document.querySelectorAll('[data-ws-tab-id]')].map((element) => element.dataset.wsTabId)`);
  if (tabs.length < 2) {
    const clicked = await cdp.evaluate(`(() => {
      const button = [...document.querySelectorAll('button')]
        .find((element) => element.textContent.trim() === '+' && element.className.includes('border-dashed'));
      if (!button) return false;
      button.click();
      return true;
    })()`);
    if (!clicked) throw new Error('workspace add button unavailable');
    tabs = await waitUntil(async () => {
      const values = await cdp.evaluate(`[...document.querySelectorAll('[data-ws-tab-id]')].map((element) => element.dataset.wsTabId)`);
      return values.length >= 2 ? values : null;
    }, 'second workspace creation', 30_000);
  }
  const target = tabs.find((id) => id !== originalWorkspaceId);
  if (!target) throw new Error(`no alternate workspace: ${JSON.stringify(tabs)}`);
  const clickTab = (id) => cdp.evaluate(`document.querySelector(${JSON.stringify(`[data-ws-tab-id="${id}"]`)})?.click() ?? false`);
  await clickTab(target);
  await waitUntil(async () => (await invoke(cdp, 'get_window_active_workspace_id')) === target, 'alternate workspace activation', 30_000);
  await clickTab(originalWorkspaceId);
  await waitUntil(async () => (await invoke(cdp, 'get_window_active_workspace_id')) === originalWorkspaceId, 'original workspace restoration', 30_000);
  await waitUntil(async () => (await paneProbe(cdp, paneId))?.paneId === paneId, 'original pane remount', 30_000);
  return { target, tabCount: tabs.length };
}

async function exitCodex(cdp, workspaceId, paneId, promptSummary = null) {
  let last = null;
  const waitForShellPrompt = async (maxMs = 45_000) => waitUntil(async () => {
    const rows = await visibleRows(cdp, paneId);
    const cursor = await hookCall(cdp, 'kernelCursor', paneId);
    last = { tail: rows.slice(-8), cursor, state: await hookCall(cdp, 'kernelDecState', paneId) };
    return shellPromptVisible(rows, cursor) ? true : null;
  }, 'Codex return to PowerShell', maxMs).catch(() => false);
  const waitForTerminalOwner = async (maxMs = 45_000) => waitUntil(async () => {
    const rows = await visibleRows(cdp, paneId);
    const cursor = await hookCall(cdp, 'kernelCursor', paneId);
    last = { tail: rows.slice(-8), cursor, state: await hookCall(cdp, 'kernelDecState', paneId) };
    return codexOwnerState(rows, cursor);
  }, 'terminal owner (PowerShell or Codex)', maxMs).catch(() => null);
  const probeShell = async () => {
    const shellChallenge = `ridge_shell_ready_${crypto.randomBytes(8).toString('hex')}`;
    const shellExpected = shellChallenge.toUpperCase();
    await writePty(
      cdp,
      workspaceId,
      paneId,
      `Write-Output ('${shellChallenge}'.ToUpperInvariant())\r`,
    );
    return waitUntil(async () => {
      const rows = await visibleRows(cdp, paneId);
      const cursor = await hookCall(cdp, 'kernelCursor', paneId);
      last = { tail: rows.slice(-8), cursor, state: await hookCall(cdp, 'kernelDecState', paneId) };
      return compact(rows).includes(shellExpected) ? true : null;
    }, 'PowerShell execution probe', 1_500).catch(() => false);
  };

  // Do not inject a PowerShell probe into a live Codex composer: that turns
  // the probe into agent input and makes the next exit attempt ambiguous.
  // Cold kernel activation legitimately takes longer than the app-ready hook,
  // so establish one visible terminal owner before emitting any test input.
  let owner = await waitForTerminalOwner();
  if (owner === 'shell') return probeShell();
  if (owner === 'update-prompt') {
    await skipCodexUpdatePrompt(cdp, workspaceId, paneId, promptSummary);
    owner = 'codex';
  }
  if (owner !== 'codex') {
    // A previous fixture may leave PowerShell on a cleared screen with no
    // visible prompt. Ctrl-C asks the shell to repaint before declaring the
    // pane owner unknown; it also safely interrupts a stale child command.
    await writePty(cdp, workspaceId, paneId, '\x03');
    if (await waitForShellPrompt(2_000)) return probeShell();
    throw new Error(`neither PowerShell nor Codex owns the pane: ${JSON.stringify(last)}`);
  }
  for (let attempt = 0; attempt < 2; attempt++) {
    // Clear the shell probe if it landed in Codex's composer, then use Codex's
    // application-level exit command. Ctrl-D can close the parent PowerShell;
    // Ctrl-C alone only clears the composer in current Codex TUI releases.
    await writePty(cdp, workspaceId, paneId, '\x03');
    await sleep(150);
    // Codex can exit on its own while an MCP startup failure is being
    // reported. Re-check ownership before `/quit`; otherwise that command
    // becomes accidental PowerShell input after the process has returned.
    if (await waitForShellPrompt(800)) return probeShell();
    await writePty(cdp, workspaceId, paneId, '/quit\r');
    // Codex may paint shutdown immediately but keep the process alive while
    // in-process services flush. Poll the prompt passively; only prove the
    // shell after it actually owns the input line again.
    if (await waitForShellPrompt()) return probeShell();
  }
  throw new Error(`Codex TUI remained after bounded exit attempts: ${JSON.stringify(last)}`);
}

async function testMouse(cdp, workspaceId, paneId) {
  // ConPTY consumes DEC mouse mode output before Ridge sees it. Drive the
  // mirror mode directly, then prove the resulting click crosses the real PTY.
  let writes;
  try {
    await hookCall(cdp, 'feedPty', paneId, '\x1b[?1000h\x1b[?1006h');
    await waitUntil(async () => {
      const state = await hookCall(cdp, 'kernelDecState', paneId);
      return state?.mouseReportingModes ? state : null;
    }, 'direct WASM SGR mouse mode', 1_000);
    await hookCall(cdp, 'clearPtyWriteLog', paneId);
    await dispatchClick(cdp, await paneProbe(cdp, paneId));
    writes = await waitUntil(async () => {
      const entries = await hookCall(cdp, 'ptyWriteLog', paneId);
      const data = entries.map((entry) => entry.data).join('');
      return /\x1b\[<0;\d+;\d+[Mm]/.test(data) ? entries : null;
    }, 'mouse forwarding bytes', 10_000);
  } finally {
    await hookCall(cdp, 'feedPty', paneId, '\x1b[?1000l\x1b[?1006l');
    await waitUntil(async () => {
      const state = await hookCall(cdp, 'kernelDecState', paneId);
      return state?.mouseReportingModes === 0 ? state : null;
    }, 'direct WASM mouse mode reset', 1_000);
  }

  await writePty(cdp, workspaceId, paneId, `Write-Output '${selectionMarker}'\r`);
  const row = await waitUntil(async () => {
    const rows = await visibleRows(cdp, paneId);
    const index = rows.findIndex((line) => line.includes(selectionMarker));
    return index >= 0 ? index : null;
  }, 'selection marker output', 15_000);
  const probe = await paneProbe(cdp, paneId);
  await dragRow(cdp, probe, row);
  const selected = await waitUntil(async () => {
    const text = await hookCall(cdp, 'getSelectionText', paneId);
    return text.includes(selectionMarker) ? text : null;
  }, 'mouse drag selection', 10_000);
  return { writes: writes.map((entry) => entry.data), selected };
}

async function testIndexedGrayForeground(cdp, workspaceId, paneId) {
  const source = `ridge_indexed_gray_${crypto.randomBytes(8).toString('hex')}`;
  const marker = source.toUpperCase();
  const command = `$e=[char]27; [Console]::Out.Write($e+'[38;5;244m'+('${source}'.ToUpperInvariant())+$e+'[0m'+[Environment]::NewLine)\r`;
  await writePty(cdp, workspaceId, paneId, command);
  await waitUntil(async () => compact(await visibleRows(cdp, paneId)).includes(marker), 'indexed gray foreground', 15_000);
  return cdp.evaluate(`(() => {
    const rows = window.__windDumpRows(${JSON.stringify(paneId)}, 0, window.__windE2E.rows(${JSON.stringify(paneId)}) - 1);
    const gray = rows.flatMap((row) => row.nonSpace).filter((cell) => cell.fg === 'idx(244)');
    return { indexedCellCount: gray.length, text: gray.map((cell) => cell.ch).join('') };
  })()`);
}

/**
 * Exercise the production native-parser → binary Channel → WASM mirror lane
 * under a deliberately paced PowerShell burst. This is a default performance
 * gate; set RIDGE_TERM_E2E_BURST_LINES=0 only when a focused diagnostic must
 * omit its ~2-second workload.
 *
 * The transport counts are as important as the event-loop samples: a desktop
 * delta pane must not silently fall back to the JSON/raw-byte parser path
 * during the exact high-frequency output workload that users notice first.
 */
async function testOutputBurst(cdp, workspaceId, paneId, lineCount, mode, intervalMs) {
  // Keep every waited-for marker different from its input echo. A PowerShell
  // command line is visible before it executes; using a lower-case source and
  // waiting for its upper-case runtime result prevents a false-green burst.
  const markerSource = `ridge_burst_done_${crypto.randomBytes(8).toString('hex')}`;
  const marker = markerSource.toUpperCase();
  await cdp.evaluate(`(() => {
    window.__ridgeTermProbe.reset();
    window.__RIDGE_PERF_TRACE = true;
    window.__ridgePtyDeltaTrace = [];
    performance.clearMeasures('rg.ptyDelta.apply');
    performance.clearMeasures('rg.ptyText.feed');
    performance.clearMeasures('rg.terminal.render');
    window.__ridgeBurstFrameProbe?.stop?.();
    const state = { frames: [], raf: 0, last: performance.now() };
    const maxFrames = 4096;
    const append = (value) => {
      if (state.frames.length >= maxFrames) state.frames.shift();
      state.frames.push(value);
    };
    const tick = (now) => {
      append(now - state.last);
      state.last = now;
      state.raf = requestAnimationFrame(tick);
    };
    state.raf = requestAnimationFrame(tick);
    window.__ridgeBurstFrameProbe = {
      read() {
        const values = state.frames.slice(1);
        const sorted = values.slice().sort((a, b) => a - b);
        const quantile = (p) => sorted.length ? Math.round(sorted[Math.floor((sorted.length - 1) * p)] * 100) / 100 : 0;
        return {
          frames: values.length,
          p50: quantile(0.5),
          p95: quantile(0.95),
          max: Math.round((sorted.at(-1) || 0) * 100) / 100,
          jank25: values.filter((value) => value > 25).length,
          jank33: values.filter((value) => value > 33).length,
          jank50: values.filter((value) => value > 50).length,
        };
      },
      stop() { cancelAnimationFrame(state.raf); },
    };
  })()`);
  try {
    const delay = intervalMs > 0 ? `; Start-Sleep -Milliseconds ${intervalMs}` : '';
    const readySource = mode === 'tui'
      ? `ridge_tui_burst_ready_${crypto.randomBytes(8).toString('hex')}`
      : null;
    const readyMarker = readySource?.toUpperCase() ?? null;
    const terminalRows = mode === 'tui'
      ? await cdp.evaluate(`window.__windE2E.rows(${JSON.stringify(paneId)})`)
      : 0;
    const scrollRegionBottom = Math.max(3, terminalRows - 1);
    const command = mode === 'tui'
      ? `$e=[char]27; [Console]::Out.Write($e+'[?1049h'+$e+'[2J'+$e+'[H'+('${readySource}'.ToUpperInvariant())+$e+'[2;${scrollRegionBottom}r'+$e+'[${scrollRegionBottom};1H'); Start-Sleep -Milliseconds 300; 1..${lineCount} | ForEach-Object { [Console]::Out.Write('RIDGE_TUI_FRAME_'+$_+[char]13+[char]10)${delay} }; [Console]::Out.Write(('${markerSource}'.ToUpperInvariant())); Start-Sleep -Milliseconds 1000; [Console]::Out.Write($e+'[r'+$e+'[?1049l')\r`
      : `1..${lineCount} | ForEach-Object { [Console]::Out.WriteLine('RIDGE_BURST_LINE_' + $_)${delay} }; [Console]::Out.WriteLine('${markerSource}'.ToUpperInvariant())\r`;
    await writePty(cdp, workspaceId, paneId, command);
    const scrollbackStart = readyMarker
      ? await waitUntil(async () => {
        const rows = await visibleRows(cdp, paneId);
        return compact(rows).includes(readyMarker)
          ? { value: await hookCall(cdp, 'scrollbackLen', paneId) }
          : null;
      }, 'in-place TUI burst start', 15_000)
      : null;
    const scrollbackBefore = scrollbackStart?.value ?? null;
    await waitUntil(async () => compact(await visibleRows(cdp, paneId)).includes(marker), 'native delta output burst', 30_000);
    const rowsDuringBurst = mode === 'tui' ? await visibleRows(cdp, paneId) : null;
    const decDuringBurst = mode === 'tui' ? await hookCall(cdp, 'kernelDecState', paneId) : null;
    if (mode === 'tui' && (!decDuringBurst?.isAltScreen || !compact(rowsDuringBurst).includes(readyMarker))) {
      throw new Error(`alternate-screen scroll region was not preserved: ${JSON.stringify({ decDuringBurst, rowsDuringBurst })}`);
    }
    const screenshot = mode === 'tui' ? await capture(cdp, '08-alt-scroll-burst.png') : null;
    await sleep(250);
    const metrics = await cdp.evaluate(`(() => {
      const summarizeValues = (input) => {
        const values = [...input].sort((a, b) => a - b);
        const quantile = (p) => values.length ? Math.round(values[Math.floor((values.length - 1) * p)] * 100) / 100 : 0;
        return {
          count: values.length,
          p95: quantile(0.95),
          max: Math.round((values.at(-1) || 0) * 100) / 100,
          total: Math.round(values.reduce((sum, value) => sum + value, 0) * 100) / 100,
        };
      };
      const summarize = (name) => summarizeValues(
        performance.getEntriesByName(name).map((entry) => entry.duration),
      );
      return {
        lines: ${lineCount},
        mode: ${JSON.stringify(mode)},
        alternateScreen: ${mode === 'tui'},
        scrollRegion: ${mode === 'tui' ? JSON.stringify({ top: 2, bottom: scrollRegionBottom }) : 'null'},
        decDuringBurst: ${JSON.stringify(decDuringBurst)},
        screenshot: ${JSON.stringify(screenshot)},
        intervalMs: ${intervalMs},
        scrollbackBefore: ${scrollbackBefore ?? 'null'},
        scrollbackAfter: window.__windE2E.scrollbackLen(${JSON.stringify(paneId)}),
        delta: summarize('rg.ptyDelta.apply'),
        deltaBytes: summarizeValues(window.__ridgePtyDeltaTrace || []),
        text: summarize('rg.ptyText.feed'),
        render: summarize('rg.terminal.render'),
        eventLoop: window.__ridgeTermProbe.read(),
        frame: window.__ridgeBurstFrameProbe.read(),
      };
    })()`);
    if (mode === 'tui') {
      await cdp.evaluate('window.__ridgeTermProbe.reset()');
      await waitUntil(async () => {
        const rows = await visibleRows(cdp, paneId);
        const dec = await hookCall(cdp, 'kernelDecState', paneId);
        return dec?.isAltScreen === false && shellPromptVisible(rows, await hookCall(cdp, 'kernelCursor', paneId));
      }, 'alternate-screen burst restoration', 5_000);
      await sleep(250);
      metrics.restorationEventLoop = await cdp.evaluate('window.__ridgeTermProbe.read()');
    }
    return metrics;
  } finally {
    await cdp.evaluate(`(() => {
      window.__RIDGE_PERF_TRACE = false;
      delete window.__ridgePtyDeltaTrace;
      performance.clearMeasures('rg.ptyDelta.apply');
      performance.clearMeasures('rg.ptyText.feed');
      performance.clearMeasures('rg.terminal.render');
      window.__ridgeBurstFrameProbe?.stop?.();
      delete window.__ridgeBurstFrameProbe;
    })()`).catch(() => {});
  }
}

async function installCodexFrameProbe(cdp) {
  await cdp.evaluate(`(() => {
    window.__ridgeCodexFrameProbe?.stop?.();
    const state = { frames: [], raf: 0, last: performance.now() };
    const maxFrames = 4096;
    const append = (value) => {
      if (state.frames.length >= maxFrames) state.frames.shift();
      state.frames.push(value);
    };
    const tick = (now) => {
      append(now - state.last);
      state.last = now;
      state.raf = requestAnimationFrame(tick);
    };
    state.raf = requestAnimationFrame(tick);
    window.__ridgeCodexFrameProbe = {
      reset() {
        state.frames.length = 0;
        state.last = performance.now();
      },
      read() {
        const values = state.frames.slice();
        const sorted = values.slice().sort((a, b) => a - b);
        const quantile = (p) => sorted.length
          ? Math.round(sorted[Math.floor((sorted.length - 1) * p)] * 100) / 100
          : 0;
        return {
          frames: values.length,
          p50: quantile(0.5),
          p95: quantile(0.95),
          max: Math.round((sorted.at(-1) || 0) * 100) / 100,
          jank25: values.filter((value) => value > 25).length,
          jank33: values.filter((value) => value > 33).length,
          jank50: values.filter((value) => value > 50).length,
        };
      },
      stop() { cancelAnimationFrame(state.raf); },
    };
  })()`);
}

function summarizeSoakHeap(rounds) {
  const used = rounds
    .map((round) => round.heap?.usedSize)
    .filter((value) => Number.isFinite(value));
  if (!used.length) return { samples: 0, warmupRounds: 0, note: 'Runtime.getHeapUsage returned no usedSize' };
  const warmupRounds = Math.min(2, used.length);
  const postWarmup = used.slice(warmupRounds);
  const plateau = postWarmup.length ? postWarmup : used;
  return {
    samples: used.length,
    warmupRounds,
    initialUsedSize: used[0],
    warmupUsedSize: used[warmupRounds - 1],
    postWarmupFirst: plateau[0],
    postWarmupFinal: plateau.at(-1),
    postWarmupMin: Math.min(...plateau),
    postWarmupMax: Math.max(...plateau),
    postWarmupRange: Math.max(...plateau) - Math.min(...plateau),
    note: 'Heap trend only; no forced-GC hard gate.',
  };
}

function assertCodexPerformance(metrics, label) {
  const budgets = {
    eventLoopP95MaxMs: globalEventLoopP95MaxMs,
    longTaskMaxMs: globalLongTaskMaxMs,
    longTask50msCountMax: globalLongTask50msCountMax,
    frameP95MaxMs: burstFrameP95MaxMs,
    frameJank50Max: 0,
  };
  const exceeded = metrics.eventLoop.p95 > budgets.eventLoopP95MaxMs
    || metrics.eventLoop.longtaskMax > budgets.longTaskMaxMs
    || metrics.eventLoop.longtask50msCount > budgets.longTask50msCountMax
    || metrics.frame.p95 > budgets.frameP95MaxMs
    || metrics.frame.jank50 > budgets.frameJank50Max;
  if (exceeded) {
    throw new Error(`${label} performance budget exceeded: ${JSON.stringify({ budgets, metrics })}`);
  }
}

async function testCodexCtrlC(cdp, workspaceId, paneId, ctrlSummary) {
  const startedAt = Date.now();
  const responseTimeoutMs = Math.min(timeoutMs, 60_000);
  let lastSpy = { entries: 0, totalBytes: 0, ctrlCBytes: 0, unexpectedCodePoints: [] };
  const readSpy = async () => {
    const entries = await hookCall(cdp, 'ptyWriteLog', paneId);
    const data = entries.map((entry) => entry.data).join('');
    const unexpected = [...data].filter((value) => value !== '\x03');
    return {
      entries: entries.length,
      totalBytes: data.length,
      ctrlCBytes: [...data].filter((value) => value === '\x03').length,
      unexpectedCodePoints: unexpected.slice(0, 16).map((value) => value.codePointAt(0)),
    };
  };

  await installCodexFrameProbe(cdp);
  try {
    await cdp.evaluate('window.__ridgeTermProbe.reset(); window.__ridgeCodexFrameProbe.reset();');
    await hookCall(cdp, 'clearPtyWriteLog', paneId);
    await focusTerminalInput(cdp, paneId);
    await dispatchCtrlCRepeat(cdp, ctrlCCount, ctrlCIntervalMs);
    ctrlSummary.dispatchElapsedMs = Date.now() - startedAt;

    try {
      lastSpy = await waitUntil(async () => {
        lastSpy = await readSpy();
        return lastSpy.ctrlCBytes >= ctrlCCount ? lastSpy : null;
      }, 'Ctrl+C PTY spy bytes', Math.min(timeoutMs, 10_000));
    } catch (error) {
      ctrlSummary.pty = lastSpy;
      throw new Error(`Ctrl+C PTY spy mismatch: ${JSON.stringify({ expected: ctrlCCount, observed: lastSpy, cause: error.message })}`);
    }
    ctrlSummary.pty = lastSpy;
    if (lastSpy.ctrlCBytes !== ctrlCCount || lastSpy.totalBytes !== ctrlCCount) {
      throw new Error(`Ctrl+C PTY spy mismatch: ${JSON.stringify({ expected: ctrlCCount, observed: lastSpy })}`);
    }

    await hookCall(cdp, 'clearPtyWriteLog', paneId);
    await sleep(300);
    let lastAlive = null;
    let alive;
    try {
      alive = await waitUntil(async () => {
        const rows = await visibleRows(cdp, paneId);
        const cursor = await hookCall(cdp, 'kernelCursor', paneId);
        const processName = await foreground(cdp, workspaceId, paneId).catch(() => null);
        lastAlive = { processName, tail: rows.slice(-8), cursor };
        if (shellPromptVisible(rows, cursor)) return { status: 'shell', ...lastAlive };
        return codexVisible(rows) || /codex/i.test(processName || '')
          ? { status: 'codex', ...lastAlive }
          : null;
      }, 'Codex ownership after Ctrl+C stress', 10_000);
    } catch (error) {
      throw new Error(`Codex unavailable after Ctrl+C stress: ${JSON.stringify({ count: ctrlCCount, intervalMs: ctrlCIntervalMs, lastAlive, cause: error.message })}`);
    }
    ctrlSummary.ownerAfterStress = alive;
    if (alive.status !== 'codex') {
      throw new Error(`Codex exited during Ctrl+C stress: ${JSON.stringify({ count: ctrlCCount, intervalMs: ctrlCIntervalMs, alive })}`);
    }

    const tokenSource = `ridge_ctrl_c_recovery_${crypto.randomBytes(8).toString('hex')}`;
    const token = tokenSource.toUpperCase();
    const prompt = `Reply with only this token converted to uppercase, with no punctuation: ${tokenSource}`;
    ctrlSummary.recoveryToken = token;
    await typeTerminalPrompt(cdp, paneId, prompt);
    await waitUntil(async () => {
      const data = (await hookCall(cdp, 'ptyWriteLog', paneId)).map((entry) => entry.data).join('');
      return data.includes(prompt) && data.includes('\r') ? true : null;
    }, 'Ctrl+C recovery browser keyboard bytes', 10_000);
    ctrlSummary.recoveryKeyboardPtyBytes = true;
    await sleep(500);
    await writePty(cdp, workspaceId, paneId, '\r');

    let lastRecovery = null;
    let recovery;
    try {
      recovery = await waitUntil(async () => {
        const rows = await visibleRows(cdp, paneId);
        const cursor = await hookCall(cdp, 'kernelCursor', paneId);
        const processName = await foreground(cdp, workspaceId, paneId).catch(() => null);
        lastRecovery = { processName, tail: rows.slice(-8), cursor };
        if (shellPromptVisible(rows, cursor)) return { status: 'shell', ...lastRecovery };
        return compact(rows).includes(token) ? { status: 'response', ...lastRecovery } : null;
      }, 'Codex model response after Ctrl+C stress', responseTimeoutMs);
    } catch (error) {
      throw new Error(`Codex recovery failed after Ctrl+C stress: ${JSON.stringify({ count: ctrlCCount, intervalMs: ctrlCIntervalMs, lastRecovery, cause: error.message })}`);
    }
    if (recovery.status !== 'response') {
      throw new Error(`Codex exited during Ctrl+C recovery: ${JSON.stringify({ count: ctrlCCount, intervalMs: ctrlCIntervalMs, recovery })}`);
    }
    ctrlSummary.recoveryModelOutputProven = true;
    await sleep(250);
    [ctrlSummary.frame, ctrlSummary.eventLoop] = await Promise.all([
      cdp.evaluate('window.__ridgeCodexFrameProbe.read()'),
      cdp.evaluate('window.__ridgeTermProbe.read()'),
    ]);
    ctrlSummary.elapsedMs = Date.now() - startedAt;
    assertCodexPerformance(ctrlSummary, 'Codex Ctrl+C stress');
    ctrlSummary.completed = true;
    return ctrlSummary;
  } finally {
    ctrlSummary.elapsedMs ||= Date.now() - startedAt;
    if (!ctrlSummary.frame || !ctrlSummary.eventLoop) {
      try {
        [ctrlSummary.frame, ctrlSummary.eventLoop] = await Promise.all([
          cdp.evaluate('window.__ridgeCodexFrameProbe.read()'),
          cdp.evaluate('window.__ridgeTermProbe.read()'),
        ]);
      } catch { /* page closed */ }
    }
    await hookCall(cdp, 'clearPtyWriteLog', paneId).catch(() => {});
    await cdp.evaluate(`(() => {
      window.__ridgeCodexFrameProbe?.stop?.();
      delete window.__ridgeCodexFrameProbe;
    })()`).catch(() => {});
  }
}

async function testCodexSoak(cdp, workspaceId, paneId, soakSummary) {
  const startedAt = Date.now();
  const deadline = soakDurationMs > 0 ? startedAt + soakDurationMs : Infinity;
  const responseTimeoutMs = Math.min(timeoutMs, 60_000);
  await installCodexFrameProbe(cdp);
  try {
    for (let index = 0;
      (soakRounds === 0 || index < soakRounds) && Date.now() < deadline;
      index++) {
      const markerSource = `ridge_soak_done_${index}_${crypto.randomBytes(8).toString('hex')}`;
      const marker = markerSource.toUpperCase();
      const prompt = `Reply with only this token converted to uppercase, with no punctuation: ${markerSource}`;
      await cdp.evaluate('window.__ridgeTermProbe.reset(); window.__ridgeCodexFrameProbe.reset();');
      await hookCall(cdp, 'clearPtyWriteLog', paneId);
      await typeTerminalPrompt(cdp, paneId, prompt);
      await waitUntil(async () => {
        const data = (await hookCall(cdp, 'ptyWriteLog', paneId)).map((entry) => entry.data).join('');
        return data.includes(prompt) && data.includes('\r') ? true : null;
      }, `Codex soak browser keyboard bytes round ${index + 1}`, Math.min(timeoutMs, 10_000));
      await sleep(500);
      await writePty(cdp, workspaceId, paneId, '\r');
      await waitUntil(async () => compact(await visibleRows(cdp, paneId)).includes(marker), `Codex soak response round ${index + 1}`, responseTimeoutMs);
      await sleep(250);

      const [frame, eventLoop, heapResponse, metricsResponse] = await Promise.all([
        cdp.evaluate('window.__ridgeCodexFrameProbe.read()'),
        cdp.evaluate('window.__ridgeTermProbe.read()'),
        cdp.send('Runtime.getHeapUsage'),
        cdp.send('Performance.getMetrics'),
      ]);
      const round = {
        index: index + 1,
        elapsedMs: Date.now() - startedAt,
        keyboardPtyBytes: true,
        frame,
        eventLoop,
        heap: heapResponse.result || null,
        performanceMetrics: metricsResponse.result?.metrics || [],
      };
      soakSummary.rounds.push(round);
      soakSummary.elapsedMs = round.elapsedMs;
      soakSummary.heapTrend = summarizeSoakHeap(soakSummary.rounds);
      assertCodexPerformance(round, `Codex soak round ${index + 1}`);
    }
    if (!soakSummary.rounds.length) throw new Error('Codex soak completed zero rounds; increase soak duration or rounds');
    soakSummary.completed = true;
    soakSummary.elapsedMs = Date.now() - startedAt;
    soakSummary.heapTrend = summarizeSoakHeap(soakSummary.rounds);
    return soakSummary;
  } finally {
    await cdp.evaluate(`(() => {
      window.__ridgeCodexFrameProbe?.stop?.();
      delete window.__ridgeCodexFrameProbe;
    })()`).catch(() => {});
  }
}

const summary = {
  ok: false,
  port,
  requestedDpr: emulatedDpr,
  paneId: null,
  workspaceId: null,
  foregroundProcess: null,
  expectedWebgpuFailure: expectWebgpuFailure,
  expectedWebglFallback: expectWebglFallback,
  webgpuFailure: null,
  webglFallback: null,
  modelOutputProven: false,
  fixtureOutputProven: false,
  commandEchoExcluded: true,
  updatePrompt: { detected: false, handled: false, choice: null },
  backend: null,
  font: null,
  rasterFixture: null,
  nativeGlyphFixture: null,
  gray: null,
  theme: null,
  resize: null,
  workspaceRoundTrip: null,
  stability: null,
  mouse: null,
  burst: null,
  paneWrapIsolation: null,
  performance: null,
  performanceBudgets: {
    globalEventLoopP95MaxMs,
    globalLongTaskMaxMs,
    globalLongTask50msCountMax,
    burstEventLoopP95MaxMs: burstP95MaxMs,
    burstFrameP95MaxMs,
    burstFrameJank50Max: 0,
  },
  soak: soakEnabled ? {
    enabled: true,
    roundsRequested: soakRounds,
    durationMsRequested: soakDurationMs,
    responseTimeoutMs: Math.min(timeoutMs, 60_000),
    completed: false,
    elapsedMs: 0,
    rounds: [],
    heapTrend: null,
  } : null,
  ctrlC: ctrlCEnabled ? {
    enabled: true,
    count: ctrlCCount,
    intervalMs: ctrlCIntervalMs,
    completed: false,
    dispatchElapsedMs: 0,
    elapsedMs: 0,
    pty: null,
    ownerAfterStress: null,
    recoveryToken: null,
    recoveryKeyboardPtyBytes: false,
    recoveryModelOutputProven: false,
    frame: null,
    eventLoop: null,
  } : null,
  runtimeErrors: [],
  screenshots: [],
  lastRows: null,
  lastDecState: null,
  cdpError: null,
  artifactDir,
};

let cdp;
let paneId;
let workspaceId;
let retainedExpected = expected;
let originalDpr = null;
let deviceMetricsOverridden = false;
let gpuOverrideScriptId = null;
try {
  const target = await findTarget();
  cdp = new Cdp(target.webSocketDebuggerUrl);
  await cdp.open();
  await cdp.send('Runtime.enable');
  await cdp.send('Page.enable');
  await cdp.send('Log.enable');
  await cdp.send('Performance.enable');
  if (emulatedDpr !== null) {
    const viewport = await cdp.evaluate('({ width: innerWidth, height: innerHeight, dpr: devicePixelRatio })');
    originalDpr = viewport.dpr;
    await cdp.send('Emulation.setDeviceMetricsOverride', {
      width: Math.max(1, Math.round(viewport.width)),
      height: Math.max(1, Math.round(viewport.height)),
      deviceScaleFactor: emulatedDpr,
      mobile: false,
    });
    deviceMetricsOverridden = true;
    await cdp.send('Page.reload');
    await sleep(500);
  }
  if (expectWebgpuFailure && expectWebglFallback) {
    throw new Error('choose either WebGPU failure or WebGL fallback fixture');
  }
  if (expectWebglFallback) {
    gpuOverrideScriptId = await forceWebgpuUnavailable(cdp);
  }
  if (expectWebgpuFailure) {
    gpuOverrideScriptId = await forceWebgpuUnavailable(cdp, { disableWebgl: true });
    summary.webgpuFailure = await testWebgpuUnavailable(cdp);
    summary.screenshots.push(await capture(cdp, '09-webgpu-unavailable.png'));
    summary.runtimeErrors = summary.webgpuFailure.runtimeExceptions;
    summary.ok = true;
  } else {
  await waitUntil(() => cdp.evaluate(`Boolean(
    window.__ridgeAppReady && window.__TAURI__?.core?.invoke && window.__windE2E
    && ${emulatedDpr === null ? 'true' : `Math.abs(devicePixelRatio - ${emulatedDpr}) < 0.001`}
  )`), 'Ridge DEV hooks', 180_000);
  await installPerformanceProbe(cdp);
  await cdp.evaluate('window.__ridgeTermProbe.reset()');

  workspaceId = await invoke(cdp, 'get_window_active_workspace_id');
  const initial = await waitUntil(() => paneProbe(cdp), 'visible terminal pane', 60_000);
  paneId = initial.paneId;
  summary.workspaceId = workspaceId;
  summary.paneId = paneId;
  summary.backend = await hookCall(cdp, 'backendName', paneId);
  if (expectWebglFallback) {
    if (summary.backend?.toLowerCase() !== 'webgl2') {
      throw new Error(`expected WebGL2 fallback, got ${summary.backend ?? 'no backend'}`);
    }
    summary.webglFallback = await cdp.evaluate(`({
      navigatorGpuAvailable: Boolean(navigator.gpu),
      contextCalls: window.__ridgeGpuContextCalls ?? [],
      visibleFailure: [...document.querySelectorAll('[role="alert"]')]
        .some((element) => element.textContent?.includes('WEBGPU_INIT_FAILED')),
    })`);
    if (summary.webglFallback.navigatorGpuAvailable || summary.webglFallback.visibleFailure) {
      throw new Error(`WebGL2 fallback fixture violated contract: ${JSON.stringify(summary.webglFallback)}`);
    }
  }
  summary.font = await cdp.evaluate(`({
    configuredFamily: getComputedStyle(document.documentElement).getPropertyValue('--rg-term-font-family').trim(),
    configuredSize: getComputedStyle(document.documentElement).getPropertyValue('--rg-term-font-size').trim(),
    dpr: window.devicePixelRatio,
  })`);
  await waitUntil(async () => {
    await writePty(cdp, workspaceId, paneId, '');
    return true;
  }, 'native PTY activation', 60_000);
  await hookCall(cdp, 'installPtyWriteSpy', paneId);
  await exitCodex(cdp, workspaceId, paneId, summary.updatePrompt);
  await writePty(cdp, workspaceId, paneId, 'Clear-Host\r');
  await sleep(500);
  summary.screenshots.push(await capture(cdp, '01-shell.png'));

  if (fixtureOnly) {
    await writePty(cdp, workspaceId, paneId, `Write-Output ('${challenge}'.ToUpperInvariant())\r`);
    await waitUntil(async () => compact(await visibleRows(cdp, paneId)).includes(expected), 'fixture PTY output', 10_000);
    summary.fixtureOutputProven = true;
    summary.screenshots.push(await capture(cdp, '03-codex-output.png'));
  } else {
    await writePty(cdp, workspaceId, paneId, `${codexLaunch}\r`);
    const startupState = await waitUntil(async () => {
      const rows = await visibleRows(cdp, paneId);
      const state = codexOwnerState(rows, await hookCall(cdp, 'kernelCursor', paneId));
      return state === 'update-prompt' || state === 'codex' ? state : null;
    }, 'Codex TUI or update prompt first frame', 45_000);
    if (startupState === 'update-prompt') {
      await skipCodexUpdatePrompt(cdp, workspaceId, paneId, summary.updatePrompt);
    }
    summary.foregroundProcess = await foreground(cdp, workspaceId, paneId).catch(() => null);
    let sawMcpStartup = false;
    const readinessStartedAt = Date.now();
    await waitUntil(async () => {
      const rows = await visibleRows(cdp, paneId);
      if (shellPromptVisible(rows, await hookCall(cdp, 'kernelCursor', paneId))) throw new Error('Codex returned to PowerShell during MCP startup');
      const segment = latestCodexSegment(rows);
      const starting = segment.some((line) => /Starting MCP servers/i.test(line));
      if (starting) sawMcpStartup = true;
      return !starting && (sawMcpStartup || Date.now() - readinessStartedAt >= 2_000) ? true : null;
    }, 'Codex input readiness', 60_000);

    const prompt = `Reply with only this token converted to uppercase, with no punctuation: ${challenge}`;
    if (prompt.includes(expected)) throw new Error('model marker leaked into echoed prompt');
    await hookCall(cdp, 'clearPtyWriteLog', paneId);
    await typeTerminalPrompt(cdp, paneId, prompt);
    await waitUntil(async () => {
      const data = (await hookCall(cdp, 'ptyWriteLog', paneId)).map((entry) => entry.data).join('');
      return data.includes(prompt) && data.includes('\r') ? true : null;
    }, 'browser keyboard PTY bytes', 10_000);
    await sleep(500);
    await writePty(cdp, workspaceId, paneId, '\r');
    await sleep(2000);
    summary.screenshots.push(await capture(cdp, '02-prompt-submitted.png'));
    await waitUntil(async () => compact(await visibleRows(cdp, paneId)).includes(expected), 'non-echo Codex model output');
    summary.modelOutputProven = true;
    summary.screenshots.push(await capture(cdp, '03-codex-output.png'));
  }

  if (soakEnabled) {
    if (fixtureOnly) throw new Error('Codex soak requires real Codex TUI; unset RIDGE_TERM_E2E_FIXTURE_ONLY');
    await testCodexSoak(cdp, workspaceId, paneId, summary.soak);
  }

  if (ctrlCEnabled) {
    if (fixtureOnly) throw new Error('Codex Ctrl+C stress requires real Codex TUI; unset RIDGE_TERM_E2E_FIXTURE_ONLY');
    await testCodexCtrlC(cdp, workspaceId, paneId, summary.ctrlC);
    retainedExpected = summary.ctrlC.recoveryToken;
  }

  summary.theme = await rotateTheme(cdp, paneId);
  if (!compact(await visibleRows(cdp, paneId)).includes(retainedExpected)) throw new Error('Codex output lost after theme rotation');
  summary.resize = await resizePane(cdp, paneId);
  if (!compact(await visibleRows(cdp, paneId)).includes(retainedExpected)) throw new Error('Codex output lost after pane resize');
  summary.workspaceRoundTrip = await switchWorkspaceRoundTrip(cdp, workspaceId, paneId);
  if (!compact(await visibleRows(cdp, paneId)).includes(retainedExpected)) throw new Error('Codex output lost after workspace round trip');

  let settleSignature = '';
  let consecutiveStableSamples = 0;
  await waitUntil(async () => {
    const rows = await visibleRows(cdp, paneId);
    const cursor = await hookCall(cdp, 'kernelCursor', paneId);
    const signature = `${hashRows(rows)}:${JSON.stringify(cursor)}`;
    consecutiveStableSamples = signature === settleSignature ? consecutiveStableSamples + 1 : 1;
    settleSignature = signature;
    return consecutiveStableSamples >= 4 ? true : null;
  }, 'post-transition terminal settle', 5_000);

  // Post-settle kernel cursor, not the frozen presentation cursor.
  // Freeze-during-rewind is the renderer contract (`--cursor-freeze-unit`).
  const hashes = [];
  const cursors = [];
  for (let i = 0; i < 20; i++) {
    const rows = await visibleRows(cdp, paneId);
    if (!compact(rows).includes(retainedExpected)) throw new Error(`Codex output absent during stability sample ${i}`);
    hashes.push(hashRows(rows));
    cursors.push(await hookCall(cdp, 'kernelCursor', paneId));
    await sleep(50);
  }
  const caret = await cdp.evaluate(`(() => {
    const element = document.querySelector(${JSON.stringify(`.rg-pane-container[data-rg-pane-id="${paneId}"] .rg-ime-helper`)});
    return element ? getComputedStyle(element).caretColor : null;
  })()`);
  const uniqueHashes = new Set(hashes).size;
  const uniqueCursors = new Set(cursors.map((cursor) => JSON.stringify(cursor))).size;
  summary.stability = { uniqueHashes, uniqueCursors, caret, dec: await hookCall(cdp, 'kernelDecState', paneId) };
  if (uniqueHashes > 2 || uniqueCursors > 2) throw new Error(`settled TUI unstable: ${JSON.stringify(summary.stability)}`);
  if (caret && caret !== 'transparent' && !/rgba\([^)]*,\s*0\)$/.test(caret)) throw new Error(`native textarea caret is visible: ${caret}`);
  summary.screenshots.push(await capture(cdp, '04-recovered.png'));

  if (fixtureOnly) {
    const rows = await visibleRows(cdp, paneId);
    if (!shellPromptVisible(rows, await hookCall(cdp, 'kernelCursor', paneId))) {
      throw new Error('fixture-only run requires an already visible PowerShell prompt');
    }
  } else {
    await exitCodex(cdp, workspaceId, paneId);
  }
  summary.rasterFixture = await testRoundedBoxDecstbmFixture(cdp, workspaceId, paneId);
  const screenshotRows = await visibleRows(cdp, paneId);
  const screenshotText = screenshotRows.join('\n');
  const screenshotTrimmed = screenshotRows.map((line) => line.trim());
  const screenshotFrameRows = {
    top: screenshotTrimmed.findIndex((line) => /^╭──────────────╮$/.test(line)),
    middle: screenshotTrimmed.findIndex((line) => /^│ RIDGE FONT │$/.test(line)),
    bottom: screenshotTrimmed.findIndex((line) => /^╰──────────────╯$/.test(line)),
  };
  if (screenshotFrameRows.top < 11 || screenshotFrameRows.middle !== screenshotFrameRows.top + 1
    || screenshotFrameRows.bottom !== screenshotFrameRows.top + 2
    || !['╭', '╮', '╯', '╰'].every((glyph) => screenshotText.includes(glyph))) {
    throw new Error(`fixture screenshot rows lack the final rounded box: ${JSON.stringify({ screenshotFrameRows })}`);
  }
  summary.rasterFixture.screenshotVisible = true;
  summary.rasterFixture.screenshotFrameRows = screenshotFrameRows;
  summary.screenshots.push(await capture(cdp, '07-box-decstbm.png'));
  const rasterGeometry = await paneProbe(cdp, paneId);
  if (rasterGeometry) {
    const gridLeft = rasterGeometry.rect.left + rasterGeometry.anchor.x
      - rasterGeometry.anchor.col * rasterGeometry.anchor.cellW;
    const gridTop = rasterGeometry.rect.top + rasterGeometry.anchor.y
      - rasterGeometry.anchor.row * rasterGeometry.anchor.cellH;
    summary.screenshots.push(await capture(cdp, '07-box-decstbm-join-4x.png', {
      x: gridLeft,
      y: gridTop + screenshotFrameRows.top * rasterGeometry.anchor.cellH,
      width: Math.min(rasterGeometry.rect.width, 20 * rasterGeometry.anchor.cellW),
      height: 3 * rasterGeometry.anchor.cellH,
      scale: 4,
    }));
  }
  summary.nativeGlyphFixture = await testNativeGlyphFixture(cdp, workspaceId, paneId);
  const nativeGlyphScreenshot = await capture(cdp, '11-native-cjk-emoji.png');
  summary.screenshots.push(nativeGlyphScreenshot);
  const nativeGlyphGeometry = await paneProbe(cdp, paneId);
  if (nativeGlyphGeometry) {
    const gridLeft = nativeGlyphGeometry.rect.left + nativeGlyphGeometry.anchor.x
      - nativeGlyphGeometry.anchor.col * nativeGlyphGeometry.anchor.cellW;
    const gridTop = nativeGlyphGeometry.rect.top + nativeGlyphGeometry.anchor.y
      - nativeGlyphGeometry.anchor.row * nativeGlyphGeometry.anchor.cellH;
    summary.screenshots.push(await capture(cdp, '11-native-cjk-emoji-4x.png', {
      x: gridLeft,
      y: gridTop,
      width: Math.min(nativeGlyphGeometry.rect.width, 42 * nativeGlyphGeometry.anchor.cellW),
      height: 5 * nativeGlyphGeometry.anchor.cellH,
      scale: 4,
    }));
    const dpr = await cdp.evaluate('devicePixelRatio');
    if (Math.abs(dpr - 1) < 0.01) {
      const sharpness = await measureMonochromeSharpness(nativeGlyphScreenshot, {
        x: gridLeft,
        y: gridTop,
        width: Math.min(nativeGlyphGeometry.rect.width, 42 * nativeGlyphGeometry.anchor.cellW),
        height: 2 * nativeGlyphGeometry.anchor.cellH,
      });
      summary.nativeGlyphFixture.sharpness = { dpr, ...sharpness };
      if (sharpness.solidRatio < 0.7 || sharpness.transitionRatio > 0.3) {
        throw new Error(`monochrome glyph sharpness regressed (solid >= 0.7, transition <= 0.3): ${JSON.stringify(sharpness)}`);
      }
    }
  }
  // Exclude Codex/network startup noise; global gate covers settled terminal workload.
  await cdp.evaluate('window.__ridgeTermProbe.reset()');
  summary.gray = await testIndexedGrayForeground(cdp, workspaceId, paneId);
  if (summary.gray.indexedCellCount < 8 || !summary.gray.text.includes('RIDGE_INDEXED_GRAY')) {
    throw new Error(`indexed gray foreground was not preserved: ${JSON.stringify(summary.gray)}`);
  }
  summary.screenshots.push(await capture(cdp, '06-indexed-gray.png'));
  summary.paneWrapIsolation = await testPaneLastRowWrapIsolation(cdp, workspaceId, paneId);
  summary.screenshots.push(summary.paneWrapIsolation.screenshot);
  summary.mouse = await testMouse(cdp, workspaceId, paneId);
  summary.screenshots.push(await capture(cdp, '05-mouse-selection.png'));

  if (burstLines > 0) {
    summary.burst = await testOutputBurst(cdp, workspaceId, paneId, burstLines, burstMode, burstIntervalMs);
    if (summary.burst.screenshot) summary.screenshots.push(summary.burst.screenshot);
    if (summary.burst.delta.count === 0 || summary.burst.text.count !== 0) {
      throw new Error(`desktop burst used an unexpected transport: ${JSON.stringify(summary.burst)}`);
    }
    if (summary.burst.mode === 'tui' && summary.burst.scrollbackAfter !== summary.burst.scrollbackBefore) {
      throw new Error(`in-place TUI burst entered scrollback: ${JSON.stringify(summary.burst)}`);
    }
    if (summary.burst.eventLoop.p95 > burstP95MaxMs) {
      throw new Error(`native burst event-loop budget exceeded: ${JSON.stringify({
        threshold: { p95MaxMs: burstP95MaxMs },
        metrics: summary.burst,
      })}`);
    }
    if (summary.burst.frame.p95 > burstFrameP95MaxMs || summary.burst.frame.jank50 > 0) {
      throw new Error(`native burst rAF budget exceeded: ${JSON.stringify({
        threshold: { p95MaxMs: burstFrameP95MaxMs, jank50Max: 0 },
        metrics: summary.burst,
      })}`);
    }
  }

  summary.performance = await cdp.evaluate('window.__ridgeTermProbe.read()');
  const runtimeExceptions = cdp.events
    .filter((event) => event.method === 'Runtime.exceptionThrown')
    .map((event) => event.params?.exceptionDetails?.exception?.description || event.params?.exceptionDetails?.text || 'unknown runtime exception');
  const dangerousLogs = cdp.events
    .filter((event) => event.method === 'Log.entryAdded' || event.method === 'Runtime.consoleAPICalled')
    .map((event) => event.method === 'Log.entryAdded'
      ? String(event.params?.entry?.text || '')
      : (event.params?.args || []).map((arg) => String(arg.value ?? arg.description ?? '')).join(' '))
    .filter((text) => /GPUValidation|WebGPU.*(?:error|lost)|device.*lost|uncaught (?:type|range)?error/i.test(text));
  summary.runtimeErrors = [...runtimeExceptions, ...dangerousLogs];
  if (summary.performance.p95 > globalEventLoopP95MaxMs
    || summary.performance.longtaskMax > globalLongTaskMaxMs
    || summary.performance.longtask50msCount > globalLongTask50msCountMax) {
    throw new Error(`global performance budget exceeded: ${JSON.stringify({
      thresholds: {
        eventLoopP95MaxMs: globalEventLoopP95MaxMs,
        longTaskMaxMs: globalLongTaskMaxMs,
        longTask50msCountMax: globalLongTask50msCountMax,
      },
      metrics: summary.performance,
    })}`);
  }
  if (summary.runtimeErrors.length) throw new Error(`runtime errors: ${summary.runtimeErrors.join(' | ')}`);
  summary.ok = true;
  }
} catch (error) {
  summary.error = error.stack || error.message || String(error);
  process.exitCode = 1;
  if (cdp && paneId && workspaceId) {
    try {
      const rows = await visibleRows(cdp, paneId);
      summary.lastRows = rows;
      summary.lastDecState = await hookCall(cdp, 'kernelDecState', paneId);
      summary.screenshots.push(await capture(cdp, '99-failure.png'));
      if (codexVisible(rows) && !shellPromptVisible(rows, await hookCall(cdp, 'kernelCursor', paneId))) await exitCodex(cdp, workspaceId, paneId);
    } catch { /* the dev page may already be gone */ }
  }
} finally {
  if (cdp) {
    try { await cdp.evaluate('window.__ridgeTermProbe?.stop?.()'); } catch { /* page closed */ }
    if (deviceMetricsOverridden) {
      try {
        await cdp.send('Emulation.clearDeviceMetricsOverride');
        await waitUntil(
          () => cdp.evaluate(`Math.abs(devicePixelRatio - ${originalDpr ?? 1}) < 0.001`),
          'restore original DPR',
          5_000,
        );
      } catch { /* connection may already be gone; CDP also clears override on detach */ }
    }
    if (gpuOverrideScriptId) {
      try {
        await cdp.send('Page.removeScriptToEvaluateOnNewDocument', { identifier: gpuOverrideScriptId });
        await cdp.send('Page.reload', { ignoreCache: true });
        await sleep(500);
      } catch { /* target may already be gone */ }
    }
    summary.cdpError = cdp.connectionError?.message || null;
    cdp.close();
  }
  fs.writeFileSync(path.join(artifactDir, 'summary.json'), JSON.stringify(summary, null, 2), 'utf8');
  console.log(JSON.stringify(summary, null, 2));
}
