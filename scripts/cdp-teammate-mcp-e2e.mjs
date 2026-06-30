#!/usr/bin/env node
// Domain C MCP「自由交流」端到端验证 —— P1（缺口1 寻址自洽 + 缺口3 get_team_profile 路由）。
//
// 直接以一个外部 MCP 客户端身份连内置 teammate MCP WebSocket，跑完整握手并断言：
//   - initialize / tools/list：ridge_get_team_profile 与 ridge_send_to_teammate 已广告
//   - tools/call(ridge_get_team_profile)：返回 roster；非空时每个成员同时带 paneId+paneIndex
//   - tools/call(ridge_send_to_teammate, target=<数字索引>)：delivered，且目标 pane 抓到注入文本
//   - tools/call(ridge_send_to_teammate, target=<Uuid 字符串>)：delivered，且目标 pane 抓到注入文本
//       ↑ 这两条共同证明缺口1：花名册回传的 paneIndex(数字) 与 paneId(Uuid) 两键落到同一 pane
//   - tools/call(ridge_send_to_teammate, target=<伪造 Uuid>)：JSON-RPC error（INVALID_PARAMS）
//
// 端点+token 发现（无需 CDP）：
//   1) 环境变量 RIDGE_TEAMMATE_URL + RIDGE_TEAMMATE_TOKEN（在任一 teammate 分屏里
//      `echo $RIDGE_TEAMMATE_URL` / `echo $RIDGE_TEAMMATE_TOKEN` 拷出，或 P2「复制连接信息」按钮）；
//   2) 否则扫 os.tmpdir() 的 ridge-teammate-endpoint-*.json sidecar（teammate 分屏 spawn 后自动写）。
//
// 前置（人工，rebuild 后）：
//   - 已 rebuild 并重启 ridge（本脚本不杀会话、不 rebuild）；
//   - 当前活动工作区至少有 2 个终端分屏（普通 shell 即可；脚本会向「目标」分屏注入可见标记）；
//   - 注入的标记会作为一行命令回显到目标 shell（多半 "command not found"），不影响断言。
//
// 用法：
//   node scripts/cdp-teammate-mcp-e2e.mjs                 # 自动发现端点；目标默认 pane 索引 1
//   node scripts/cdp-teammate-mcp-e2e.mjs --target 2      # 指定目标 pane 索引
//   RIDGE_TEAMMATE_URL=http://127.0.0.1:PORT RIDGE_TEAMMATE_TOKEN=... node scripts/cdp-teammate-mcp-e2e.mjs
//
// 退出码：全部断言通过=0，否则=1。

import http from 'node:http';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import crypto from 'node:crypto';

const log = (...a) => console.log('[mcp-e2e]', ...a);
let pass = 0;
let fail = 0;
function check(name, cond, detail) {
  if (cond) {
    pass++;
    log(`✅ ${name}`);
  } else {
    fail++;
    log(`❌ ${name} — ${detail ?? ''}`);
  }
}

function arg(flag, fallback) {
  const i = process.argv.indexOf(flag);
  return i >= 0 && process.argv[i + 1] ? process.argv[i + 1] : fallback;
}

// ── 端点 + token 发现 ────────────────────────────────────────────────────────
function discoverEndpoint() {
  const envUrl = process.env.RIDGE_TEAMMATE_URL;
  const envTok = process.env.RIDGE_TEAMMATE_TOKEN;
  if (envUrl && envTok) {
    return { url: envUrl.trim(), token: envTok.trim(), via: 'env' };
  }
  const dir = os.tmpdir();
  let best = null;
  for (const name of fs.readdirSync(dir)) {
    if (!/^ridge-teammate-endpoint-.*\.json$/.test(name)) continue;
    const full = path.join(dir, name);
    try {
      const st = fs.statSync(full);
      if (!best || st.mtimeMs > best.mtimeMs) {
        const body = JSON.parse(fs.readFileSync(full, 'utf8'));
        if (body.url && body.token) best = { ...body, mtimeMs: st.mtimeMs, file: full };
      }
    } catch {
      /* ignore unreadable sidecar */
    }
  }
  if (best) return { url: best.url.trim(), token: best.token.trim(), via: `sidecar ${best.file}` };
  throw new Error(
    'no endpoint: set RIDGE_TEAMMATE_URL + RIDGE_TEAMMATE_TOKEN, or open a teammate split so a sidecar is written to ' +
      dir,
  );
}

