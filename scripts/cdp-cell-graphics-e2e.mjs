#!/usr/bin/env node

import fs from 'node:fs';
import http from 'node:http';
import path from 'node:path';
import sharp from 'sharp';
import { resolveCdpPort } from './cdp-port.mjs';
import { isRidgeCdpTarget } from './lib/cdpTarget.mjs';

const port = resolveCdpPort();
const artifactDir = path.resolve('.iteration/artifacts/cell-graphics');
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
        const pending = this.pending.get(message.id);
        if (!pending) return;
        this.pending.delete(message.id);
        pending(message);
      };
    });
  }

  send(method, params = {}) {
    const id = ++this.nextId;
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        this.pending.delete(id);
        reject(new Error(`${method} timed out`));
      }, 30_000);
      this.pending.set(id, (message) => {
        clearTimeout(timer);
        if (message.error) reject(new Error(`${method}: ${message.error.message}`));
        else resolve(message.result);
      });
      this.ws.send(JSON.stringify({ id, method, params }));
    });
  }

  async evaluate(expression) {
    const result = await this.send('Runtime.evaluate', {
      expression,
      awaitPromise: true,
      returnByValue: true,
    });
    if (result.exceptionDetails) {
      throw new Error(result.exceptionDetails.exception?.description || result.exceptionDetails.text);
    }
    return result.result?.value;
  }

  invoke(command, args = {}) {
    return this.evaluate(
      `window.__TAURI__.core.invoke(${JSON.stringify(command)}, ${JSON.stringify(args)})`,
    );
  }

  close() {
    try { this.ws.close(); } catch { /* already closed */ }
  }
}

async function waitUntil(probe, description, timeoutMs = 30_000) {
  const deadline = Date.now() + timeoutMs;
  let last;
  while (Date.now() < deadline) {
    last = await probe();
    if (last) return last;
    await sleep(150);
  }
  throw new Error(`${description} timed out; last=${JSON.stringify(last)}`);
}

function luminance(data, info, x, y) {
  const offset = (y * info.width + x) * info.channels;
  return Math.round((data[offset] + data[offset + 1] + data[offset + 2]) / 3);
}

function assertBrightLine(data, info, axis, coordinate, start, end, label) {
  let dark = 0;
  let total = 0;
  for (let offset = start; offset < end; offset += 1) {
    const x = axis === 'x' ? coordinate : offset;
    const y = axis === 'y' ? coordinate : offset;
    if (x < 0 || y < 0 || x >= info.width || y >= info.height) continue;
    total += 1;
    if (luminance(data, info, x, y) < 24) dark += 1;
  }
  if (total === 0 || dark / total > 0.02) {
    throw new Error(`${label} contains ${dark}/${total} dark pixels`);
  }
  return { dark, total };
}

function brightCoordinates(data, info, axis, fixedStart, fixedEnd, scanStart, scanEnd) {
  const values = [];
  for (let scan = scanStart; scan < scanEnd; scan += 1) {
    let bright = false;
    for (let fixed = fixedStart; fixed < fixedEnd; fixed += 1) {
      const x = axis === 'x' ? scan : fixed;
      const y = axis === 'y' ? scan : fixed;
      if (x >= 0 && y >= 0 && x < info.width && y < info.height
        && luminance(data, info, x, y) >= 24) {
        bright = true;
        break;
      }
    }
    if (bright) values.push(scan);
  }
  return values;
}

function assertBrightPixels(data, info, points, label) {
  const dark = points.filter(([x, y]) => luminance(data, info, x, y) < 24).length;
  if (points.length === 0 || dark > 0) {
    throw new Error(`${label} contains ${dark}/${points.length} dark pixels`);
  }
  return { dark, total: points.length };
}

const targets = await listTargets();
const target = targets.find((item) => isRidgeCdpTarget(item) && item.webSocketDebuggerUrl)
  ?? targets.find((item) => item.type === 'page' && item.webSocketDebuggerUrl);
if (!target) throw new Error(`no Ridge CDP page on 127.0.0.1:${port}`);

