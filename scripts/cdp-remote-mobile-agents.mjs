#!/usr/bin/env node
// CDP e2e：手机端 remote 的 agent 监控/干预面板（用户需求「手机端也要能标记出
// 哪些终端是 agent、分别给不同 agent 发消息、看状态，与桌面端接近」）。
//
// 全链路真跑，不 mock：
//   1. 经 Tauri 桥打开 LAN remote 服务，取真端口与真 TOTP 码
//   2. 对 `POST /verify` 提交真验证码换 token
//   3. 把 CDP 页面**导航到手机 SPA 本体**（`?ui=mobile`），token 注入 localStorage
//   4. 断言手机端花名册渲染出：agent 标记 / 工作状态 / 最近回复 / 每人一个输入框
//   5. 跑完导航回 dev 页，别把开发会话留在远程页上
//
// 用法：
//   终端 1: pnpm tauri:dev:cdp
//   终端 2: node scripts/cdp-remote-mobile-agents.mjs
import http from 'node:http';
import https from 'node:https';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { resolveCdpPort } from './cdp-port.mjs';

const CDP_PORT = Number(process.env.CDP_PORT ?? resolveCdpPort());
const log = (...a) => console.log('[remote-mobile]', ...a);
const fail = (...a) => {
  console.error('[remote-mobile] FAIL:', ...a);
  process.exitCode = 1;
};
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

