#!/usr/bin/env node
// 取「开新工作区 tab」瞬间的**完整** console/异常文本（不截断），用于给
// Svelte `effect_update_depth_exceeded` 之类的自循环定名。
import http from 'node:http';
import { resolveCdpPort } from './cdp-port.mjs';

const CDP_PORT = Number(process.env.CDP_PORT ?? resolveCdpPort());
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const httpJson = (p) =>
  new Promise((res, rej) => {
    http
      .get({ host: '127.0.0.1', port: CDP_PORT, path: p, timeout: 5000 }, (r) => {
        let b = '';
        r.on('data', (c) => (b += c));
        r.on('end', () => {
          try {
            res(JSON.parse(b));
          } catch (e) {
            rej(e);
          }
        });
      })
      .on('error', rej);
  });

const list = await httpJson('/json/list');
const t = list.find((x) => x.type === 'page' && !/devtools/.test(x.url || ''));
if (!t) throw new Error('no page target');
const ws = new WebSocket(t.webSocketDebuggerUrl);
let id = 0;
const pend = new Map();
const seen = [];
await new Promise((res, rej) => {
  ws.onopen = res;
  ws.onerror = rej;
  ws.onmessage = (ev) => {
    const m = JSON.parse(ev.data);
    if (m.id && pend.has(m.id)) {
      pend.get(m.id)(m);
      pend.delete(m.id);
    } else if (m.method === 'Runtime.consoleAPICalled' || m.method === 'Runtime.exceptionThrown') {
      seen.push(m);
    }
  };
});
const send = (method, params = {}) =>
  new Promise((res) => {
    const i = ++id;
    pend.set(i, res);
    ws.send(JSON.stringify({ id: i, method, params }));
  });
const ev = async (expression) => {
  const r = await send('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true });
  return r.result?.result?.value;
};

await send('Runtime.enable');
console.log('[detail] reloading for a clean reactive tree…');
await send('Page.enable');
await send('Page.reload');
await sleep(9000);
seen.length = 0;

const before = await ev(
  `({ ws: document.querySelectorAll('[data-rg-ws-pane-host]').length,
      panes: document.querySelectorAll('.rg-pane-container').length })`,
);
console.log('[detail] before:', JSON.stringify(before));

for (let i = 0; i < 2; i++) {
  const r = await ev(
    `(() => { const b=[...document.querySelectorAll('button')].find(x=>x.textContent.trim()==='+' && x.className.includes('border-dashed')); if(!b) return 'no-btn'; b.click(); return 'ok'; })()`,
  );
  console.log(`[detail] click #${i + 1}: ${r}`);
  await sleep(7000);
  const after = await ev(
    `({ ws: document.querySelectorAll('[data-rg-ws-pane-host]').length,
        panes: document.querySelectorAll('.rg-pane-container').length,
        tabs: document.querySelectorAll('[data-rg-ws-tab], [role="tab"]').length })`,
  );
  console.log(`[detail] after click #${i + 1}:`, JSON.stringify(after));
}

console.log(`\n[detail] ===== ${seen.length} console/exception records =====\n`);
for (const m of seen) {
  if (m.method === 'Runtime.exceptionThrown') {
    const d = m.params.exceptionDetails;
    console.log(`--- EXCEPTION: ${d.text} ${d.exception?.description ?? ''}`.slice(0, 3000));
    continue;
  }
  const type = m.params.type;
  if (type === 'debug') continue;
  const text = (m.params.args ?? [])
    .map((a) => a.value ?? a.description ?? JSON.stringify(a.preview ?? {}))
    .join(' ');
  console.log(`--- ${type.toUpperCase()}: ${String(text).slice(0, 3000)}`);
}
ws.close();