// ── HTTP（带 token 头）────────────────────────────────────────────────────────
function httpReq(base, method, p, { token, body } = {}) {
  const u = new URL(p, base);
  const payload = body != null ? Buffer.from(JSON.stringify(body)) : null;
  const headers = { 'x-ridge-token': token };
  if (payload) {
    headers['content-type'] = 'application/json';
    headers['content-length'] = String(payload.length);
  }
  return new Promise((resolve, reject) => {
    const req = http.request(
      { hostname: u.hostname, port: u.port, path: u.pathname + u.search, method, headers, timeout: 5000 },
      (res) => {
        let b = '';
        res.on('data', (c) => (b += c));
        res.on('end', () => resolve({ status: res.statusCode, text: b }));
      },
    );
    req.on('error', reject);
    req.on('timeout', () => req.destroy(new Error('http timeout')));
    if (payload) req.write(payload);
    req.end();
  });
}

// ── 依赖零的 WebSocket 客户端（支持自定义 header，用于带 x-ridge-token）──────────
// 浏览器/undici 的全局 WebSocket 不能设 header；MCP 鉴权只认 header → 手写最小客户端。
class MiniWs {
  constructor(base, p, token) {
    this.url = new URL(p, base.replace(/^http/, 'http')); // base 是 http://，WS 走同主机
    this.token = token;
    this.sock = null;
    this.buf = Buffer.alloc(0);
    this.queue = []; // 已解析出的完整文本帧
    this.waiters = []; // 等待文本帧的 resolver
  }
  connect() {
    const key = crypto.randomBytes(16).toString('base64');
    return new Promise((resolve, reject) => {
      const req = http.request({
        hostname: this.url.hostname,
        port: this.url.port,
        path: this.url.pathname + this.url.search,
        method: 'GET',
        headers: {
          'x-ridge-token': this.token,
          Connection: 'Upgrade',
          Upgrade: 'websocket',
          'Sec-WebSocket-Key': key,
          'Sec-WebSocket-Version': '13',
        },
      });
      req.on('upgrade', (_res, socket) => {
        this.sock = socket;
        socket.on('data', (d) => this._onData(d));
        socket.on('close', () => this._flushClose());
        socket.on('error', () => this._flushClose());
        resolve();
      });
      req.on('response', (res) => reject(new Error(`ws upgrade rejected: HTTP ${res.statusCode}`)));
      req.on('error', reject);
      req.end();
    });
  }
  _onData(d) {
    this.buf = Buffer.concat([this.buf, d]);
    // 解析尽可能多的完整帧（服务端→客户端不掩码）
    while (this.buf.length >= 2) {
      const b0 = this.buf[0];
      const b1 = this.buf[1];
      const opcode = b0 & 0x0f;
      const masked = (b1 & 0x80) !== 0;
      let len = b1 & 0x7f;
      let off = 2;
      if (len === 126) {
        if (this.buf.length < off + 2) return;
        len = this.buf.readUInt16BE(off);
        off += 2;
      } else if (len === 127) {
        if (this.buf.length < off + 8) return;
        len = Number(this.buf.readBigUInt64BE(off));
        off += 8;
      }
      const maskKey = masked ? this.buf.subarray(off, off + 4) : null;
      if (masked) off += 4;
      if (this.buf.length < off + len) return; // 帧未到齐
      let payload = this.buf.subarray(off, off + len);
      if (masked && maskKey) {
        const out = Buffer.allocUnsafe(len);
        for (let i = 0; i < len; i++) out[i] = payload[i] ^ maskKey[i & 3];
        payload = out;
      }
      this.buf = this.buf.subarray(off + len);
      if (opcode === 0x1) {
        const txt = payload.toString('utf8');
        const w = this.waiters.shift();
        if (w) w(txt);
        else this.queue.push(txt);
      } else if (opcode === 0x9) {
        this._send(payload, 0xa); // ping → pong
      } else if (opcode === 0x8) {
        this._flushClose();
      }
    }
  }
  _flushClose() {
    while (this.waiters.length) this.waiters.shift()(null);
  }
  _send(payloadBuf, opcode) {
    const len = payloadBuf.length;
    const maskKey = crypto.randomBytes(4);
    let header;
    if (len < 126) {
      header = Buffer.from([0x80 | opcode, 0x80 | len]);
    } else if (len < 65536) {
      header = Buffer.alloc(4);
      header[0] = 0x80 | opcode;
      header[1] = 0x80 | 126;
      header.writeUInt16BE(len, 2);
    } else {
      header = Buffer.alloc(10);
      header[0] = 0x80 | opcode;
      header[1] = 0x80 | 127;
      header.writeBigUInt64BE(BigInt(len), 2);
    }
    const masked = Buffer.allocUnsafe(len);
    for (let i = 0; i < len; i++) masked[i] = payloadBuf[i] ^ maskKey[i & 3];
    this.sock.write(Buffer.concat([header, maskKey, masked]));
  }
  sendText(txt) {
    this._send(Buffer.from(txt, 'utf8'), 0x1);
  }
  nextText(timeoutMs = 5000) {
    if (this.queue.length) return Promise.resolve(this.queue.shift());
    return new Promise((resolve, reject) => {
      const t = setTimeout(() => reject(new Error('ws frame timeout')), timeoutMs);
      this.waiters.push((v) => {
        clearTimeout(t);
        if (v == null) reject(new Error('ws closed before frame'));
        else resolve(v);
      });
    });
  }
  close() {
    try {
      this.sock?.end();
    } catch {
      /* noop */
    }
  }
}

