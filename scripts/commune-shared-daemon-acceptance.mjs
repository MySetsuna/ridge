// Real installed Codex and Ridge processes; model responses come only from an
// owned loopback fixture. No user sessions, credentials, model service or WAN.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import net from 'node:net';
import http from 'node:http';
import { spawn, spawnSync } from 'node:child_process';
import { once } from 'node:events';
import { randomBytes, randomUUID } from 'node:crypto';
import { setTimeout as delay } from 'node:timers/promises';
import { WebSocket } from 'ws';

const root = fs.mkdtempSync(path.join(os.tmpdir(), 'ridge-shared-real-'));
const cwd = path.join(root, 'project'), home = path.join(root, 'codex-home'), data = path.join(root, 'runtime');
for (const directory of [cwd, home, data]) fs.mkdirSync(directory);
const target = path.resolve(process.env.CARGO_TARGET_DIR ?? 'target', 'debug');
const suffix = process.platform === 'win32' ? '.exe' : '';
for (const binary of ['ridge', 'ridge-kernel']) fs.copyFileSync(path.join(target, binary + suffix), path.join(root, binary + suffix));
const pathValue = Object.entries(process.env).find(([name]) => name.toLowerCase() === 'path')?.[1] ?? '';
const codex = process.env.RIDGE_CODEX_EXE ?? pathValue.split(path.delimiter).map(directory => path.join(directory, 'codex' + suffix)).find(file => fs.existsSync(file));
assert.ok(codex && fs.existsSync(codex), 'Install Codex or set RIDGE_CODEX_EXE for the required real shared daemon acceptance');
const token = randomBytes(32).toString('hex'), tokenFile = path.join(root, 'capability-token');
fs.writeFileSync(tokenFile, token, { mode: 0o600 });
const env = Object.fromEntries(Object.entries(process.env).filter(([name]) => !/^(CODEX_|OPENAI_)/i.test(name)));
Object.assign(env, { CODEX_HOME: home, NO_PROXY: '127.0.0.1,localhost', HTTP_PROXY: 'http://127.0.0.1:1', HTTPS_PROXY: 'http://127.0.0.1:1',
  RIDGE_KERNEL_DATA_DIR: data, RIDGE_KERNEL_FS_ROOT: root, RIDGE_WORKSPACE_SAVE_DIR: data, RIDGE_AUTH_FILE: path.join(data, 'auth.json') });
