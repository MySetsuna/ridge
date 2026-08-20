#!/usr/bin/env node
// Real WebView2 acceptance probe for Codex TUI rendering and terminal input.
// Uses Ridge's DEV-only PTY hooks, so every byte still crosses the native PTY
// and returns through the production parser/renderer path.

import crypto from 'node:crypto';
import fs from 'node:fs';
import http from 'node:http';
import path from 'node:path';
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
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const timeoutMs = Number.parseInt(process.env.RIDGE_TERM_E2E_TIMEOUT_MS || '240000', 10);
const artifactDir = path.resolve(process.env.RIDGE_TERM_E2E_ARTIFACT_DIR || '.iteration/artifacts/term-render');
const fixtureOnly = process.env.RIDGE_TERM_E2E_FIXTURE_ONLY === '1';
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
  }

  async open() {
    await new Promise((resolve, reject) => {
      this.ws.onopen = resolve;
      this.ws.onerror = (event) => reject(new Error(`CDP WebSocket error: ${event.message || event.type}`));
      this.ws.onmessage = (event) => {
        const message = JSON.parse(event.data);
        const pending = this.pending.get(message.id);
        if (pending) {
          this.pending.delete(message.id);
          if (message.error) pending.reject(new Error(`${pending.method}: ${JSON.stringify(message.error)}`));
          else pending.resolve(message);
        } else if (message.method) {
          this.events.push(message);
        }
      };
    });
  }

  send(method, params = {}) {
    const id = ++this.nextId;
    return new Promise((resolve, reject) => {
      this.pending.set(id, { method, resolve, reject });
      this.ws.send(JSON.stringify({ id, method, params }));
    });
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
      lastError = error;
    }
    await sleep(200);
  }
  try {
    last = await probe();
    if (last) return last;
  } catch (error) {
    lastError = error;
  }
  throw new Error(`${description} timed out after ${maxMs}ms; last=${JSON.stringify(last)}${lastError ? `; error=${lastError.message}` : ''}`);
}

const invoke = (cdp, command, args = {}) => cdp.evaluate(
  `window.__TAURI__.core.invoke(${JSON.stringify(command)}, ${JSON.stringify(args)})`,
);

const compact = (rows) => (rows ?? []).join('').replace(/\s+/g, '');
const hashRows = (rows) => crypto.createHash('sha256').update((rows ?? []).join('\n')).digest('hex');

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

async function foreground(cdp, workspaceId, paneId) {
  return invoke(cdp, 'get_pane_foreground_process', { workspaceId, paneId });
}

