// Headed Tauri acceptance with a labelled ACP fixture and an isolated persistent store.
import fs from 'node:fs';
import path from 'node:path';
import net from 'node:net';
import { spawn, spawnSync } from 'node:child_process';
import { chromium, expect } from '@playwright/test';
import { cargoTool } from './lib/toolPath.mjs';
const root = path.resolve(import.meta.dirname, '..');
const run = path.join(root, 'artifacts', `m53-${Date.now()}`);
fs.mkdirSync(run, { recursive: true });
const fixture = path.join(root, 'target', 'debug', 'fake_acp_agent.exe');
const desktop = path.join(root, 'target', 'debug', 'ridge.exe');
const kernel = path.join(run, 'ridge-kernel.exe');
const profile = path.join(run, 'webview');
const store = path.join(run, 'agent-workflows.json');
const control = path.join(run, 'fixture-control');
fs.mkdirSync(control);
const results = { evidence: run, provider: 'MOCK_FIXTURE', SIMPLE_CHAT: 'NOT_RUN', PLAN_REVIEW: 'NOT_RUN', COMMUNION_EXECUTION: 'NOT_RUN', RESTORE_RESUME: 'NOT_RUN' };
const log = fs.openSync(path.join(run, 'tauri.log'), 'a');
let dev, browser;
const env = { ...process.env, CARGO_BUILD_JOBS: '2', RIDGE_DISABLE_SINGLE_INSTANCE: '1', RIDGE_CDP_STARTUP_CWD: root, RIDGE_KERNEL_DATA_DIR: path.join(run, 'kernel'), RIDGE_WORKFLOW_STORE_PATH: store, RIDGE_ACP_PROVIDER_PATH: fixture, CARGO_BIN_EXE_fake_acp_agent: fixture, WEBVIEW2_USER_DATA_FOLDER: profile, WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: '--remote-debugging-port=0 --remote-debugging-address=127.0.0.1 --remote-allow-origins=*', NO_PROXY: 'localhost,127.0.0.1', no_proxy: 'localhost,127.0.0.1' };
// This isolated runner tests workflow restore, not detached-kernel force-kill survival.
env.RIDGE_TEST_ALLOW_NON_BREAKAWAY = '1';
results.kernelLaunch = 'TEST_NON_BREAKAWAY';
env.LOCALAPPDATA = path.join(run, 'app-data');
const delay = ms => new Promise(resolve => setTimeout(resolve, ms));
async function until(check, timeout = 120000) {
  const end = Date.now() + timeout;
  while (Date.now() < end) { const result = await check(); if (result) return result; await delay(100); }
  throw new Error('bounded acceptance observation timed out');
}
async function build() {
  console.log(`M53_BUILD_STARTED evidence=${run}`);
  const child = spawn(cargoTool('cargo'), ['build', '-p', 'ridge', '--bin', 'ridge'], { cwd: root, env, stdio: ['ignore', log, log], windowsHide: true });
  await new Promise((resolve, reject) => { child.on('error', reject); child.on('exit', code => code === 0 ? resolve() : reject(new Error(`desktop build exited ${code}; see tauri.log`))); });
  fs.copyFileSync(desktop, kernel);
  env.RIDGE_KERNEL_HOST_BINARY = kernel;
}
async function stop() {
  if (browser) { await browser.close().catch(() => {}); browser = null; }
  if (dev?.pid && dev.exitCode === null) {
    // Only the process tree this runner created. No process-name or global browser kills.
    spawnSync('taskkill', ['/PID', String(dev.pid), '/T', '/F'], { windowsHide: true, stdio: 'ignore' });
    await new Promise(resolve => { if (dev.exitCode !== null) resolve(); else { dev.once('exit', resolve); setTimeout(resolve, 3000); } });
  }
  dev = null;
}
async function start() {
  const port = await new Promise(resolve => { const server = net.createServer(); server.listen(0, '127.0.0.1', () => { const port = server.address().port; server.close(() => resolve(port)); }); });
  env.RIDGE_DEV_SERVER_PORT = String(port);
  const config = path.join(run, 'tauri.config.json');
  fs.writeFileSync(config, JSON.stringify({ build: { beforeDevCommand: 'node scripts/start-vite-dev.mjs', devUrl: `http://127.0.0.1:${port}` } }));
  const activePort = path.join(profile, 'EBWebView', 'DevToolsActivePort');
  if (fs.existsSync(activePort)) fs.renameSync(activePort, `${activePort}.${Date.now()}.previous`);
  dev = spawn('pnpm', ['tauri', 'dev', '--no-watch', '--config', config], { cwd: root, env, shell: true, windowsHide: true, stdio: ['ignore', log, log] });
  const cdp = await until(async () => {
    if (dev.exitCode !== null) throw new Error(`Tauri exited ${dev.exitCode}; see tauri.log`);
    if (!fs.existsSync(activePort)) return null;
    const port = Number(fs.readFileSync(activePort, 'utf8').split(/\r?\n/)[0]);
    try { const response = await fetch(`http://127.0.0.1:${port}/json/version`); if (response.ok) return { port, ...await response.json() }; } catch {}
    return null;
  }, 600000);
  browser = await chromium.connectOverCDP(cdp.webSocketDebuggerUrl);
  const page = await until(() => browser.contexts().flatMap(context => context.pages()).find(page => page.url().startsWith(`http://127.0.0.1:${port}`)));
  page.setDefaultTimeout(30000);
  page.on('pageerror', error => console.error(`M53_PAGE_ERROR ${error.message}`));
  console.log(`M53_HEADED_CONNECTED cdp=${cdp.port}`);
  await expect(page.getByRole('button', { name: 'Agent Communion', exact: true })).toBeVisible({ timeout: 120000 });
  await page.getByRole('button', { name: 'Agent Communion', exact: true }).click();
  await expect(page.getByRole('textbox', { name: 'Prompt', exact: true })).toBeEnabled();
  return page;
}
const invoke = (page, command, args = {}) => page.evaluate(({ command, args }) => window.__TAURI_INTERNALS__.invoke(command, args), { command, args });
const tasks = plan => plan.stories.flatMap(story => story.tasks);
async function currentPlan(page) { const plans = JSON.parse(fs.readFileSync(store, 'utf8')); return Object.values(plans).sort((a,b) => b.created_at_ms - a.created_at_ms)[0]; }
async function propose(page, title) {
  await page.getByRole('textbox', { name: 'Prompt', exact: true }).fill(title);
  await page.getByRole('button', { name: 'Send', exact: true }).click();
  await expect(page.getByRole('button', { name: 'Confirm Plan', exact: true })).toBeVisible();
  return currentPlan(page);
}
async function approve(page, task) {
  const bar = page.locator(`[data-approval-task="${task}"]`);
  await expect(bar).toBeVisible();
  await bar.getByRole('button', { name: 'Approve', exact: true }).click();
}
try {
  await build();
  let page = await start();
  await page.getByRole('textbox', { name: 'Prompt', exact: true }).fill('Simple fixture acceptance');
  await page.getByRole('button', { name: 'Send', exact: true }).click();
  await expect(page.locator('[data-testid="approval-bar"]')).toBeVisible();
  const before = await invoke(page, 'acp_session_snapshot');
  expect(Object.keys(before.turns)).toHaveLength(1);
  expect(await invoke(page, 'plan_list')).toEqual([]);
  await page.screenshot({ path: path.join(run, 'simple-approval.png') });
  await approve(page, 'lead');
  await expect(page.getByRole('textbox', { name: 'Prompt', exact: true })).toBeEnabled();
  const after = await invoke(page, 'acp_session_snapshot');
  expect(Object.keys(after.turns)).toEqual(Object.keys(before.turns));
  expect(Object.values(after.turns)[0].phase).toBe('completed');
  results.SIMPLE_CHAT = 'PASS';
  console.log('M53_SIMPLE_CHAT=PASS');

  await page.getByRole('button', { name: 'New Chat', exact: true }).click();
  await page.getByRole('combobox', { name: 'Agent / Communion mode' }).selectOption('communion');
  const cancelled = await propose(page, 'Cancel this review before any Staff runs');
  await page.getByRole('button', { name: 'Cancel review', exact: true }).click();
  await until(async () => tasks(await invoke(page, 'plan_get', { planId: cancelled.id })).every(task => task.status === 'cancelled'));
  const plan = await propose(page, 'Run a reviewed A -> (B,C) -> D collaboration');
  await page.getByRole('textbox', { name: 'Title A', exact: true }).fill('Reviewed A');
  await page.getByRole('combobox', { name: 'Point A', exact: true }).selectOption('large');
  for (const [task, name] of [['A','Staff 1'],['B','Staff 1'],['C','Staff 2'],['D','Staff 1']]) await page.getByRole('combobox', { name: `Assignee ${task}`, exact: true }).selectOption(name);
  await page.getByRole('textbox', { name: 'Dependencies D', exact: true }).fill('B, C');
  await page.getByRole('textbox', { name: 'Title A', exact: true }).focus();
  await until(async () => { const reviewed = await invoke(page, 'plan_get', { planId: plan.id }); return tasks(reviewed).find(task => task.id === 'A').title === 'Reviewed A' && tasks(reviewed).find(task => task.id === 'C').assignee === 'Staff 2'; });
  const reviewed = await invoke(page, 'plan_get', { planId: plan.id });
  expect(reviewed.confirmed).toBe(false);
  expect(tasks(reviewed).every(task => task.assigned_session_id === null && task.status === 'queued')).toBe(true);
  results.PLAN_REVIEW = 'PASS';
  await page.screenshot({ path: path.join(run, 'plan-review.png') });
  await page.getByRole('button', { name: 'Confirm Plan', exact: true }).click();
  await approve(page, 'A');
  await expect(page.locator('[data-approval-task="B"]')).toBeVisible();
  await approve(page, 'C');
  await until(async () => tasks(await invoke(page, 'plan_get', { planId: plan.id })).find(task => task.id === 'C').status === 'completed');
  const paused = await invoke(page, 'plan_get', { planId: plan.id });
  expect(tasks(paused).find(task => task.id === 'B').status).toBe('waiting_approval');
  expect(tasks(paused).find(task => task.id === 'D').status).toBe('queued');
  await page.screenshot({ path: path.join(run, 'parallel-approval.png') });
  await approve(page, 'B'); await approve(page, 'D');
  await expect(page.locator('[data-testid="lead-final"]')).toBeVisible();
  const completed = await invoke(page, 'plan_get', { planId: plan.id });
  expect(tasks(completed).every(task => task.status === 'completed')).toBe(true);
  expect(new Set(tasks(completed).map(task => task.assigned_session_id)).size).toBe(2);
  results.COMMUNION_EXECUTION = 'PASS';
  await page.screenshot({ path: path.join(run, 'structured-lead-final.png') });
  console.log('M53_PLAN_REVIEW=PASS M53_COMMUNION_EXECUTION=PASS');

  // Prepare a second real workflow, pause C on an explicit fixture barrier, then restart the host.
  await stop(); env.RIDGE_ACP_FIXTURE_CONTROL_DIR = control;
  page = await start();
  await page.getByRole('combobox', { name: 'Agent / Communion mode' }).selectOption('communion');
  const restarting = await propose(page, 'Restart while C is running; do not repeat A/B');
  await page.getByRole('button', { name: 'Confirm Plan', exact: true }).click();
  await approve(page, 'A'); await approve(page, 'B');
  await until(async () => fs.existsSync(path.join(control, 'C.waiting')) && tasks(await invoke(page, 'plan_get', { planId: restarting.id })).find(task => task.id === 'B').status === 'completed');
  const interrupted = await invoke(page, 'plan_get', { planId: restarting.id });
  expect(tasks(interrupted).find(task => task.id === 'A').status).toBe('completed');
  expect(tasks(interrupted).find(task => task.id === 'B').status).toBe('completed');
  expect(tasks(interrupted).find(task => task.id === 'C').status).toBe('running');
  await page.screenshot({ path: path.join(run, 'before-host-restart.png') });
  await stop(); page = await start();
  await page.getByRole('combobox', { name: 'Agent / Communion mode' }).selectOption('communion');
  await expect(page.getByRole('button', { name: 'Resume C', exact: true })).toBeVisible();
  const restored = await invoke(page, 'plan_get', { planId: restarting.id });
  expect(tasks(restored).find(task => task.id === 'C').status).toBe('interrupted');
  expect(tasks(restored).find(task => task.id === 'D').status).toBe('queued');
  expect(await page.locator('[data-testid="approval-bar"]').count()).toBe(0);
  await page.screenshot({ path: path.join(run, 'interrupted-resume.png') });
  fs.writeFileSync(path.join(control, 'C.resume'), 'continue fresh attempt');
  await page.getByRole('button', { name: 'Resume C', exact: true }).click();
  await approve(page, 'C'); await approve(page, 'D');
  await expect(page.locator('[data-testid="lead-final"]')).toBeVisible();
  const resumed = await invoke(page, 'plan_get', { planId: restarting.id });
  for (const id of ['A','B']) expect(tasks(resumed).find(task => task.id === id)).toEqual(tasks(interrupted).find(task => task.id === id));
  expect(tasks(resumed).find(task => task.id === 'C').assigned_session_id).not.toBe(tasks(interrupted).find(task => task.id === 'C').assigned_session_id);
  results.RESTORE_RESUME = 'PASS';
  await page.screenshot({ path: path.join(run, 'resumed-completed.png') });
  console.log('M53_RESTORE_RESUME=PASS');
} catch (error) {
  results.error = String(error?.stack ?? error);
  console.error(results.error);
  if (browser) await browser.contexts()[0]?.pages()[0]?.screenshot({ path: path.join(run, 'failure.png') }).catch(() => {});
  process.exitCode = 1;
} finally {
  fs.writeFileSync(path.join(run, 'results.json'), JSON.stringify(results, null, 2));
  console.log(JSON.stringify(results));
  await stop(); fs.closeSync(log);
}
