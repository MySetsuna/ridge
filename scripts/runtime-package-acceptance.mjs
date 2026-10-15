import assert from 'node:assert/strict';
import { execFile, spawn } from 'node:child_process';
import { promisify } from 'node:util';
import { mkdtemp, mkdir, readFile, writeFile, readdir } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { createHash, randomUUID } from 'node:crypto';

const execute = promisify(execFile);
const expected = JSON.parse(await readFile('src-tauri/tauri.conf.json', 'utf8')).version;
const installer = resolve(process.argv[2] || `release/ridge_${expected}_x64-setup.exe`);
const root = await mkdtemp(join(tmpdir(), 'ridge-package-acceptance-'));
const extracted = join(root, 'extracted');
await mkdir(extracted);
const report = { installer, root, completeChange: false, scope: 'extracted release payload; installer hooks and native GUI not exercised', checks: [] };
const hash = async path => createHash('sha256').update(await readFile(path)).digest('hex');
const delay = ms => new Promise(done => setTimeout(done, ms));
async function until(check, message) {
  const deadline = Date.now() + 30000;
  while (Date.now() < deadline) { if (await check()) return; await delay(100); }
  throw new Error(message);
}
async function files(directory) {
  const result = [];
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    const path = join(directory, entry.name);
    if (entry.isDirectory()) result.push(...await files(path));
    else result.push(path);
  }
  return result;
}
let child, endpoint;
async function request(path, body, method = 'POST') {
  const response = await fetch(`http://127.0.0.1:${endpoint.port}${path}`, {
    method, headers: { 'x-ridge-kernel-token': endpoint.token, 'content-type': 'application/json' },
    body: body === undefined ? undefined : JSON.stringify(body), signal: AbortSignal.timeout(10000),
  });
  assert.equal(response.status, 200);
  return response.json();
}
async function stop() {
  if (!child || child.exitCode !== null || child.signalCode !== null) return;
  try { await request('/v1/shutdown', {}); } catch {}
  try { await until(() => child.exitCode !== null || child.signalCode !== null, 'owned packaged Runtime did not stop'); }
  catch (error) { child.kill(); throw error; }
}
try {
  report.installerSha256 = await hash(installer);
  await execute('7z', ['x', installer, `-o${extracted}`, '-y'], { windowsHide: true, maxBuffer: 4 * 1024 * 1024 });
  const payload = await files(extracted);
  const kernels = payload.filter(path => /[\\/]ridge-kernel\.exe$/i.test(path));
  assert.equal(kernels.length, 1, 'installer must contain exactly one independent Runtime');
  assert.ok(payload.some(path => /[\\/]ridge\.exe$/i.test(path)), 'desktop payload missing');
  const binary = kernels[0];
  assert.equal((await execute(binary, ['--version'], { windowsHide: true })).stdout.trim(), `ridge-kernel ${expected}`);
  report.runtimeSha256 = await hash(binary);
  assert.equal(report.runtimeSha256, await hash('target/runtime-binaries/ridge-kernel-x86_64-pc-windows-msvc.exe'));
  report.checks.push('installer carries the exact version-verified independent Runtime');
  const data = join(root, 'data'); await mkdir(data);
  const env = { ...process.env, RIDGE_KERNEL_DATA_DIR: data, RIDGE_KERNEL_FS_ROOT: root,
    RIDGE_AUTH_FILE: join(root, 'auth.json'), RIDGE_WORKSPACE_SAVE_DIR: join(root, 'snapshots'), RIDGE_TEST_TLS_DIR: join(root, 'tls') };
  delete env.RIDGE_KERNEL_HOST_BINARY;
  async function start() {
    child = spawn(binary, [], { env, windowsHide: true, stdio: 'ignore' });
    await until(async () => {
      assert.equal(child.exitCode, null, 'packaged Runtime exited during startup');
      try { endpoint = JSON.parse(await readFile(join(data, 'kernel.json'), 'utf8')); return endpoint.pid === child.pid; }
      catch { return false; }
    }, 'packaged Runtime did not publish its own endpoint');
    const health = await request('/v1/health', undefined, 'GET');
    assert.equal(health.pid, child.pid); assert.equal(health.role, 'ridge-kernel');
  }
  await start();
  const command = async (method, args) => {
    const result = await request('/v1/domain/commands', { method, args, native: true, roots: [], connectionId: randomUUID() });
    assert.equal(result.ok, true, JSON.stringify(result.error)); return result.value;
  };
  const project = await command('open_project', { path: root });
  const theme = await command('get_theme_data', {}); assert.ok(Array.isArray(theme.themes));
  await stop(); await start();
  const history = await command('get_recent_projects', {});
  assert.ok(history.some(row => row.id === project.id && row.path === root));
  report.checks.push('extracted Runtime serves shared project/theme commands and reopens persisted project identity');
  await writeFile(join(root, 'result.json'), JSON.stringify(report, null, 2));
  console.log(`PASS extracted release Runtime: ${join(root, 'result.json')}`);
} catch (error) {
  report.error = String(error);
  await writeFile(join(root, 'failure.json'), JSON.stringify(report, null, 2));
  console.error(`FAIL extracted release Runtime: ${join(root, 'failure.json')}`);
  throw error;
} finally { await stop(); }