function codexVisible(rows) {
  return (rows ?? []).some((line) => /(?:OpenAI Codex|RidgeCode)/i.test(line));
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

async function capture(cdp, name) {
  const response = await cdp.send('Page.captureScreenshot', { format: 'png' });
  if (!response.result?.data) throw new Error(`no screenshot data for ${name}`);
  const target = path.join(artifactDir, name);
  fs.writeFileSync(target, Buffer.from(response.result.data, 'base64'));
  return target;
}

async function dispatchClick(cdp, probe) {
  const x = Math.round(probe.rect.left + probe.rect.width * 0.5);
  const y = Math.round(probe.rect.top + probe.rect.height * 0.5);
  await cdp.send('Input.dispatchMouseEvent', { type: 'mousePressed', x, y, button: 'left', buttons: 1, clickCount: 1 });
  await cdp.send('Input.dispatchMouseEvent', { type: 'mouseReleased', x, y, button: 'left', buttons: 0, clickCount: 1 });
}

async function typeTerminalPrompt(cdp, paneId, text) {
  const focused = await cdp.evaluate(`(() => {
    const pane = document.querySelector(${JSON.stringify(`.rg-pane-container[data-rg-pane-id="${paneId}"]`)});
    const target = pane?.querySelector('.rg-ime-helper') || pane;
    if (!target) return false;
    target.focus();
    return document.activeElement === target;
  })()`);
  if (!focused) throw new Error('terminal input target could not be focused');
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
    const tick = 50;
    let last = performance.now();
    state.interval = setInterval(() => {
      const now = performance.now();
      state.samples.push(Math.max(0, now - last - tick));
      last = now;
    }, tick);
    try {
      state.observer = new PerformanceObserver((list) => {
        for (const entry of list.getEntries()) state.longtasks.push(Math.round(entry.duration));
      });
      state.observer.observe({ entryTypes: ['longtask'] });
    } catch {}
    window.__ridgeTermProbe = {
      reset() { state.samples.length = 0; state.longtasks.length = 0; },
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

async function exitCodex(cdp, workspaceId, paneId) {
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
    if (shellPromptVisible(rows, cursor)) return 'shell';
    return codexVisible(rows) ? 'codex' : null;
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
  const owner = await waitForTerminalOwner();
  if (owner === 'shell') return probeShell();
  if (owner !== 'codex') {
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
  await hookCall(cdp, 'feedPty', paneId, '\x1b[?1000h\x1b[?1006h');
  await waitUntil(async () => {
    const state = await hookCall(cdp, 'kernelDecState', paneId);
    return state?.mouseReportingModes ? state : null;
  }, 'direct WASM SGR mouse mode', 1_000);
  await hookCall(cdp, 'clearPtyWriteLog', paneId);
  await dispatchClick(cdp, await paneProbe(cdp, paneId));
  const writes = await waitUntil(async () => {
    const entries = await hookCall(cdp, 'ptyWriteLog', paneId);
    const data = entries.map((entry) => entry.data).join('');
    return /\x1b\[<0;\d+;\d+[Mm]/.test(data) ? entries : null;
  }, 'mouse forwarding bytes', 10_000);
  await hookCall(cdp, 'feedPty', paneId, '\x1b[?1000l\x1b[?1006l');
  await waitUntil(async () => {
    const state = await hookCall(cdp, 'kernelDecState', paneId);
    return state?.mouseReportingModes === 0 ? state : null;
  }, 'direct WASM mouse mode reset', 1_000);

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
    const tick = (now) => {
      state.frames.push(now - state.last);
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
    const command = mode === 'tui'
      ? `$e=[char]27; [Console]::Out.Write($e+'[H'+$e+'[2K'+('${readySource}'.ToUpperInvariant())); Start-Sleep -Milliseconds 300; 1..${lineCount} | ForEach-Object { [Console]::Out.Write($e+'[H'+$e+'[2K'+'RIDGE_TUI_FRAME_'+$_)${delay} }; [Console]::Out.Write($e+'[H'+$e+'[2K'+('${markerSource}'.ToUpperInvariant()))\r`
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
    await sleep(250);
    return cdp.evaluate(`(() => {
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

const summary = {
  ok: false,
  port,
  paneId: null,
  workspaceId: null,
  foregroundProcess: null,
  modelOutputProven: false,
  fixtureOutputProven: false,
  commandEchoExcluded: true,
  backend: null,
  gray: null,
  theme: null,
  resize: null,
  workspaceRoundTrip: null,
  stability: null,
  mouse: null,
  burst: null,
  performance: null,
  runtimeErrors: [],
  screenshots: [],
  lastRows: null,
  lastDecState: null,
  artifactDir,
};

let cdp;
let paneId;
let workspaceId;
try {
  const target = await findTarget();
  cdp = new Cdp(target.webSocketDebuggerUrl);
  await cdp.open();
  await cdp.send('Runtime.enable');
  await cdp.send('Page.enable');
  await cdp.send('Log.enable');
  await waitUntil(() => cdp.evaluate('Boolean(window.__ridgeAppReady && window.__TAURI__?.core?.invoke && window.__windE2E)'), 'Ridge DEV hooks', 180_000);
  await installPerformanceProbe(cdp);
  await cdp.evaluate('window.__ridgeTermProbe.reset()');

  workspaceId = await invoke(cdp, 'get_window_active_workspace_id');
  const initial = await waitUntil(() => paneProbe(cdp), 'visible terminal pane', 60_000);
  paneId = initial.paneId;
  summary.workspaceId = workspaceId;
  summary.paneId = paneId;
  summary.backend = await hookCall(cdp, 'backendName', paneId);
  await waitUntil(async () => {
    await writePty(cdp, workspaceId, paneId, '');
    return true;
  }, 'native PTY activation', 60_000);
  await hookCall(cdp, 'installPtyWriteSpy', paneId);
  await exitCodex(cdp, workspaceId, paneId);
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
    await waitUntil(async () => {
      const rows = await visibleRows(cdp, paneId);
      return codexVisible(rows) && !shellPromptVisible(rows, await hookCall(cdp, 'kernelCursor', paneId)) ? true : null;
    }, 'Codex TUI first frame', 45_000);
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

  summary.theme = await rotateTheme(cdp, paneId);
  if (!compact(await visibleRows(cdp, paneId)).includes(expected)) throw new Error('Codex output lost after theme rotation');
  summary.resize = await resizePane(cdp, paneId);
  if (!compact(await visibleRows(cdp, paneId)).includes(expected)) throw new Error('Codex output lost after pane resize');
  summary.workspaceRoundTrip = await switchWorkspaceRoundTrip(cdp, workspaceId, paneId);
  if (!compact(await visibleRows(cdp, paneId)).includes(expected)) throw new Error('Codex output lost after workspace round trip');

  const hashes = [];
  const cursors = [];
  for (let i = 0; i < 20; i++) {
    const rows = await visibleRows(cdp, paneId);
    if (!compact(rows).includes(expected)) throw new Error(`Codex output absent during stability sample ${i}`);
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
  summary.gray = await testIndexedGrayForeground(cdp, workspaceId, paneId);
  if (summary.gray.indexedCellCount < 8 || !summary.gray.text.includes('RIDGE_INDEXED_GRAY')) {
    throw new Error(`indexed gray foreground was not preserved: ${JSON.stringify(summary.gray)}`);
  }
  summary.screenshots.push(await capture(cdp, '06-indexed-gray.png'));
  summary.mouse = await testMouse(cdp, workspaceId, paneId);
  summary.screenshots.push(await capture(cdp, '05-mouse-selection.png'));

  if (burstLines > 0) {
    summary.burst = await testOutputBurst(cdp, workspaceId, paneId, burstLines, burstMode, burstIntervalMs);
    if (summary.burst.delta.count === 0 || summary.burst.text.count !== 0) {
      throw new Error(`desktop burst used an unexpected transport: ${JSON.stringify(summary.burst)}`);
    }
    if (summary.burst.mode === 'tui' && summary.burst.scrollbackAfter !== summary.burst.scrollbackBefore) {
      throw new Error(`in-place TUI burst entered scrollback: ${JSON.stringify(summary.burst)}`);
    }
    if (summary.burst.eventLoop.p95 > burstP95MaxMs) {
      throw new Error(`native burst event-loop p95 ${summary.burst.eventLoop.p95}ms exceeds ${burstP95MaxMs}ms`);
    }
    if (summary.burst.frame.p95 > burstFrameP95MaxMs || summary.burst.frame.jank50 > 0) {
      throw new Error(`native burst rAF budget exceeded: ${JSON.stringify(summary.burst.frame)}`);
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
  if (summary.performance.p95 > 100) throw new Error(`event-loop p95 ${summary.performance.p95}ms exceeds 100ms`);
  if (summary.performance.longtaskMax > 2000) throw new Error(`long task ${summary.performance.longtaskMax}ms exceeds 2000ms`);
  if (summary.runtimeErrors.length) throw new Error(`runtime errors: ${summary.runtimeErrors.join(' | ')}`);
  summary.ok = true;
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
    cdp.close();
  }
  fs.writeFileSync(path.join(artifactDir, 'summary.json'), JSON.stringify(summary, null, 2), 'utf8');
  console.log(JSON.stringify(summary, null, 2));
}
