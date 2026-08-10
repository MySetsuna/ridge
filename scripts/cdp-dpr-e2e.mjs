#!/usr/bin/env node
// Real WebView2 DPR acceptance probe for the terminal raster-density fix.
// Captures runtime DPR, CSS/backing canvas sizes, and a screenshot artifact.

import fs from 'node:fs';
import http from 'node:http';
import path from 'node:path';
import { resolveCdpPort } from './cdp-port.mjs';
import { isRidgeCdpTarget } from './lib/cdpTarget.mjs';

const port = resolveCdpPort();
const artifact = process.env.RIDGE_DPR_ARTIFACT || '.iteration/artifacts/dpr/desktop-shot.png';
const appReadyTimeoutMs = Number.parseInt(process.env.RIDGE_DPR_APP_READY_TIMEOUT_MS || '60000', 10);
const rendererTimeoutMs = Number.parseInt(process.env.RIDGE_DPR_RENDERER_TIMEOUT_MS || '60000', 10);
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

function listTargets() {
  return new Promise((resolve, reject) => {
    http.get({ host: '127.0.0.1', port, path: '/json/list', timeout: 3000 }, (response) => {
      let body = '';
      response.on('data', (chunk) => (body += chunk));
      response.on('end', () => {
        try { resolve(JSON.parse(body)); } catch (error) { reject(error); }
      });
    }).on('error', reject);
  });
}

async function findTarget() {
  const deadline = Date.now() + 30_000;
  while (Date.now() < deadline) {
    try {
      const target = (await listTargets()).find(isRidgeCdpTarget);
      if (target) return target;
    } catch { /* dev:cdp may still be starting */ }
    await sleep(500);
  }
  throw new Error(`no Ridge CDP page on 127.0.0.1:${port}`);
}

class Cdp {
  constructor(url) {
    this.ws = new WebSocket(url);
    this.nextId = 0;
    this.pending = new Map();
  }

  async open() {
    await new Promise((resolve, reject) => {
      this.ws.onopen = resolve;
      this.ws.onerror = (event) => reject(new Error(`CDP WebSocket error: ${event.message || event.type}`));
      this.ws.onmessage = (event) => {
        const message = JSON.parse(event.data);
        const resolvePending = this.pending.get(message.id);
        if (!resolvePending) return;
        this.pending.delete(message.id);
        resolvePending(message);
      };
    });
  }

  send(method, params = {}) {
    const id = ++this.nextId;
    return new Promise((resolve) => {
      this.pending.set(id, resolve);
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
      throw new Error(`CDP evaluation failed: ${JSON.stringify(response.result.exceptionDetails)}`);
    }
    return response.result?.result?.value;
  }

  close() {
    try { this.ws.close(); } catch { /* already closed */ }
  }
}

async function waitForRenderer(cdp) {
  const appReadyDeadline = Date.now() + appReadyTimeoutMs;
  let last = null;
  while (Date.now() < appReadyDeadline) {
    last = await cdp.evaluate(`({
      appReady: Boolean(window.__ridgeAppReady),
      readyState: document.readyState,
      dpr: window.devicePixelRatio,
      innerWidth: window.innerWidth,
      innerHeight: window.innerHeight,
      canvases: [...document.querySelectorAll('canvas')].map((canvas) => ({
        cssWidth: canvas.getBoundingClientRect().width,
        cssHeight: canvas.getBoundingClientRect().height,
        width: canvas.width,
        height: canvas.height,
      })),
    })`);
    if (last?.appReady) break;
    await sleep(250);
  }
  if (!last?.appReady) {
    throw new Error(`app did not signal ridge:app-ready within ${appReadyTimeoutMs}ms: ${JSON.stringify(last)}`);
  }

  const rendererDeadline = Date.now() + rendererTimeoutMs;
  while (Date.now() < rendererDeadline) {
    last = await cdp.evaluate(`({
      appReady: Boolean(window.__ridgeAppReady),
      readyState: document.readyState,
      dpr: window.devicePixelRatio,
      innerWidth: window.innerWidth,
      innerHeight: window.innerHeight,
      canvases: [...document.querySelectorAll('canvas')].map((canvas) => ({
        cssWidth: canvas.getBoundingClientRect().width,
        cssHeight: canvas.getBoundingClientRect().height,
        width: canvas.width,
        height: canvas.height,
      })),
    })`);
    const backing = last?.canvases?.filter((canvas) => canvas.width > 0 && canvas.height > 0).length ?? 0;
    if (last?.canvases?.length > 0 && backing > 0) return last;
    await sleep(250);
  }
  throw new Error(`renderer did not mount a backing canvas within ${rendererTimeoutMs}ms after app-ready: ${JSON.stringify(last)}`);
}

const target = await findTarget();
const cdp = new Cdp(target.webSocketDebuggerUrl);
await cdp.open();
await cdp.send('Runtime.enable');
await cdp.send('Page.enable');
const runtime = await waitForRenderer(cdp);
const screenshot = await cdp.send('Page.captureScreenshot', { format: 'png' });
cdp.close();

if (!runtime || !Number.isFinite(runtime.dpr) || runtime.dpr <= 0) {
  throw new Error(`invalid WebView DPR: ${JSON.stringify(runtime)}`);
}
if (!screenshot.result?.data) throw new Error('CDP returned no screenshot data');

const artifactPath = path.resolve(artifact);
fs.mkdirSync(path.dirname(artifactPath), { recursive: true });
fs.writeFileSync(artifactPath, Buffer.from(screenshot.result.data, 'base64'));

const backingCanvasCount = runtime.canvases.filter((canvas) => canvas.width > 0 && canvas.height > 0).length;
if (runtime.canvases.length === 0) {
  throw new Error('DPR probe found no terminal canvas; page may not have mounted the renderer');
}
if (backingCanvasCount !== runtime.canvases.length) {
  throw new Error(
    `DPR probe found ${runtime.canvases.length - backingCanvasCount} canvas element(s) without a backing buffer`,
  );
}
console.log(JSON.stringify({ ok: true, dpr: runtime.dpr, innerWidth: runtime.innerWidth, innerHeight: runtime.innerHeight, canvasCount: runtime.canvases.length, backingCanvasCount, artifact: artifactPath }));
