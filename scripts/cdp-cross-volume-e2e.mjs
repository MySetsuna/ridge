#!/usr/bin/env node
// Real Tauri cross-volume move acceptance probe.
// Verifies rename failure -> copy -> source delete through move_path, then
// verifies reverse movement and removes only its unique test directories.

import http from 'node:http';
import { resolveCdpPort } from './cdp-port.mjs';

const port = resolveCdpPort();
const runId = `${Date.now()}-${process.pid}`;
const source = process.env.RIDGE_CROSS_VOLUME_SOURCE || `C:/code/wind/.iteration/cross-volume-e2e-${runId}`;
const target = process.env.RIDGE_CROSS_VOLUME_TARGET || `D:/ridge-cross-volume-e2e-${runId}`;
const markerText = 'cross-volume-permission-e2e';

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

  async invoke(command, args = {}) {
    const response = await this.send('Runtime.evaluate', {
      expression: `window.__TAURI__.core.invoke(${JSON.stringify(command)}, ${JSON.stringify(args)})`,
      awaitPromise: true,
      returnByValue: true,
    });
    if (response.result?.exceptionDetails) {
      throw new Error(`${command}: ${JSON.stringify(response.result.exceptionDetails)}`);
    }
    return response.result?.result?.value;
  }

  close() {
    try { this.ws.close(); } catch { /* already closed */ }
  }
}

const targetInfo = (await listTargets()).find((item) => item.type === 'page' && item.webSocketDebuggerUrl);
if (!targetInfo) throw new Error(`no Ridge CDP page on 127.0.0.1:${port}`);

const cdp = new Cdp(targetInfo.webSocketDebuggerUrl);
let sourceCreated = false;
let targetCreated = false;
try {
  await cdp.open();
  await cdp.send('Runtime.enable');
  await cdp.invoke('create_directory', { path: source });
  sourceCreated = true;
  await cdp.invoke('write_file', { path: `${source}/marker.txt`, content: markerText });
  await cdp.invoke('move_path', { from: source, to: target });
  sourceCreated = false;
  targetCreated = true;
  const moved = await cdp.invoke('read_file', { path: `${target}/marker.txt` });
  await cdp.invoke('move_path', { from: target, to: source });
  targetCreated = false;
  sourceCreated = true;
  const roundTrip = await cdp.invoke('read_file', { path: `${source}/marker.txt` });
  if (moved !== markerText || roundTrip !== markerText) {
    throw new Error(`cross-volume content mismatch: ${JSON.stringify({ moved, roundTrip })}`);
  }
  console.log(JSON.stringify({ ok: true, movedBytes: moved.length, roundTripBytes: roundTrip.length, source, target }));
} finally {
  if (sourceCreated || targetCreated) {
    await cdp.invoke('delete_path', { path: sourceCreated ? source : target }).catch(() => {});
  }
  cdp.close();
}
