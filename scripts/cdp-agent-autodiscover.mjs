#!/usr/bin/env node
// CDP e2e：验证「在 Ridge 分屏里跑起一个 agent CLI → 花名册自动收人」这条链路，
// 以及成员条目是否真的带上了监控/干预所需的字段（状态 / 活跃度 / 近期回复）。
//
// 测试替身：把系统自带的一个长驻小程序拷成 `aider.exe` 放到临时目录，在 pane 里
// 启动它。对后端而言这就是「该 pane 的 shell 子树下挂着一个 agent CLI」——正是
// autodiscover 的判据；不需要真去跑一个计费的 agent。
//
// 替身**必须**取 `aider` 而非 `claude`：本仓开发时跑的就是 claude CLI，按镜像名
// 收尸会连开发者自己的会话一起杀。收尸一律按**全路径**，永不按 `/IM <name>`。
//
// 用法：
//   终端 1: pnpm tauri:dev:cdp
//   终端 2: node scripts/cdp-agent-autodiscover.mjs
import http from 'node:http';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { resolveCdpPort } from './cdp-port.mjs';

const CDP_PORT = Number(process.env.CDP_PORT ?? resolveCdpPort());
const log = (...a) => console.log('[agent-e2e]', ...a);
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

// ── 造替身：<tmp>/ridge-agent-e2e/claude.exe ──
const dir = path.join(os.tmpdir(), 'ridge-agent-e2e');
fs.mkdirSync(dir, { recursive: true });
const fake = path.join(dir, 'aider.exe');
const src = path.join(process.env.SystemRoot ?? 'C:\\Windows', 'System32', 'timeout.exe');
if (!fs.existsSync(fake)) fs.copyFileSync(src, fake);
log('fake agent binary:', fake);
{
  // 清掉上一轮残留的替身，否则「agent 退出 → 花名册回收」这一半永远测不到。
  // 只按**这个可执行文件的全路径**匹配 —— 绝不 `taskkill /IM`，那会误伤同名真进程。
  const { execSync } = await import('node:child_process');
  const q = fake.replace(/\\/g, '\\\\');
  try {
    execSync(`wmic process where "ExecutablePath='${q}'" call terminate`, { stdio: 'ignore' });
  } catch {
    /* 没有残留 */
  }
}

const list = await httpJson('/json/list');
const t = list.find((x) => x.type === 'page' && !/devtools/.test(x.url || ''));
if (!t) throw new Error('no page target on :' + CDP_PORT);
const ws = new WebSocket(t.webSocketDebuggerUrl);
let id = 0;
const pend = new Map();
await new Promise((res, rej) => {
  ws.onopen = res;
  ws.onerror = rej;
  ws.onmessage = (e) => {
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
    ws.send(JSON.stringify({ id: i, method, params }));
  });
const ev = async (expression) => {
  const r = await send('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true });
  if (r.result?.exceptionDetails) {
    throw new Error('eval threw: ' + JSON.stringify(r.result.exceptionDetails).slice(0, 500));
  }
  return r.result?.result?.value;
};
/** 经 Tauri 内部桥直接调后端命令（页面脚本作用域拿不到打包后的 invoke）。 */
const invoke = (cmd, args = {}) =>
  ev(
    `window.__TAURI_INTERNALS__.invoke(${JSON.stringify(cmd)}, ${JSON.stringify(args)})
       .then(r => ({ ok: true, r })).catch(e => ({ ok: false, e: String(e) }))`,
  );

await send('Runtime.enable');

const wsId = (await invoke('get_active_workspace_id')).r;
// 必须取**活动**工作区里的 pane：keep-alive 让多个工作区同时在 DOM 里，
// 无作用域的 querySelector 会抓到隐藏工作区的 pane，拓扑查询自然看不到它。
const paneId = await ev(
  `document.querySelector('[data-rg-ws-pane-host="${wsId}"] [data-rg-pane-id]')?.dataset.rgPaneId ?? ''`,
);
log('workspace:', wsId, '| pane:', paneId);
if (!paneId) throw new Error('no mounted pane');

const roster0 = (await invoke('get_teammate_topology', { workspaceId: wsId })).r;
log('roster BEFORE:', JSON.stringify((roster0?.roster ?? []).map((m) => m.id)));

// 在该 pane 里启动替身 agent（600s 足够跑完本测）。CR 结尾＝真回车。
// 短时长：替身会自然退出，用来验证「agent 退了要从花名册回收」这一半。
await invoke('write_to_pty', { paneId, data: `& '${fake}' /t 25 /nobreak\r` });
log('launched fake agent in pane; waiting for autodiscover…');

let hit = null;
for (let i = 0; i < 12; i++) {
  await sleep(2500);
  const topo = (await invoke('get_teammate_topology', { workspaceId: wsId })).r;
  hit = (topo?.roster ?? []).find((m) => m.paneId === paneId && m.isAuto);
  if (hit) break;
}

if (!hit) {
  const topo = (await invoke('get_teammate_topology', { workspaceId: wsId })).r;
  console.error('[agent-e2e] FAIL: 没有自动收人。当前 roster =', JSON.stringify(topo?.roster ?? []));
  process.exitCode = 1;
} else {
  log('roster AFTER: auto member =', JSON.stringify(hit, null, 2).slice(0, 900));
  const missing = ['activity', 'outputSeq', 'recentOutput'].filter((k) => !(k in hit));
  if (missing.length) {
    console.error('[agent-e2e] FAIL: 成员缺监控字段:', missing.join(', '));
    process.exitCode = 1;
  }
  // 显示名必须是识别出来的 agent，而不是 shell 自报的 `…\powershell.exe`
  // （iter-62 e2e 首跑就是被这个盖掉的）。
  const shown = hit.title && hit.title.trim() ? hit.title : hit.name;
  if (/\.exe$/i.test(shown) || /[\\/]/.test(shown)) {
    console.error('[agent-e2e] FAIL: 成员显示名被 shell 自报标题盖掉:', shown);
    process.exitCode = 1;
  } else {
    log('显示名 =', shown);
  }
  // 干预：面板侧发消息 = 写该成员的 pane。
  const sent = await invoke('write_to_pty', { paneId: hit.paneId, data: '\u0003' });
  if (!sent.ok) {
    console.error('[agent-e2e] FAIL: 给成员发消息失败:', sent.e);
    process.exitCode = 1;
  }
  // 替身退出后应被自动回收（只回收 auto: 条目，人工标记不动）。
  let gone = false;
  for (let i = 0; i < 12; i++) {
    await sleep(2500);
    const topo = (await invoke('get_teammate_topology', { workspaceId: wsId })).r;
    if (!(topo?.roster ?? []).some((m) => m.id === hit.id)) {
      gone = true;
      break;
    }
  }
  if (!gone) {
    console.error('[agent-e2e] FAIL: agent 退出后未从花名册回收');
    process.exitCode = 1;
  } else {
    log('agent 退出后已自动回收 ✔');
  }
}

if (!process.exitCode) log('GATE: PASS');
ws.close();
