#!/usr/bin/env node
// CDP e2e：验证 agent **面板 UI** 本身（不只是后端数据链路）。
//
// 用户诉求原文是「不是只读展示的成员列表，而是可监控、可干预、可展示状态、
// 可展示近期任务和近期回复、可以给每个 agent 在面板侧发送消息的管理列表面板」，
// 且「编组 tab 在展示面上是一致的」。这些都是**渲染结果**，后端拓扑对了不代表
// 面板画对了——所以单独一条 e2e 钉死 DOM。
//
// 用法：
//   终端 1: pnpm tauri:dev:cdp
//   终端 2: node scripts/cdp-agent-panel-ui.mjs
import http from 'node:http';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { resolveCdpPort } from './cdp-port.mjs';

const CDP_PORT = Number(process.env.CDP_PORT ?? resolveCdpPort());
const log = (...a) => console.log('[panel-ui]', ...a);
const fail = (...a) => {
  console.error('[panel-ui] FAIL:', ...a);
  process.exitCode = 1;
};
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

// ── 替身 agent：同 cdp-agent-autodiscover.mjs，取 `aider` 而非 `claude`
// （按名收尸会误杀开发者自己的 claude 会话），收尸一律按全路径。 ──
const dir = path.join(os.tmpdir(), 'ridge-agent-e2e');
fs.mkdirSync(dir, { recursive: true });
// 替身取 `ping.exe`（而非 timeout.exe）：本测要在替身跑着的同时**继续用这个 shell
// 回显**面板发来的消息，而 `timeout /nobreak` 会独占并吞掉键入。ping 不读 stdin，
// 再配 `Start-Process` 放后台，shell 停在提示符上，干预链路才测得到。
const fake = path.join(dir, 'aider.exe');
const src = path.join(process.env.SystemRoot ?? String.raw`C:\Windows`, 'System32', 'PING.EXE');
{
  const { spawnSync } = await import('node:child_process');
  const q = fake.replaceAll("'", "''");
  spawnSync('powershell.exe', [
    '-NoProfile',
    '-NonInteractive',
    '-Command',
    `Get-CimInstance Win32_Process | Where-Object { $_.ExecutablePath -eq '${q}' } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force }`,
  ], { stdio: 'ignore' });
  // 先收尸再覆盖：两条 e2e 用同一个文件名但不同底座（ping / timeout），
  // 「已存在就跳过拷贝」会把上一条的二进制留下来，参数对不上即刻退出。
  fs.copyFileSync(src, fake);
}

const list = await httpJson('/json/list');
const t = list.find((x) => x.type === 'page' && !/devtools/.test(x.url || ''));
if (!t) throw new Error('no page target on :' + CDP_PORT);
const sock = new WebSocket(t.webSocketDebuggerUrl);
let id = 0;
const pend = new Map();
await new Promise((res, rej) => {
  sock.onopen = res;
  sock.onerror = rej;
  sock.onmessage = (e) => {
    const m = JSON.parse(e.data);
    if (m.id && pend.has(m.id)) {
      pend.get(m.id)(m);
      pend.delete(m.id);
    }
  };
});
const send = (method, params = {}) =>
  new Promise((res) => {
    const i = ++id;
    pend.set(i, res);
    sock.send(JSON.stringify({ id: i, method, params }));
  });
