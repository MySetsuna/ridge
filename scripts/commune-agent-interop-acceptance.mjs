// Real installed CLI startup only. This never sends a model prompt.
// Full dispatch/approval/cancel and phone/WAN acceptance remain separate evidence.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawn } from 'node:child_process';
import { stripVTControlCharacters } from 'node:util';

const skipGrok = process.argv.slice(2).includes('--skip-grok');
assert.ok(process.argv.slice(2).every(arg => arg === '--skip-grok'), 'unsupported interoperability acceptance argument');
const target = process.env.CARGO_TARGET_DIR ?? path.resolve('target');
const executable = process.env.RIDGE_AGENT_INTEROP_EXE ?? path.join(target, 'debug', process.platform === 'win32' ? 'ridge.exe' : 'ridge');
assert.ok(fs.existsSync(executable), 'build ridge-cli before real Agent interoperability acceptance');
const probeDirectory = fs.mkdtempSync(path.join(os.tmpdir(), 'ridge-interop-binary-'));
const probeExecutable = path.join(probeDirectory, path.basename(executable));
fs.copyFileSync(executable, probeExecutable);
const kernelExecutable = path.join(target, 'debug', process.platform === 'win32' ? 'ridge-kernel.exe' : 'ridge-kernel');
const probeKernel = path.join(probeDirectory, path.basename(kernelExecutable));
fs.copyFileSync(kernelExecutable, probeKernel);
const stopOwned = async child => {
  if (child.exitCode !== null || child.signalCode !== null) return;
  const closed = new Promise(resolve => child.once('close', resolve));
  if (process.platform === 'win32') {
    await new Promise(resolve => spawn('taskkill', ['/F', '/T', '/PID', String(child.pid)], { windowsHide: true, stdio: 'ignore' }).once('close', resolve));
  } else { try { process.kill(-child.pid, 'SIGKILL'); } catch {} }
  await Promise.race([closed, new Promise(resolve => setTimeout(resolve, 3000))]);
};
const evidenceDirectory = path.resolve(process.env.RIDGE_COMMUNE_EVIDENCE_DIR ?? 'artifacts/commune103');
fs.mkdirSync(evidenceDirectory, { recursive: true });
const matrix = { scope: 'Explicit new session initialization and orderly terminal exit against installed CLIs, zero model prompts', results: [], fullConversationAcceptance: 'UNVERIFIED' };

for (const profile of ['codex', 'claude', 'grok', 'opencode']) {
  if (profile === 'grok' && skipGrok) {
    matrix.results.push({profile, initialization:'SKIPPED', reason:'User explicitly waived Grok real CLI validation on 2026-11-16; initialization and full conversation remain unverified'});
    fs.writeFileSync(path.join(evidenceDirectory, 'agent-interop.json'), JSON.stringify(matrix, null, 2));
    console.log('grok SKIPPED user-authorized validation waiver; unverified');
    continue;
  }
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), `ridge-interop-${profile}-`));
  const logPath = path.join(evidenceDirectory, `interop-${profile}.log`);
  const log = fs.openSync(logPath, 'w');
  const kernel = spawn(probeKernel, [], { windowsHide: true, detached: process.platform !== 'win32', stdio: 'ignore',
    env: { ...process.env, RIDGE_CDP_SHELL_PATH: process.env.PATH, RIDGE_KERNEL_DATA_DIR: directory, RIDGE_KERNEL_FS_ROOT: directory,
      RIDGE_WORKSPACE_SAVE_DIR: directory, RIDGE_AUTH_FILE: path.join(directory, 'auth.json') } });
  let endpoint;
  const readyDeadline = Date.now() + 10000;
  while (!endpoint) {
    try { const value = JSON.parse(fs.readFileSync(path.join(directory, 'kernel.json'), 'utf8')); if (value.pid === kernel.pid) endpoint = value; } catch {}
    if (Date.now() > readyDeadline || kernel.exitCode !== null) { await stopOwned(kernel); throw Error('owned interop Runtime failed to start'); }
    if (!endpoint) await new Promise(resolve => setTimeout(resolve, 25));
  }
  const child = spawn(probeExecutable, ['agent', profile, '--cwd', directory], {
    windowsHide: true,
    detached: process.platform !== 'win32',
    stdio: ['pipe', 'pipe', 'pipe'],
    env: { ...process.env, RIDGE_KERNEL_DATA_DIR: directory, RIDGE_AUTH_FILE: path.join(directory, 'auth.json') },
  });
  let output = '', timedOut = false;
  for (const stream of [child.stdout, child.stderr]) stream.on('data', bytes => {
    fs.writeSync(log, bytes);
    output = (output + bytes.toString()).slice(-65536);
  });
  child.stdin.on('error', () => {});
  child.stdin.end('/exit\n');
  const timeout = setTimeout(() => {
    timedOut = true;
    // This child was created above. Reclaim only its owned process tree.
    void stopOwned(child);
  }, 45000);
  const outcome = await new Promise(resolve => {
    child.once('error', error => resolve({ code: null, error: error.message }));
    child.once('close', code => resolve({ code }));
  });
  clearTimeout(timeout);
  fs.closeSync(log);
  output = stripVTControlCharacters(output.replace(/\x1b\[\d+;\d+H/g, '\n'));
  const initialized = outcome.code === 0 && /^Session \S+/m.test(output);
  try { await fetch(`http://127.0.0.1:${endpoint.port}/v1/shutdown`, { method: 'POST', headers: { 'x-ridge-kernel-token': endpoint.token }, signal: AbortSignal.timeout(3000) }); } catch {}
  await new Promise(resolve => setTimeout(resolve, 200));
  await stopOwned(kernel);
  const advertised = path.join(directory, 'agent-connections');
  const residual = fs.existsSync(advertised) ? fs.readdirSync(advertised) : [];
  const reason = initialized ? 'real session initialized and terminal exited'
    : timedOut ? 'startup deadline exceeded; owned process tree terminated'
    : /unavailable|not found|not installed|No such file/i.test(output) ? 'required executable or protocol adapter unavailable'
    : /auth|credential|login|log in/i.test(output) ? 'Agent authentication or configuration requires attention'
    : 'real protocol startup failed; inspect local evidence log';
  matrix.results.push({ profile, initialization: initialized ? 'PASS' : 'BLOCKED', reason, endpointFilesAfterExit: residual.length, agentInfo: output.match(/^Agent .+$/m)?.[0] ?? null, logPath });
  fs.writeFileSync(path.join(evidenceDirectory, 'agent-interop.json'), JSON.stringify(matrix, null, 2));
  console.log(`${profile} ${initialized ? 'PASS' : 'BLOCKED'} ${reason}`);
  if (initialized) assert.equal(residual.length, 0, `${profile} left endpoint or credential files after orderly exit`);
}
fs.unlinkSync(probeExecutable);
fs.unlinkSync(probeKernel);
fs.rmdirSync(probeDirectory);
if (matrix.results.some(result => result.initialization !== 'PASS' && !(skipGrok && result.profile === 'grok' && result.initialization === 'SKIPPED'))) process.exitCode = 1;
