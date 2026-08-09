#!/usr/bin/env node
// Full local Cloud/Postgres E2E runner.
//
// Required environment:
//   RIDGE_CDP_PORT, RIDGE_CLOUD_USER_TOKEN, RIDGE_CLOUD_DEVICE_TOKEN,
//   RIDGE_CLOUD_USERNAME, RIDGE_CLOUD_DEVICE
// Optional: RIDGE_CLOUD_E2E_PATH, RIDGE_CLOUD_PANE_ID, RIDGE_CLOUD_PANE_WRITE.
// Tokens stay in process memory and are never printed or written to evidence.

import http from 'node:http';

const port = Number(process.env.RIDGE_CDP_PORT || 0);
const required = {
  userToken: process.env.RIDGE_CLOUD_USER_TOKEN,
  deviceToken: process.env.RIDGE_CLOUD_DEVICE_TOKEN,
  username: process.env.RIDGE_CLOUD_USERNAME,
  device: process.env.RIDGE_CLOUD_DEVICE,
};
const missing = Object.entries(required).filter(([, value]) => !value).map(([key]) => key);
if (!port || missing.length > 0) {
  throw new Error(`missing RIDGE_CDP_PORT or cloud credentials: ${missing.join(', ') || 'RIDGE_CDP_PORT'}`);
}

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

function httpJson(path) {
  return new Promise((resolve, reject) => {
    http.get({ host: '127.0.0.1', port, path, timeout: 3000 }, (response) => {
      let body = '';
      response.on('data', (chunk) => (body += chunk));
      response.on('end', () => {
        try {
          resolve(JSON.parse(body));
        } catch (error) {
          reject(error);
        }
      });
    }).on('error', reject);
  });
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

async function findRidgeTarget() {
  const deadline = Date.now() + 90_000;
  while (Date.now() < deadline) {
    try {
      const targets = await httpJson('/json/list');
      const target = targets.find((item) => item.type === 'page' && item.webSocketDebuggerUrl);
      if (target) return target;
    } catch { /* CDP starts after the Tauri window */ }
    await sleep(1000);
  }
  throw new Error(`no Ridge CDP target on 127.0.0.1:${port}`);
}

function redact(value) {
  return String(value)
    .replace(/(token|password|secret|authorization)\s*[:=]\s*[^,\s}]+/gi, '$1=<redacted>')
    .replace(/(Bearer\s+)[^\s]+/gi, '$1<redacted>');
}

const target = await findRidgeTarget();
const cdp = new Cdp(target.webSocketDebuggerUrl);
await cdp.open();
await cdp.send('Runtime.enable');

const options = {
  deviceToken: required.deviceToken,
  userToken: required.userToken,
  username: required.username,
  device: required.device,
  path: process.env.RIDGE_CLOUD_E2E_PATH || 'C:/code/wind',
  offsets: [0, 3, 6],
  limit: 3,
  timeoutMs: 20_000,
};
if (process.env.RIDGE_CLOUD_PANE_ID) {
  options.paneStream = {
    paneId: process.env.RIDGE_CLOUD_PANE_ID,
    write: process.env.RIDGE_CLOUD_PANE_WRITE || undefined,
    waitMs: 2_000,
  };
}

const expression = `(async()=>{try{const m=await import('/packages/remote/src/shared/cloud/__cloudE2eHarness.ts?cdp=${Date.now()}');return await m.runCloudDirChildrenE2E(${JSON.stringify(options)})}catch(error){return {__cloudE2eImportError:{name:error?.name,message:error?.message,stack:error?.stack}}}})()`;
const result = await cdp.evaluate(expression);
cdp.close();

if (result?.__cloudE2eImportError) {
  throw new Error(`Cloud E2E harness failed to load: ${JSON.stringify(result.__cloudE2eImportError)}`);
}

const probesOk = result?.results?.length > 0 && result.results.every((probe) => probe.ok);
const paneOk = !options.paneStream || (result?.paneStream?.frames ?? 0) > 0;
const ok = result?.connected === true && probesOk && Array.isArray(result.capabilities) && paneOk;
console.log(JSON.stringify({
  ok,
  connected: result?.connected === true,
  capabilities: result?.capabilities ?? null,
  results: result?.results ?? [],
  keyBindingMode: result?.keyBindingMode ?? null,
  paneStream: result?.paneStream ?? null,
  log: (result?.log ?? []).map(redact),
}));
if (!ok) process.exitCode = 1;