const ev = async (expression) => {
  const r = await send('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true });
  if (r.result?.exceptionDetails) {
    throw new Error('eval threw: ' + JSON.stringify(r.result.exceptionDetails).slice(0, 500));
  }
  return r.result?.result?.value;
};
const invoke = (cmd, args = {}) =>
  ev(
    `window.__TAURI_INTERNALS__.invoke(${JSON.stringify(cmd)}, ${JSON.stringify(args)})
       .then(r => ({ ok: true, r })).catch(e => ({ ok: false, e: String(e) }))`,
  );

await send('Runtime.enable');

const wsId = (await invoke('get_active_workspace_id')).r;
const paneId = await ev(
  `document.querySelector('[data-rg-ws-pane-host="${wsId}"] [data-rg-pane-id]')?.dataset.rgPaneId ?? ''`,
);
if (!paneId) throw new Error('no mounted pane');
log('workspace:', wsId, '| pane:', paneId);

// 替身跑够整场测试（面板轮询 + 两个 Tab 断言），且放后台以便 shell 继续回显。
await invoke('write_to_pty', {
  paneId,
  data: `Start-Process -FilePath '${fake}' -ArgumentList '-n','200','127.0.0.1' -WindowStyle Hidden\r`,
});
log('fake agent launched; opening Agent panel…');

// 打开左侧图标栏的 agent Tab。
const opened = await ev(
  String.raw`(() => {
     const b = document.querySelector('button[title="Agent\u0027s Commune"]');
     if (!b) return 'no-tab-button';
     b.click();
     return 'clicked';
   })()`,
);
if (opened !== 'clicked') fail('找不到 Agent 面板入口按钮:', opened);

const membersTab = await ev(
  `(() => {
     const b = [...document.querySelectorAll('button')]
       .find((x) => x.textContent.includes('成员') && x.textContent.trim().length <= 4);
     if (!b) return 'no-members-tab';
     b.click();
     return 'clicked';
   })()`,
);
if (membersTab !== 'clicked') fail('找不到「成员」Tab:', membersTab);

/** 抓「某成员行」的能力清单：状态徽标 / 最近回复 / 发消息输入框 + 发送键。 */
const ROW_PROBE = `(() => {
  const ta = [...document.querySelectorAll('textarea')]
    .find((x) => (x.placeholder || '').startsWith('给 ')
      && /自动/.test(x.closest('li')?.innerText || ''));
  if (!ta) return { found: false };
  const li = ta.closest('li');
  const text = li ? li.innerText : '';
  return {
    found: true,
    text,
    hasStatus: /运行中|空闲|已暂停|失联|等待审批/.test(text),
    hasAuto: /自动/.test(text),
    hasRecent: /最近回复/.test(text),
    hasSend: !!li?.querySelector('button[aria-label="发送给该成员"]'),
    hasSuspend: !!li?.querySelector('button[title*="暂停"], button[title*="恢复"]'),
  };
})()`;

let row = null;
for (let i = 0; i < 16; i++) {
  await sleep(2000);
  row = await ev(ROW_PROBE);
  if (row?.found) break;
}
if (!row?.found) {
  fail('成员 Tab 里没有渲染出该 agent 的成员行');
} else {
  log('成员行 innerText =', JSON.stringify(row.text).slice(0, 300));
  for (const [k, msg] of [
    ['hasStatus', '缺工作状态徽标'],
    ['hasAuto', '缺「自动」识别标记'],
    ['hasRecent', '缺「最近回复」区'],
    ['hasSend', '缺发送按钮'],
    ['hasSuspend', '缺暂停/恢复干预键'],
  ]) {
    if (!row[k]) fail('成员 Tab', msg);
  }

  // 干预链路：在面板侧输入框打字 → 点发送 → 该 pane 的 scrollback 里出现这段文本。
  const marker = 'rg-panel-probe-' + Math.floor(performance.now?.() ?? 0);
  const sent = await ev(
    `(() => {
       const ta = [...document.querySelectorAll('textarea')]
         .find((x) => (x.placeholder || '').startsWith('给 ')
           && /自动/.test(x.closest('li')?.innerText || ''));
       if (!ta) return 'no-textarea';
       const setter = Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value').set;
       setter.call(ta, ${JSON.stringify(marker)});
       ta.dispatchEvent(new Event('input', { bubbles: true }));
       const btn = ta.closest('li')?.querySelector('button[aria-label="发送给该成员"]');
       if (!btn) return 'no-send-button';
       btn.click();
       return 'sent';
     })()`,
  );
  if (sent !== 'sent') fail('面板侧发消息失败:', sent);
  else {
    let echoed = false;
    for (let i = 0; i < 8; i++) {
      await sleep(1200);
      const tail = await invoke('get_pane_scrollback_tail', { paneId, maxBytes: 16384 });
      if (typeof tail.r?.bytes === 'string' && tail.r.bytes.includes(marker)) {
        echoed = true;
        break;
      }
    }
    if (!echoed) fail('面板侧发出的消息没有落到该成员的 pane');
    else log('面板侧 → pane 干预链路 ✔');
  }
}

// ── 编组 Tab：用户明确要求「两个 tab 在展示面上是一致的」 ──
const switched = await ev(
  `(() => {
     // Tab 按钮里含图标 svg，文本前后有空白，故按「短且含『编组』」匹配。
     const b = [...document.querySelectorAll('button')]
       .find((x) => x.textContent.includes('编组') && x.textContent.trim().length <= 4);
     if (!b) return 'no-groups-tab';
     b.click();
     return 'clicked';
   })()`,
);
if (switched !== 'clicked') fail('找不到「编组」Tab:', switched);
else {
  await sleep(1500);
  const g = await ev(ROW_PROBE);
  if (!g?.found) fail('编组 Tab 里没有渲染出成员行（要求与成员 Tab 展示一致）');
  else {
    for (const [k, msg] of [
      ['hasStatus', '缺工作状态徽标'],
      ['hasRecent', '缺「最近回复」区'],
      ['hasSend', '缺发送按钮（要求所有成员可发消息，非只组长）'],
    ]) {
      if (!g[k]) fail('编组 Tab', msg);
    }
    if (process.exitCode !== 1) log('编组 Tab 与成员 Tab 展示一致 ✔');
  }
}

// 收尾：结束替身，别把它留给下一次跑。
{
  const { spawnSync } = await import('node:child_process');
  const q = fake.replaceAll("'", "''");
  spawnSync('powershell.exe', [
    '-NoProfile',
    '-NonInteractive',
    '-Command',
    `Get-CimInstance Win32_Process | Where-Object { $_.ExecutablePath -eq '${q}' } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force }`,
  ], { stdio: 'ignore' });
}

if (!process.exitCode) log('GATE: PASS');
sock.close();