const httpJson = (p, port = CDP_PORT) =>
  new Promise((res, rej) => {
    http
      .get({ host: '127.0.0.1', port, path: p, timeout: 5000 }, (r) => {
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

// remote 服务是**自签 TLS**（见 serve.rs 的 tls_enabled / crate::tls），故走 https
// 且不校验证书链——这正是手机端首次连要走 CertTrustGuide 的那张证书。
const postForm = (port, p, body) =>
  new Promise((res, rej) => {
    const req = https.request(
      {
        host: '127.0.0.1',
        port,
        path: p,
        method: 'POST',
        timeout: 8000,
        rejectUnauthorized: false,
        headers: {
          'Content-Type': 'application/x-www-form-urlencoded',
          'Content-Length': Buffer.byteLength(body),
        },
      },
      (r) => {
        let b = '';
        r.on('data', (c) => (b += c));
        r.on('end', () => res({ status: r.statusCode, body: b }));
      },
    );
    req.on('error', rej);
    req.end(body);
  });

// ── 替身 agent（同其它 e2e：取 aider/ping，收尸按全路径，绝不按镜像名）──
const dir = path.join(os.tmpdir(), 'ridge-agent-e2e');
fs.mkdirSync(dir, { recursive: true });
const fake = path.join(dir, 'aider.exe');
const killFake = async () => {
  const { spawnSync } = await import('node:child_process');
  const q = fake.replace(/'/g, "''");
  spawnSync('powershell.exe', [
    '-NoProfile',
    '-NonInteractive',
    '-Command',
    `Get-CimInstance Win32_Process | Where-Object { $_.ExecutablePath -eq '${q}' } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force }`,
  ], { stdio: 'ignore' });
};
await killFake();
fs.copyFileSync(path.join(process.env.SystemRoot ?? 'C:\\Windows', 'System32', 'PING.EXE'), fake);

// ── CDP ──
const list = await httpJson('/json/list');
const t = list.find((x) => x.type === 'page' && !/devtools/.test(x.url || ''));
if (!t) throw new Error('no page target on :' + CDP_PORT);
const DEV_URL = process.env.RIDGE_DEV_URL ?? t.url;
const sock = new WebSocket(t.webSocketDebuggerUrl);
let id = 0;
const pend = new Map();
// 页面日志：手机端 SPA 里出错只会在 roster 上显示一个「—」，不抓 console 根本
// 不知道是鉴权挂了还是命令被拒（首跑就在这上面绕了两轮）。
const pageLogs = [];
await new Promise((res, rej) => {
  sock.onopen = res;
  sock.onerror = rej;
  sock.onmessage = (e) => {
    const m = JSON.parse(e.data);
    if (m.id && pend.has(m.id)) {
      pend.get(m.id)(m);
      pend.delete(m.id);
    } else if (m.method === 'Runtime.consoleAPICalled') {
      const txt = (m.params.args ?? [])
        .map((a) => a.value ?? a.description ?? '')
        .join(' ')
        .slice(0, 300);
      if (/error|warn/i.test(m.params.type) || /fail|denied|error/i.test(txt)) {
        pageLogs.push(`${m.params.type}: ${txt}`);
      }
    } else if (m.method === 'Runtime.exceptionThrown') {
      pageLogs.push('EXC: ' + (m.params.exceptionDetails?.exception?.description ?? '').slice(0, 300));
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
await send('Page.enable');
// 自签证书：不忽略的话 WebView2 会停在拦截页，SPA 根本加载不到。
await send('Security.enable');
await send('Security.setIgnoreCertificateErrors', { ignore: true });

const finish = async () => {
  await killFake();
  // 无论成败都把开发会话导航回去，别留在远程页上。
  try {
    await send('Page.navigate', { url: DEV_URL });
  } catch {
    /* 页已关 */
  }
  if (!process.exitCode) log('GATE: PASS');
  sock.close();
};

try {
  // 1) 起替身 agent，让花名册里真有人
  const wsId = (await invoke('get_active_workspace_id')).r;
  const paneId = await ev(
    `document.querySelector('[data-rg-ws-pane-host="${wsId}"] [data-rg-pane-id]')?.dataset.rgPaneId ?? ''`,
  );
  if (!paneId) throw new Error('no mounted pane');
  await invoke('write_to_pty', {
    paneId,
    data: `Start-Process -FilePath '${fake}' -ArgumentList '-n','200','127.0.0.1' -WindowStyle Hidden\r`,
  });
  log('workspace:', wsId, '| pane:', paneId, '| fake agent launched');

  // 2) 开 LAN remote，取真端口与真 TOTP 码
  const enabled = await invoke('set_remote_enabled', { enabled: true });
  if (!enabled.ok) throw new Error('set_remote_enabled failed: ' + enabled.e);
  let info = null;
  for (let i = 0; i < 20; i++) {
    await sleep(1000);
    const r = await invoke('get_remote_info');
    if (r.ok && r.r?.port) {
      info = r.r;
      break;
    }
  }
  if (!info) throw new Error('remote server never reported a port');
  log('remote port:', info.port, '| totp:', info.totpCode);

  // 3) 真验码换 token
  const device = 'e2e-mobile-probe';
  const verified = await postForm(
    info.port,
    '/verify',
    `code=${encodeURIComponent(String(info.totpCode))}&device=${encodeURIComponent(device)}`,
  );
  if (verified.status !== 200) throw new Error(`/verify → ${verified.status} ${verified.body}`);
  const token = JSON.parse(verified.body).token;
  if (!token) throw new Error('/verify returned no token: ' + verified.body);
  log('token acquired ✔');

  // 4) 导航到**手机 SPA 本体**并注入 token（`?ui=mobile` 压过桌面 UA 分流）
  const mobileUrl = `https://127.0.0.1:${info.port}/?ui=mobile`;
  await send('Page.navigate', { url: mobileUrl });
  await sleep(1500);
  // token 与 **device id** 必须成对：host 按 (token, device) 校验会话，
  // 只塞 token 而让 SPA 自己随机生成 device 会被直接拒（首跑就是这么红的）。
  await ev(
    `(() => { try {
       localStorage.setItem('ridge_remote_token', ${JSON.stringify(token)});
       localStorage.setItem('ridge_remote_device', ${JSON.stringify(device)});
     } catch {} return 1; })()`,
  );
  // The LAN endpoint can retain an older self-signed ServiceWorker between
  // runs. Unregister it before the second navigation so this probe exercises
  // the freshly built mobile assets instead of a stale precache revision.
  await ev(`(async () => {
    try {
      for (const registration of await navigator.serviceWorker?.getRegistrations?.() ?? []) {
        await registration.unregister();
      }
      for (const key of await caches?.keys?.() ?? []) await caches.delete(key);
    } catch {}
    return 1;
  })()`);
  await send('Page.navigate', { url: mobileUrl });
  await sleep(4000);

  // 5) 断言手机端花名册。先切到「agent」侧栏视图（底部 Tab / 侧栏入口按文案找）。
  const teamOpened = await ev(
    `(() => {
       const b = document.querySelector('button[title="Team"]');
       if (!b) return 'no-team-tab';
       b.click();
       return 'clicked';
     })()`,
  );
  if (teamOpened !== 'clicked') fail('手机端找不到 Team 侧栏入口:', teamOpened);
  await sleep(1500);

  const ROSTER_PROBE = `(() => {
    const txt = document.body.innerText || '';
    const inputs = [...document.querySelectorAll('input, textarea')]
      .filter((x) => /发消息|message/i.test(x.placeholder || ''));
    return {
      url: location.href,
      isMobileSpa: !!document.querySelector('#app, .remote-root') || /remote/i.test(document.title),
      sawAider: /aider/.test(txt),
      sawStatus: /运行中|空闲|已暂停|失联/.test(txt),
      sawAuto: /自动/.test(txt),
      sawRecent: /最近回复/.test(txt),
      perMemberInputs: inputs.length,
      roster: document.querySelector('.roster')?.innerText ?? '(no .roster)',
      chunks: performance
        .getEntriesByType('resource')
        .map((r) => r.name.split('/').pop())
        .filter((n) => /RemoteSidebar|^index-/.test(n ?? '')),
      head: txt.slice(0, 400),
    };
  })()`;

  // 数据面直测：SPA 的 roster 拉取失败时只会显示一个「—」（catch 里静默），
  // 所以先在**同源页面里**用同样的 token/device 开一条 WS，把三条依赖命令逐个打一遍，
  // 谁挂了就直接报出错误文本——否则只能靠猜。
  const api = await ev(`(async () => {
    const tok = localStorage.getItem('ridge_remote_token');
    const dev = localStorage.getItem('ridge_remote_device');
    const url = 'wss://' + location.host + '/ws?token=' + encodeURIComponent(tok) + '&device=' + encodeURIComponent(dev);
    const s = new WebSocket(url);
    const out = {};
    try {
      await new Promise((res, rej) => {
        s.onopen = res;
        s.onerror = () => rej(new Error('ws open failed'));
        setTimeout(() => rej(new Error('ws open timeout')), 8000);
      });
      let n = 0;
      const call = (cmd) => new Promise((res) => {
        const reqId = ++n;
        const onMsg = (e) => {
          if (typeof e.data !== 'string') return;
          const m = JSON.parse(e.data);
          if (m.type === 'invoke-result' && m._reqId === reqId) {
            s.removeEventListener('message', onMsg);
            res(m._error ? { error: String(m._error) } : { ok: true, result: m._result });
          }
        };
        s.addEventListener('message', onMsg);
        s.send(JSON.stringify({ type: 'invoke-request', cmd, args: {}, _reqId: reqId }));
        setTimeout(() => res({ error: 'timeout' }), 8000);
      });
      for (const cmd of ['get_teammate_topology', 'list_hitl_pending', 'get_orchestration_health', 'detect_available_shells']) {
        const r = await call(cmd);
        out[cmd] = r.error ? 'ERR ' + r.error : 'ok(' + JSON.stringify(r.result).slice(0, 160) + ')';
      }
    } catch (e) { out._fatal = String(e); }
    try { s.close(); } catch {}
    return out;
  })()`);
  log('data-plane:', JSON.stringify(api, null, 1));

  let probe = null;
  for (let i = 0; i < 15; i++) {
    await sleep(2000);
    probe = await ev(ROSTER_PROBE);
    if (probe?.sawAider) break;
  }
  log('mobile probe:', JSON.stringify(probe));
  if (pageLogs.length) log('page logs:\n  ' + [...new Set(pageLogs)].slice(0, 12).join('\n  '));
  const lifecycleNoise = pageLogs.filter((message) =>
    /render worker (?:init request timed out|request timed out)|resize before init|Unchecked runtime\.lastError/i.test(
      message,
    ),
  );
  if (lifecycleNoise.length) {
    fail('mobile page emitted terminal lifecycle/runtime messaging noise:', [...new Set(lifecycleNoise)]);
  }
  if (!probe?.sawAider) {
    fail('手机端没有渲染出自动识别的 agent 成员');
  } else {
    if (!probe.sawStatus) fail('手机端成员缺工作状态');
    if (!probe.sawAuto) fail('手机端成员缺「自动」识别标记');
    if (!probe.sawRecent) fail('手机端成员缺「最近回复」区');
    if (!probe.perMemberInputs) fail('手机端成员缺各自的发消息输入框');
  }
  // ── iter-63：手机端「切换终端类型」入口（PS → WSL），列表须与桌面同源 ──
  const shellProbe = await ev(`(async () => {
    const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
    // 先关掉 Team 侧栏（它 position:fixed 盖满屏，留着会挡住底栏的树入口）。
    document.querySelector('button[title="Team"]')?.click();
    await sleep(300);
    const tree = document.querySelector('.tree-trigger');
    if (!tree) return { step: 'no-tree-trigger' };
    tree.click();
    await sleep(500);
    // 展开活动工作区的终端列表（折叠时不渲染 pane 行）。
    if (!document.querySelector('.pane-item')) {
      document.querySelector('[title="展开/折叠终端"], [title="Expand / collapse terminals"]')?.click();
      await sleep(600);
    }
    const pill = document.querySelector('.row-shell');
    if (!pill) {
      return { step: 'no-shell-pill', panes: document.querySelectorAll('.pane-item').length };
    }
    pill.click();
    // detect_available_shells 要枚举 WSL 发行版，可能好几秒 —— 轮询等，别一竿子定死。
    let menu = null;
    let items = [];
    for (let i = 0; i < 20; i++) {
      await sleep(700);
      menu = document.querySelector('.shell-menu');
      items = menu ? [...menu.querySelectorAll('.shell-item')].map((b) => b.textContent.trim()) : [];
      if (items.length) break;
    }
    if (!menu) return { step: 'no-shell-menu' };
    return {
      step: 'ok',
      items,
      note: menu.querySelector('.shell-err, .shell-note')?.textContent?.trim() ?? '',
    };
  })()`);
  log('shell picker:', JSON.stringify(shellProbe));
  if (shellProbe?.step !== 'ok') {
    fail('手机端切换终端类型入口不可用:', shellProbe?.step);
  } else if (!shellProbe.items?.length) {
    fail('终端类型列表为空（应与桌面 detect_available_shells 同源）:', shellProbe.note);
  } else {
    // 与本机桌面端同源：至少含 PowerShell；有 WSL 则一并列出。
    const hasPs = shellProbe.items.some((x) => /powershell/i.test(x));
    if (!hasPs) fail('终端类型列表与桌面端不一致（缺 PowerShell）:', shellProbe.items.join(' | '));
    else log('终端类型列表 =', shellProbe.items.join(' | '));

    // 真切一次（用户原话就是「把终端从 Ps 切到 Wsl」）：点 WSL 项，再从数据面读
    // 该 pane 的 scrollback，看是不是真换了个 shell 在跑。
    const target = shellProbe.items.find((x) => /^WSL:/.test(x)) ?? shellProbe.items.find((x) => /Git Bash/.test(x));
    if (!target) {
      log('本机无 WSL / Git Bash，跳过「真切换」断言');
    } else {
      // 判据取 pane 标题（list-panes 回传，DOM 可读）：终端画布是 WebGPU 渲染的，
      // 里头的提示符读不到；而 `get_pane_scrollback_tail` 不在 LAN invoke 名单里。
      const TITLE = `document.querySelector('.trigger-label')?.textContent?.trim() ?? ''`;
      const before = await ev(TITLE);
      const clicked = await ev(`(() => {
        const b = [...document.querySelectorAll('.shell-item')]
          .find((x) => x.textContent.trim() === ${JSON.stringify(target)});
        if (!b) return 'item-gone';
        b.click();
        return 'clicked';
      })()`);
      if (clicked !== 'clicked') fail('点选终端类型失败:', clicked);
      else {
        let after = before;
        for (let i = 0; i < 15; i++) {
          await sleep(2000);
          await ev(`document.querySelector('.tree-trigger') && 1`);
          after = await ev(TITLE);
          if (after && after !== before && !/powershell/i.test(after)) break;
        }
        if (!after || after === before || /powershell/i.test(after)) {
          const why = await ev(
            `document.querySelector('.shell-err')?.textContent?.trim()
             ?? document.querySelector('.shell-menu')?.innerText?.trim() ?? '(菜单已关闭)'`,
          );
          fail(
            `切换到「${target}」后终端没换：标题仍是 ${JSON.stringify(after)}；面板提示：${why}`,
          );
        } else {
          log('已切到', target, '| 新终端标题 =', after);
        }
      }

      // 切回 PowerShell：本测会把 pane 留在切换后的 shell 上，下一次跑时
      // `Start-Process …`（PS 语法）会喂给 bash，agent 起不来、整条链路假红。
      await ev(`(async () => {
        const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
        document.querySelector('.row-shell')?.click();
        for (let i = 0; i < 15; i++) {
          await sleep(600);
          const ps = [...document.querySelectorAll('.shell-item')].find((x) => /Windows PowerShell/i.test(x.textContent));
          if (ps) { ps.click(); return 'restored'; }
        }
        return 'give-up';
      })()`);
    }
  }
} catch (e) {
  fail(e.message);
}

await finish();