const cdp = new Cdp(target.webSocketDebuggerUrl);
let testPaneId = null;
try {
  await cdp.open();
  await cdp.send('Runtime.enable');
  await cdp.send('Page.enable');
  const sourcePaneId = await waitUntil(
    () => cdp.evaluate(`([...document.querySelectorAll('.rg-pane-container')]
      .find((element) => element.getBoundingClientRect().width > 20)?.dataset.rgPaneId) || null`),
    'visible source pane',
  );
  const workspaceId = await cdp.invoke('get_window_active_workspace_id');
  const split = await cdp.invoke('split_pane', { paneId: sourcePaneId, direction: 'horizontal' });
  testPaneId = split?.pane_id ?? split?.paneId;
  if (!testPaneId) throw new Error(`split_pane returned no pane id: ${JSON.stringify(split)}`);

  await waitUntil(
    () => cdp.evaluate(`Boolean(document.querySelector(${JSON.stringify(`[data-rg-pane-id="${testPaneId}"]`)}))`),
    'test pane mount',
  );
  await waitUntil(async () => {
    const rows = await cdp.evaluate(`window.__windE2E?.visibleText(${JSON.stringify(testPaneId)}) ?? []`);
    return rows.some((row) => /PS\s+[^>]*>/.test(row)) ? rows : null;
  }, 'test pane PowerShell prompt');

  const command = [
    '$f=[string][char]0x2588',
    '$lo=[string][char]0x2584',
    '$up=[string][char]0x2580',
    '$rh=[string][char]0x2590',
    '$ul=[string][char]0x259B',
    '$ur=[string][char]0x259C',
    '$lh=[string][char]0x258C',
    '$nw=[string][char]0x256D',
    '$ne=[string][char]0x256E',
    '$sw=[string][char]0x2570',
    '$se=[string][char]0x256F',
    '$h=[string][char]0x2500',
    '$v=[string][char]0x2502',
    'Write-Output "RIDGE_CELL_GRAPHICS"',
    'Write-Output ($f+$f)',
    'Write-Output ($f+$f)',
    'Write-Output ($lo+$lo)',
    'Write-Output ($up+$up)',
    'Write-Output ($rh+$ul+$f+$f+$f+$ur+$lh)',
    'Write-Output ($nw+$h+$h+$h+$h+$ne)',
    'Write-Output ($v+"    "+$v)',
    'Write-Output ($sw+$h+$h+$h+$h+$se)',
  ].join(';');
  await cdp.invoke('write_to_pty', { workspaceId, paneId: testPaneId, data: `${command}\r` });

  let observedRows = [];
  const rows = await waitUntil(async () => {
    const visible = await cdp.evaluate(`window.__windE2E?.visibleText(${JSON.stringify(testPaneId)}) ?? []`);
    observedRows = visible;
    const marker = visible.findLastIndex((row) => row.trim() === 'RIDGE_CELL_GRAPHICS');
    return marker >= 0 && visible[marker + 5]?.includes('\u2590') ? { visible, marker } : null;
  }, 'cell graphic fixture').catch((error) => {
    throw new Error(`${error.message}; tail=${JSON.stringify(observedRows.slice(-12))}`);
  });
  await cdp.evaluate('new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)))');

  const geometry = await cdp.evaluate(`(() => {
    const element = document.querySelector(${JSON.stringify(`[data-rg-pane-id="${testPaneId}"]`)});
    const anchor = window.__windE2E?.inputAnchorResolved(${JSON.stringify(testPaneId)});
    if (!element || !anchor) return null;
    const rect = element.getBoundingClientRect();
    return { rect: { left: rect.left, top: rect.top }, anchor };
  })()`);
  if (!geometry) throw new Error('test pane geometry unavailable');

  const shot = await cdp.send('Page.captureScreenshot', { format: 'png', fromSurface: true });
  const image = Buffer.from(shot.data, 'base64');
  fs.mkdirSync(artifactDir, { recursive: true });
  const screenshot = path.join(artifactDir, 'cell-graphics.png');
  fs.writeFileSync(screenshot, image);
  const metadata = await sharp(image).metadata();
  const viewport = await cdp.evaluate('({ width: innerWidth, height: innerHeight })');
  const scaleX = (metadata.width || viewport.width) / viewport.width;
  const scaleY = (metadata.height || viewport.height) / viewport.height;
  const gridLeft = geometry.rect.left + geometry.anchor.x - geometry.anchor.col * geometry.anchor.cellW;
  const gridTop = geometry.rect.top + geometry.anchor.y - geometry.anchor.row * geometry.anchor.cellH;
  const rowTop = (row) => Math.round((gridTop + row * geometry.anchor.cellH) * scaleY);
  const colLeft = (col) => Math.round((gridLeft + col * geometry.anchor.cellW) * scaleX);
  const { data, info } = await sharp(image).raw().toBuffer({ resolveWithObject: true });
  const firstRow = rows.marker + 1;
  const verticalBoundary = colLeft(1);
  const fullTop = rowTop(firstRow);
  const fullBottom = rowTop(firstRow + 2);
  const fullRowBoundary = rowTop(firstRow + 1);
  const halfRowBoundary = rowTop(firstRow + 3);
  const left = colLeft(0);
  const right = colLeft(2);
  const checks = {
    fullVerticalLeft: assertBrightLine(data, info, 'x', verticalBoundary - 1, fullTop, fullBottom, 'full block left seam'),
    fullVerticalRight: assertBrightLine(data, info, 'x', verticalBoundary, fullTop, fullBottom, 'full block right seam'),
    fullHorizontalTop: assertBrightLine(data, info, 'y', fullRowBoundary - 1, left, right, 'full block top seam'),
    fullHorizontalBottom: assertBrightLine(data, info, 'y', fullRowBoundary, left, right, 'full block bottom seam'),
    halfHorizontalTop: assertBrightLine(data, info, 'y', halfRowBoundary - 1, left, right, 'lower/upper block top seam'),
    halfHorizontalBottom: assertBrightLine(data, info, 'y', halfRowBoundary, left, right, 'lower/upper block bottom seam'),
  };
  const frameTop = firstRow + 5;
  const topAxis = brightCoordinates(
    data,
    info,
    'y',
    colLeft(2),
    colLeft(3),
    rowTop(frameTop),
    rowTop(frameTop + 1),
  );
  const bottomAxis = brightCoordinates(
    data,
    info,
    'y',
    colLeft(2),
    colLeft(3),
    rowTop(frameTop + 2),
    rowTop(frameTop + 3),
  );
  const leftAxis = brightCoordinates(
    data,
    info,
    'x',
    rowTop(frameTop + 1),
    rowTop(frameTop + 2),
    colLeft(0),
    colLeft(1),
  );
  const rightAxis = brightCoordinates(
    data,
    info,
    'x',
    rowTop(frameTop + 1),
    rowTop(frameTop + 2),
    colLeft(5),
    colLeft(6),
  );
  checks.roundedHorizontal = assertBrightPixels(
    data,
    info,
    [...topAxis.flatMap((y) => [
      [colLeft(1) - 1, y], [colLeft(1), y], [colLeft(5) - 1, y], [colLeft(5), y],
    ]), ...bottomAxis.flatMap((y) => [
      [colLeft(1) - 1, y], [colLeft(1), y], [colLeft(5) - 1, y], [colLeft(5), y],
    ])],
    'rounded horizontal joins',
  );
  checks.roundedVertical = assertBrightPixels(
    data,
    info,
    [...leftAxis.flatMap((x) => [
      [x, rowTop(frameTop + 1) - 1], [x, rowTop(frameTop + 1)],
      [x, rowTop(frameTop + 2) - 1], [x, rowTop(frameTop + 2)],
    ]), ...rightAxis.flatMap((x) => [
      [x, rowTop(frameTop + 1) - 1], [x, rowTop(frameTop + 1)],
      [x, rowTop(frameTop + 2) - 1], [x, rowTop(frameTop + 2)],
    ])],
    'rounded vertical joins',
  );
  console.log(JSON.stringify({ ok: true, screenshot, markerRow: rows.marker, checks }, null, 2));
} finally {
  if (testPaneId) await cdp.invoke('close_pane', { paneId: testPaneId }).catch(() => {});
  cdp.close();
}
