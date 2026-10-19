// Owned debug presentation against a real external CLI and the selected Runtime.
import assert from 'node:assert/strict';
import { copyFile, mkdir, open, readFile } from 'node:fs/promises';
import { spawn, execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { join, resolve } from 'node:path';
import { chromium } from '@playwright/test';

const execute = promisify(execFile), delay = ms => new Promise(done => setTimeout(done, ms));
async function until(check, message) {
  const end = Date.now() + 60000;
  while (Date.now() < end) { if (await check()) return; await delay(100); }
  throw Error(message);
}
async function inventory() {
  return JSON.parse((await execute('powershell', ['-NoProfile', '-Command', 'Get-CimInstance Win32_Process | Select-Object ProcessId,ParentProcessId,ExecutablePath,CreationDate | ConvertTo-Json -Compress'], { windowsHide: true, maxBuffer: 4 * 1024 * 1024 })).stdout);
}
export async function ownedAgentDesktop({ root, kernelBinary, data, workspace, pane, kernelPid }) {
  const desktopBinary = join(root, 'owned-desktop.exe'), profile = join(root, 'webview');
  await mkdir(profile); await copyFile(resolve('target/debug/ridge.exe'), desktopBinary);
  const origins = [...new Set((await readFile(desktopBinary)).toString('latin1').match(/http:\/\/127\.0\.0\.1:\d{2,5}/g))];
  assert.equal(origins.length, 1); const origin = new URL(origins[0]);
  const env = { ...process.env, RIDGE_KERNEL_DATA_DIR: data, RIDGE_KERNEL_FS_ROOT: root,
    RIDGE_AUTH_FILE: join(root, 'auth.json'), RIDGE_WORKSPACE_SAVE_DIR: root, RIDGE_TEST_TLS_DIR: join(root, 'tls'),
    RIDGE_CDP_USER_DATA_DIR: profile, RIDGE_CDP_NATIVE_DATA_DIR: join(root, 'native'), RIDGE_CDP_STARTUP_CWD: root,
    RIDGE_CDP_ALLOW_NON_BREAKAWAY: '1', RIDGE_TEST_ALLOW_NON_BREAKAWAY: '1', RIDGE_DISABLE_SINGLE_INSTANCE: '1',
    RIDGE_KERNEL_HOST_BINARY: kernelBinary, RIDGE_REMOTE_UI_ROOT: resolve('remote-dist'), WEBVIEW2_USER_DATA_FOLDER: profile,
    WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: '--remote-debugging-port=0 --remote-debugging-address=127.0.0.1 --remote-allow-origins=*' };
  delete env.RIDGE_NATIVE_RUNTIME;
  async function launch(binary, args, name) {
    const log = await open(join(root, name + '.log'), 'a');
    const child = spawn(binary, args, { env, windowsHide: true, stdio: ['ignore', log.fd, log.fd] }); await log.close(); return child;
  }
  async function stop(child, binary) {
    if (!child || child.exitCode !== null || child.signalCode !== null) return;
    const rows = await inventory(), owner = rows.find(row => row.ProcessId === child.pid);
    assert.equal(owner?.ExecutablePath?.toLowerCase(), binary.toLowerCase());
    const ids = new Set([child.pid]); let changed = true;
    while (changed) { changed = false; for (const row of rows) if (ids.has(row.ParentProcessId) && !ids.has(row.ProcessId)) { ids.add(row.ProcessId); changed = true; } }
    assert.ok(!ids.has(kernelPid), 'desktop cleanup must not touch Runtime');
    const current = await inventory(), exact = rows.filter(row => ids.has(row.ProcessId) && current.some(now => now.ProcessId === row.ProcessId && now.CreationDate === row.CreationDate && now.ExecutablePath === row.ExecutablePath));
    if (exact.length) await execute('powershell', ['-NoProfile', '-Command', `Stop-Process -Id ${exact.map(row => row.ProcessId).join(',')} -Force -ErrorAction SilentlyContinue`], { windowsHide: true });
    await until(async () => !(await inventory()).some(row => exact.some(old => old.ProcessId === row.ProcessId && old.CreationDate === row.CreationDate)), 'owned desktop descendants did not exit');
  }
  let desktop, browser, page;
  const vite = await launch(process.execPath, ['node_modules/vite/bin/vite.js', '--host', '127.0.0.1', '--port', origin.port, '--strictPort'], 'agent-vite');
  async function closeDesktop() { await stop(desktop, desktopBinary); desktop = null; await browser?.close().catch(() => {}); browser = null; }
  async function openDesktop(expected) {
    desktop = await launch(desktopBinary, [], 'agent-desktop'); let port;
    await until(async () => { assert.equal(desktop.exitCode, null); try { port = Number((await readFile(join(profile, 'EBWebView', 'DevToolsActivePort'), 'utf8')).split('\n')[0]); return (await fetch(`http://127.0.0.1:${port}/json/version`, { signal: AbortSignal.timeout(1000) })).ok; } catch { return false; } }, 'owned Agent desktop CDP unavailable');
    const version = await (await fetch(`http://127.0.0.1:${port}/json/version`)).json();
    assert.ok(version.webSocketDebuggerUrl.startsWith(`ws://127.0.0.1:${port}/`));
    browser = await chromium.connectOverCDP(version.webSocketDebuggerUrl);
    await until(async () => { page = browser.contexts().flatMap(context => context.pages())[0]; return Boolean(page); }, 'owned Agent WebView unavailable');
    await until(async () => page.evaluate(async ({ workspace, pane, expected }) => {
      const { TerminalManager } = await import('/packages/remote/src/shared/terminal/manager.ts');
      const entry = TerminalManager.instance().panes.get(pane), context = entry?.kernel.terminalInputContext();
      return context?.[0]?.workspaceId === workspace && entry.kernel.dumpVisibleText().join('\n').includes(expected);
    }, { workspace, pane, expected }).catch(() => false), 'desktop did not render the original live Agent');
    await page.screenshot({ path: join(root, expected + '-desktop.png') });
  }
  try {
    await until(async () => { assert.equal(vite.exitCode, null); try { return (await fetch(origin, { signal: AbortSignal.timeout(1000) })).ok; } catch { return false; } }, 'owned Agent frontend unavailable');
    await openDesktop('TERMINAL_REPLY');
    return { kill: closeDesktop, reopen: openDesktop, close: async () => { await closeDesktop(); await stop(vite, process.execPath); } };
  } catch (error) { await closeDesktop(); await stop(vite, process.execPath); throw error; }
}