const children = [], sockets = [];
let endpoint;
const report = { scope: 'Real installed Codex shared daemon and Ridge CLI/Kernel; isolated home; local controlled model responses; no external model or WAN acceptance', checks: [], modelRequests: 0, grok: 'SKIPPED user-authorized waiver' };
function owned(executable, args, name) {
  const child = spawn(executable, args, { cwd, env, windowsHide: true, detached: process.platform !== 'win32', stdio: ['pipe', 'pipe', 'pipe'] });
  children.push(child); child.stdin.on('error', () => {});
  let output = '';
  for (const stream of [child.stdout, child.stderr]) stream.on('data', chunk => { output = (output + chunk).slice(-16384); });
  child.on('error', error => { output += `\n${name}: ${error.code}`; });
  child.output = () => output.replaceAll(token, '<redacted>');
  return child;
}
async function stop(child) {
  if (!child || child.exitCode !== null || child.signalCode !== null) return;
  const closed = once(child, 'close').catch(() => {});
  if (process.platform === 'win32') await new Promise(resolve => spawn('taskkill.exe', ['/F', '/T', '/PID', String(child.pid)], { windowsHide: true, stdio: 'ignore' }).once('close', resolve));
  else { try { process.kill(-child.pid, 'SIGKILL'); } catch {} }
  await Promise.race([closed, delay(2000)]);
}
async function until(read, label, timeout = 20000) {
  const deadline = Date.now() + timeout;
  do { const result = await read(); if (result) return result; await delay(40); } while (Date.now() < deadline);
  throw Error(label);
}
async function api(route, method = 'GET', value) {
  const response = await fetch(`http://127.0.0.1:${endpoint.port}${route}`, { method, headers: { 'x-ridge-kernel-token': endpoint.token, 'content-type': 'application/json' },
    body: value === undefined ? undefined : JSON.stringify(value), signal: AbortSignal.timeout(5000) });
  assert.equal(response.status, 200); return response.json();
}
async function command(method, args, native = true, required = true) {
  const result = await api('/v1/domain/commands', 'POST', { method, args, native, roots: [], connectionId: randomUUID() });
  if (required) { assert.equal(result.ok, true, `${method}: ${JSON.stringify(result)}`); return result.value; }
  return result;
}
async function observer(url) {
  const socket = new WebSocket(url, { headers: { Authorization: `Bearer ${token}` }, handshakeTimeout: 1000 });
  socket.on('error', () => {}); await once(socket, 'open'); sockets.push(socket);
  const pending = new Map(); let id = 0;
  socket.on('message', bytes => { const value = JSON.parse(bytes); const entry = pending.get(value.id); if (entry) {pending.delete(value.id); clearTimeout(entry.timer); value.error ? entry.reject(Error(JSON.stringify(value.error))) : entry.resolve(value.result);} });
  const call = (method, params = {}) => new Promise((resolve, reject) => {
    const request = ++id; const timer = setTimeout(() => {pending.delete(request); reject(Error(`${method} deadline`));}, 8000);
    pending.set(request, { resolve, reject, timer }); socket.send(JSON.stringify({ id: request, method, params }));
  });
  await call('initialize', { clientInfo: { name: 'ridge_unproven_acceptance', version: '1' }, capabilities: { experimentalApi: true } });
  socket.send(JSON.stringify({ method: 'initialized' }));
  return { call, socket };
}
function processInstance(pid) {
  if (process.platform === 'win32') {
    const result = spawnSync('powershell.exe', ['-NoProfile', '-Command', `(Get-Process -Id ${pid}).StartTime.ToFileTimeUtc().ToString('x16')`], { encoding: 'utf8', windowsHide: true });
    assert.equal(result.status, 0); return { pid, startInstance: `win:${result.stdout.trim()}` };
  }
  assert.equal(process.platform, 'linux', 'OS attestation requires Windows or Linux');
  const stat = fs.readFileSync(`/proc/${pid}/stat`, 'utf8'); const fields = stat.slice(stat.lastIndexOf(')') + 1).trim().split(/\s+/);
  return { pid, startInstance: `linux:${fs.readFileSync('/proc/sys/kernel/random/boot_id', 'utf8').trim()}:${BigInt(fields[19]).toString(16).padStart(16, '0')}` };
}
const model = http.createServer(async (request, response) => {
  if (request.method !== 'POST' || !request.url.startsWith('/v1/responses')) {response.writeHead(404); response.end(); return;}
  const chunks = []; for await (const chunk of request) chunks.push(chunk);
  const incoming = JSON.parse(Buffer.concat(chunks));
  const input = JSON.stringify(incoming.input);
  const marker = [...input.matchAll(/RIDGE_SHARED_[A-Z0-9_]+/g)].at(-1)?.[0];
  if (!marker) {report.fixtureError = 'unexpected prompt without an owned marker'; response.writeHead(400); response.end(); return;}
  report.modelRequests++;
  const id = `resp_${randomUUID()}`, messageId = `msg_${randomUUID()}`;
  const part = { type: 'output_text', text: marker, annotations: [], logprobs: [] };
  const item = { type: 'message', id: messageId, status: 'completed', role: 'assistant', content: [part] };
  const base = { id, object: 'response', created_at: Math.floor(Date.now() / 1000), model: incoming.model, output: [] };
  const events = [
    { type: 'response.created', response: { ...base, status: 'in_progress' } },
    { type: 'response.output_item.added', output_index: 0, item: { ...item, status: 'in_progress', content: [] } },
    { type: 'response.content_part.added', item_id: messageId, output_index: 0, content_index: 0, part: { ...part, text: '' } },
    { type: 'response.output_text.delta', item_id: messageId, output_index: 0, content_index: 0, delta: marker },
    { type: 'response.output_text.done', item_id: messageId, output_index: 0, content_index: 0, text: marker },
    { type: 'response.content_part.done', item_id: messageId, output_index: 0, content_index: 0, part },
    { type: 'response.output_item.done', output_index: 0, item },
    { type: 'response.completed', response: { ...base, status: 'completed', output: [item], usage: { input_tokens: 10, output_tokens: 5, total_tokens: 15 } } },
  ];
  response.writeHead(200, { 'content-type': 'text/event-stream' });
  for (const event of events) response.write(`event: ${event.type}\ndata: ${JSON.stringify(event)}\n\n`);
  response.end();
});
await new Promise(resolve => model.listen(0, '127.0.0.1', resolve));
fs.writeFileSync(path.join(home, 'config.toml'), `model = "gpt-5-codex"\nmodel_provider = "ridge_fixture"\ncheck_for_update_on_startup = false\n[model_providers.ridge_fixture]\nname = "Owned loopback fixture"\nbase_url = "http://127.0.0.1:${model.address().port}/v1"\nwire_api = "responses"\nrequires_openai_auth = false\nsupports_websockets = false\n`);
const free = net.createServer(); free.listen(0, '127.0.0.1'); await once(free, 'listening'); const port = free.address().port; await new Promise(resolve => free.close(resolve));
const serviceUrl = `ws://127.0.0.1:${port}/`;
let daemon;
try {
  daemon = owned(codex, ['app-server', '--listen', serviceUrl.slice(0, -1), '--ws-auth', 'capability-token', '--ws-token-file', tokenFile], 'Codex daemon');
  const third = await until(async () => { assert.equal(daemon.exitCode, null, daemon.output()); try {return await observer(serviceUrl);} catch {return null;} }, 'real Codex daemon unavailable');
  report.daemonPid = daemon.pid;
  const kernel = owned(path.join(root, 'ridge-kernel' + suffix), [], 'Ridge Kernel');
  endpoint = await until(async () => {assert.equal(kernel.exitCode, null, kernel.output()); try { const value = JSON.parse(fs.readFileSync(path.join(data, 'kernel.json'))); return value.pid === kernel.pid && value; } catch {return null;} }, 'isolated Runtime unavailable');
  const clients = [];
  for (const label of ['A', 'B']) {
    const before = new Set((await api('/v1/domain/ptys')).ptys.map(pane => pane.pty_id));
    const viewer = owned(path.join(root, 'ridge' + suffix), ['agent', '--connect', serviceUrl, '--protocol', 'app-server', '--bearer-token-file', tokenFile, '--cwd', cwd], `Ridge client ${label}`);
    const pane = await until(async () => {assert.equal(viewer.exitCode, null, viewer.output()); return (await api('/v1/domain/ptys')).ptys.find(pane => !before.has(pane.pty_id));}, 'CLI did not create an independent terminal');
    const candidate = await until(async () => {
      assert.equal(viewer.exitCode, null, viewer.output());
      return (await command('get_teammate_topology', { workspaceId: pane.workspace_id })).discovered.find(row => row.paneId === pane.pty_id && row.connectionReady);
    }, `shared client ${label} did not publish its actual session`);
    clients.push({ label, viewer, pane, candidate, source: { workspaceId: pane.workspace_id, paneId: pane.pty_id, candidateRef: candidate.candidateRef } });
  }
  assert.notEqual(clients[0].pane.child_pid, clients[1].pane.child_pid);
  await command('set_teammate_groups', { groupScope: 'host', expectedRevision: 0, groups: [{ id: 'shared', name: 'Shared service clients', color: '#60a5fa', memberAgentIds: [], createdAt: 1 }], enrollments: clients.map(client => ({ candidateRef: client.candidate.candidateRef, groupId: 'shared' })) });
  const groupBefore = fs.readFileSync(path.join(data, 'commune-groups.json'));
  for (const client of clients) {
    const preview = await command('prepare_commune_connection', client.source);
    assert.equal(preview.bearerTokenFile, undefined);
    client.identity = await command('register_commune_member', { ...client.source, sessionRef: preview.sessionRef });
    assert.equal(client.identity.sessionId, preview.sessionId);
    assert.equal(client.identity.agentId, (await command('get_teammate_topology', { groupScope: 'host' })).members.find(member => member.paneId === client.pane.pty_id).agentId);
    client.target = { workspaceId: client.pane.workspace_id, paneId: client.pane.pty_id, agentId: client.identity.agentId, generation: client.identity.generation, lease: client.identity.lease };
  }
  assert.notEqual(clients[0].identity.sessionId, clients[1].identity.sessionId);
  assert.deepEqual(fs.readFileSync(path.join(data, 'commune-groups.json')), groupBefore);
  const loaded = (await third.call('thread/loaded/list')).data;
  assert.deepEqual(new Set(loaded), new Set(clients.map(client => client.identity.sessionId)));
  for (const client of clients) assert.equal((await third.call('thread/read', { threadId: client.identity.sessionId })).thread.id, client.identity.sessionId);
  const launch = await command('get_collaboration_launch_options', {});
  const ordinary = await command('create_independent_terminal', { workspaceId: randomUUID(), paneId: randomUUID(), runtimeEpoch: launch.runtimeEpoch, entry: 'tui', cwd });
  const ordinaryPane = (await api('/v1/domain/ptys')).ptys.find(pane => pane.pty_id === ordinary.paneId);
  const unproven = await command('register_commune_member', { ...ordinary, process: { pid: ordinaryPane.child_pid, startInstance: ordinaryPane.child_start_instance }, endpointOwner: processInstance(daemon.pid), endpoint: serviceUrl, protocol: 'app-server', sessionId: clients[0].identity.sessionId, cwd, bearerTokenFile: tokenFile }, true, false);
  assert.equal(unproven.ok, false); assert.match(unproven.error.message ?? unproven.error, /process tree|does not own/);
  report.checks.push('Two same-CWD clients bind distinct existing sessions and retain terminal-first agentId/groups; an unproven third terminal is denied despite successful protocol listing/read');
  const badFile = path.join(root, 'wrong-token'); fs.writeFileSync(badFile, 'wrong-controlled-token');
  const deniedViewer = owned(path.join(root, 'ridge' + suffix), ['agent', '--connect', serviceUrl, '--protocol', 'app-server', '--bearer-token-file', badFile, '--cwd', cwd], 'Rejected client');
  await until(() => deniedViewer.exitCode !== null, 'authentication refusal did not terminate its client');
  assert.match(deniedViewer.output(), /authentication/); assert.ok(!deniedViewer.output().includes(token));
  assert.deepEqual(new Set((await third.call('thread/loaded/list')).data), new Set(loaded));
  report.checks.push('Bearer refusal creates no protocol session, performs no implicit recovery and does not expose credentials');
  async function dispatch(client, marker) {
    const request = { ...client.target, message: `Return ${marker} only.`, idempotency_key: randomUUID() };
    const delivery = await command('send_agent_message', request);
    const repeated = await command('send_agent_message', request); assert.equal(repeated.deliveryId, delivery.deliveryId);
    const execution = await until(async () => {
      const status = await command('get_agent_message_delivery_status', { ...client.target, deliveryId: delivery.deliveryId });
      if (['unknown', 'rejected', 'cancelled'].includes(status.execution?.state)) throw Error(`real shared dispatch failed: ${JSON.stringify(status.execution)}; ${daemon.output()}`);
      return status.execution?.state === 'turn_ended' && status.execution;
    }, `real shared turn ${client.label} did not complete`, 30000);
    assert.equal(execution.response.stopReason, 'end_turn');
    const history = JSON.stringify(await command('read_agent_recent_replies', { workspaceId: client.pane.workspace_id, limit: 100 }));
    assert.ok(history.includes(marker) && history.includes(client.identity.sessionId));
    return history;
  }
  const aHistory = await dispatch(clients[0], 'RIDGE_SHARED_CLIENT_A');
  const bHistory = await dispatch(clients[1], 'RIDGE_SHARED_CLIENT_B');
  await third.call('thread/resume', { threadId: clients[0].identity.sessionId, excludeTurns: true });
  assert.equal((await command('register_commune_member', { ...ordinary, process: { pid: ordinaryPane.child_pid, startInstance: ordinaryPane.child_start_instance }, endpointOwner: processInstance(daemon.pid), endpoint: serviceUrl, protocol: 'app-server', sessionId: clients[0].identity.sessionId, cwd, bearerTokenFile: tokenFile }, true, false)).ok, false);
  assert.ok(!aHistory.includes('RIDGE_SHARED_CLIENT_B')); assert.ok(!bHistory.includes('RIDGE_SHARED_CLIENT_A'));
  assert.equal(report.modelRequests, 2, 'exact retry sends each real turn once');
  const originalA = clients[0].identity;
  assert.equal((await command('release_teammate_agent', clients[0].target)).removed, true);
  clients[0].identity = await command('register_commune_member', clients[0].source);
  assert.equal(clients[0].identity.agentId, originalA.agentId); assert.equal(clients[0].identity.sessionId, originalA.sessionId);
  clients[0].target = { ...clients[0].target, generation: clients[0].identity.generation, lease: clients[0].identity.lease };
  assert.ok(JSON.stringify(await command('read_agent_recent_replies', { workspaceId: clients[0].pane.workspace_id, limit: 100 })).includes('RIDGE_SHARED_CLIENT_A'));
  assert.deepEqual(fs.readFileSync(path.join(data, 'commune-groups.json')), groupBefore);
  for (const client of clients) assert.equal((await api('/v1/domain/ptys')).ptys.find(pane => pane.pty_id === client.pane.pty_id).child_pid, client.pane.child_pid);
  report.checks.push('Real Codex turns dispatch once into each original session; observed history stays scoped to that session and original terminal PIDs remain unchanged');
  clients[0].viewer.stdin.write('/exit\n'); await until(() => clients[0].viewer.exitCode !== null, 'explicit client exit did not finish');
  assert.equal(daemon.exitCode, null); await dispatch(clients[1], 'RIDGE_SHARED_CLIENT_B_AFTER_A_EXIT');
  assert.deepEqual(fs.readFileSync(path.join(data, 'commune-groups.json')), groupBefore);
  await stop(daemon);
  await until(async () => (await command('get_teammate_topology', { groupScope: 'host' })).members.find(member => member.agentId === clients[1].identity.agentId)?.protocolConnected === false, 'daemon loss did not revoke member capability');
  assert.equal((await command('send_agent_message', { ...clients[1].target, message: 'must not resend', idempotency_key: randomUUID() }, true, false)).ok, false);
  assert.equal(report.modelRequests, 3); assert.deepEqual(fs.readFileSync(path.join(data, 'commune-groups.json')), groupBefore);
  report.checks.push('Exiting one client preserves the daemon and other client; daemon exit revokes old control without resends or group mutation');
  const previousDaemonPid = daemon.pid;
  daemon = owned(codex, ['app-server', '--listen', serviceUrl.slice(0, -1), '--ws-auth', 'capability-token', '--ws-token-file', tokenFile], 'Replacement Codex daemon');
  const replacement = await until(async () => {assert.equal(daemon.exitCode, null, daemon.output()); try {return await observer(serviceUrl);} catch {return null;} }, 'replacement daemon unavailable');
  assert.notEqual(daemon.pid, previousDaemonPid);
  assert.deepEqual((await replacement.call('thread/loaded/list')).data, [], 'replacement must not inherit a client binding');
  assert.equal((await command('send_agent_message', { ...clients[1].target, message: 'old binding must not reach replacement', idempotency_key: randomUUID() }, true, false)).ok, false);
  assert.equal(report.modelRequests, 3);
  const beforeReplacement = new Set((await api('/v1/domain/ptys')).ptys.map(pane => pane.pty_id));
  const viewer = owned(path.join(root, 'ridge' + suffix), ['agent', '--connect', serviceUrl, '--protocol', 'app-server', '--bearer-token-file', tokenFile, '--cwd', cwd], 'Explicit replacement client');
  const pane = await until(async () => {assert.equal(viewer.exitCode, null, viewer.output()); return (await api('/v1/domain/ptys')).ptys.find(pane => !beforeReplacement.has(pane.pty_id));}, 'replacement terminal absent');
  const candidate = await until(async () => (await command('get_teammate_topology', {workspaceId:pane.workspace_id})).discovered.find(row => row.connectionReady), 'replacement client binding absent');
  const source = {workspaceId:pane.workspace_id,paneId:pane.pty_id,candidateRef:candidate.candidateRef};
  const identity = await command('register_commune_member', source);
  assert.ok(clients.every(client => client.identity.sessionId !== identity.sessionId && client.identity.agentId !== identity.agentId));
  const fresh = {label:'C',viewer,pane,candidate,source,identity,target:{workspaceId:pane.workspace_id,paneId:pane.pty_id,agentId:identity.agentId,generation:identity.generation,lease:identity.lease}};
  await dispatch(fresh, 'RIDGE_SHARED_CLIENT_C'); clients.push(fresh);
  assert.equal(report.modelRequests, 4); assert.deepEqual(fs.readFileSync(path.join(data, 'commune-groups.json')), groupBefore);
  report.checks.push('Same-session reconnect retains agentId/groups/history; a different daemon at the same endpoint inherits no old lease or session and works only through an explicit fresh client binding');
  report.replacementDaemonPid = daemon.pid;
  report.sessions = clients.map(client => ({ label: client.label, sessionId: client.identity.sessionId, agentId: client.identity.agentId, clientPid: client.pane.child_pid }));
  report.status = 'PASS';
} catch (error) {
  report.status = 'FAIL'; report.error = String(error.stack ?? error).replaceAll(token, '<redacted>'); process.exitCode = 1;
} finally {
  for (const socket of sockets) socket.terminate();
  if (endpoint) await api('/v1/shutdown', 'POST').catch(() => {});
  for (const child of children.reverse()) await stop(child);
  model.closeAllConnections(); await new Promise(resolve => model.close(resolve));
  fs.rmSync(tokenFile, { force: true }); fs.rmSync(path.join(root, 'wrong-token'), { force: true });
  const evidence = path.resolve(process.env.RIDGE_COMMUNE_EVIDENCE_DIR ?? 'artifacts/iteration-followup'); fs.mkdirSync(evidence, { recursive: true });
  fs.writeFileSync(path.join(evidence, 'shared-daemon-real.json'), JSON.stringify(report, null, 2) + '\n');
  console.log(JSON.stringify(report, null, 2));
}
