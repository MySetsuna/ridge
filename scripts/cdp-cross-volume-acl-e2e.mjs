#!/usr/bin/env node
// Physical Windows ACL rejection probe for the cross-volume copy/delete
// sequence used by move_path.
// The fixture is unique, denied only for the current user, and restored before
// product cleanup so a failed assertion never leaves an unreadable directory.

import { spawn, spawnSync } from 'node:child_process';
import http from 'node:http';
import { resolveCdpPort } from './cdp-port.mjs';

const port = resolveCdpPort();
const runId = `${Date.now()}-${process.pid}`;
const source = process.env.RIDGE_ACL_SOURCE || `C:/code/wind/.iteration/acl-e2e-${runId}`;
const target = process.env.RIDGE_ACL_TARGET || `D:/ridge-acl-e2e-${runId}`;
const marker = `${source}/marker.txt`;
const targetMarker = `${target}/marker.txt`;
const markerText = 'cross-volume-acl-e2e';

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

function runProcess(file, args, timeoutMs = 10_000) {
  return new Promise((resolve, reject) => {
    const child = spawn(file, args, { windowsHide: true, stdio: ['ignore', 'pipe', 'pipe'] });
    let stdout = '';
    let stderr = '';
    let settled = false;
    const timer = setTimeout(() => {
      if (settled) return;
      settled = true;
      if (child.pid) {
        spawnSync('taskkill.exe', ['/PID', String(child.pid), '/T', '/F'], {
          windowsHide: true,
          stdio: 'ignore',
        });
      }
      reject(new Error(`${file} timed out after ${timeoutMs}ms`));
    }, timeoutMs);
    child.stdout.on('data', (chunk) => (stdout += chunk));
    child.stderr.on('data', (chunk) => (stderr += chunk));
    child.on('error', (error) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      reject(error);
    });
    child.on('close', (code) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      if (code !== 0) {
        reject(new Error(`${file} exited ${code}: ${stderr || stdout}`.trim()));
        return;
      }
      resolve(stdout.trim());
    });
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

async function expectFailure(operation) {
  try {
    await operation();
    return null;
  } catch (error) {
    return error instanceof Error ? error.message : String(error);
  }
}

const account = `${process.env.USERDOMAIN || ''}${process.env.USERDOMAIN ? '\\' : ''}${process.env.USERNAME || ''}`;
if (!account) throw new Error('cannot resolve current Windows account for ACL fixture');
const targetInfo = (await listTargets()).find((item) => item.type === 'page' && item.webSocketDebuggerUrl);
if (!targetInfo) throw new Error(`no Ridge CDP page on 127.0.0.1:${port}`);

const cdp = new Cdp(targetInfo.webSocketDebuggerUrl);
let sourceCreated = false;
let targetCreated = false;
let denyApplied = false;
const denyRules = [
  [source, 'DC'],
  [marker, 'DE'],
];
try {
  await cdp.open();
  await cdp.send('Runtime.enable');
  await cdp.invoke('create_directory', { path: source });
  sourceCreated = true;
  await cdp.invoke('write_file', { path: marker, content: markerText });
  await cdp.invoke('copy_path', { from: source, to: target, overwrite: false });
  targetCreated = true;

  // Apply the denial after the physical cross-volume copy has completed. This
  // isolates the source-delete failure without making the copy itself unreadable.
  for (const [path, rights] of denyRules) {
    await runProcess('icacls.exe', [path, '/deny', `${account}:(${rights})`]);
  }
  denyApplied = true;

  const deleteError = await expectFailure(() => cdp.invoke('delete_path', { path: source }));
  if (!deleteError) throw new Error('delete_path unexpectedly succeeded despite source DELETE denial');
  const copiedTarget = await cdp.invoke('read_file', { path: targetMarker });

  for (const [path] of denyRules) {
    await runProcess('icacls.exe', [path, '/remove:d', account]);
  }
  denyApplied = false;
  const sourcePreserved = await cdp.invoke('path_exists', { path: source });
  const preservedSource = await cdp.invoke('read_file', { path: marker });
  if (copiedTarget !== markerText || !sourcePreserved || preservedSource !== markerText) {
    throw new Error(`partial move preservation mismatch: ${JSON.stringify({ copiedTarget, sourcePreserved, preservedSource })}`);
  }

  console.log(JSON.stringify({
    ok: true,
    account,
    copyCompleted: true,
    sourceDeleteRejected: deleteError,
    destinationCopied: copiedTarget === markerText,
    sourcePreserved,
    source,
    target,
  }));
} finally {
  if (denyApplied) {
    for (const [path] of denyRules) {
      await runProcess('icacls.exe', [path, '/remove:d', account]).catch(() => {});
    }
  }
  if (targetCreated) await cdp.invoke('delete_path', { path: target }).catch(() => {});
  if (sourceCreated) await cdp.invoke('delete_path', { path: source }).catch(() => {});
  cdp.close();
}