let rpcId = 0;
async function rpc(ws, method, params) {
  const id = ++rpcId;
  ws.sendText(JSON.stringify({ jsonrpc: '2.0', id, method, params }));
  // 服务端逐条处理逐条回复（mcp_socket 单 recv→单 send）→ 取下一帧即本请求响应。
  const txt = await ws.nextText();
  const msg = JSON.parse(txt);
  return msg;
}

(async () => {
  const ep = discoverEndpoint();
  log(`endpoint: ${ep.url} (via ${ep.via})`);

  // 1) 枚举 pane（拿真实 Uuid）
  const lp = await httpReq(ep.url, 'GET', '/api/v1/list-panes?json=1', { token: ep.token });
  if (lp.status !== 200) throw new Error(`list-panes failed: HTTP ${lp.status} ${lp.text}`);
  const panes = JSON.parse(lp.text).panes ?? [];
  log(`panes: ${panes.map((p) => `${p.index}:${p.uuid.slice(0, 8)}`).join(' ')}`);
  const targetIndex = Number(arg('--target', '1'));
  const target = panes.find((p) => p.index === targetIndex);
  if (!target) {
    throw new Error(
      `target pane index ${targetIndex} not found (have ${panes.length} panes). Open at least ${targetIndex + 1} splits in the active workspace.`,
    );
  }
  log(`target: index=${target.index} uuid=${target.uuid}`);

  // 2) MCP WS 握手
  const ws = new MiniWs(ep.url, '/api/v1/mcp/ws', ep.token);
  await ws.connect();
  log('mcp ws connected');

  // 3) initialize
  const init = await rpc(ws, 'initialize', {});
  check('initialize → serverInfo.name=ridge-teammate', init?.result?.serverInfo?.name === 'ridge-teammate', JSON.stringify(init));

  // 4) tools/list 广告了被路由的工具
  const tl = await rpc(ws, 'tools/list', {});
  const toolNames = (tl?.result?.tools ?? []).map((t) => t.name);
  check('tools/list 含 ridge_get_team_profile', toolNames.includes('ridge_get_team_profile'), JSON.stringify(toolNames));
  check('tools/list 含 ridge_send_to_teammate', toolNames.includes('ridge_send_to_teammate'), JSON.stringify(toolNames));

  // 5) tools/call(ridge_get_team_profile)（缺口3）—— 之前会 "unknown tool"
  const gp = await rpc(ws, 'tools/call', { name: 'ridge_get_team_profile', arguments: {} });
  const gpText = gp?.result?.content?.[0]?.text;
  check('get_team_profile 已路由（非 unknown tool）', typeof gpText === 'string' && !gp?.error, JSON.stringify(gp));
  if (typeof gpText === 'string') {
    let roster = [];
    try {
      roster = JSON.parse(gpText).roster ?? [];
    } catch {
      /* leave empty */
    }
    log(`roster size=${roster.length}`);
    if (roster.length > 0) {
      const allHaveBoth = roster.every(
        (m) => typeof m.paneId === 'string' && m.paneId.length > 0 && Number.isInteger(m.paneIndex),
      );
      check('roster 每个成员同时带 paneId(Uuid) 与 paneIndex(数字)', allHaveBoth, JSON.stringify(roster));
    } else {
      log('   roster 为空（无已注册 teammate agent）→ 跳过 paneId/paneIndex 字段断言（寻址仍由下方索引/Uuid 直投验证）');
    }
  }

  // 6) 缺口1 寻址自洽：数字索引 与 Uuid 两键各发一条可见标记，capture 验证落到同一 pane
  const ts = Date.now();
  const markIdx = `RIDGE_MCP_E2E_IDX_${ts}`;
  const markUuid = `RIDGE_MCP_E2E_UUID_${ts}`;

  const sendIdx = await rpc(ws, 'tools/call', {
    name: 'ridge_send_to_teammate',
    arguments: { target_pane_id: target.index, message: markIdx },
  });
  check('send 经数字索引 → delivered', sendIdx?.result?.content?.[0]?.text === 'delivered', JSON.stringify(sendIdx));

  const sendUuid = await rpc(ws, 'tools/call', {
    name: 'ridge_send_to_teammate',
    arguments: { target_pane_id: target.uuid, message: markUuid },
  });
  check('send 经 Uuid 字符串 → delivered', sendUuid?.result?.content?.[0]?.text === 'delivered', JSON.stringify(sendUuid));

  // 7) 伪造 Uuid → INVALID_PARAMS error（不静默落 0 号 pane）
  const bogus = crypto.randomUUID();
  const sendBogus = await rpc(ws, 'tools/call', {
    name: 'ridge_send_to_teammate',
    arguments: { target_pane_id: bogus, message: 'should-not-deliver' },
  });
  check('send 经非法/陈旧 Uuid → JSON-RPC error', !!sendBogus?.error, JSON.stringify(sendBogus));

  // 8) 抓取目标 pane，断言两条标记都注入到了同一 pane
  await new Promise((r) => setTimeout(r, 600)); // 等 PTY 回显落盘
  const cap = await httpReq(ep.url, 'GET', `/api/v1/capture-pane?pane=${target.index}&lines=200`, { token: ep.token });
  const captured = cap.text ?? '';
  check('目标 pane 抓到「数字索引」注入文本', captured.includes(markIdx), `captured tail lacks ${markIdx}`);
  check('目标 pane 抓到「Uuid」注入文本', captured.includes(markUuid), `captured tail lacks ${markUuid}`);

  ws.close();
  console.log(`\n==== MCP E2E: ${pass} passed, ${fail} failed ====`);
  process.exit(fail === 0 ? 0 : 1);
})().catch((e) => {
  console.error('[mcp-e2e] FAIL:', e.message);
  process.exit(1);
});
