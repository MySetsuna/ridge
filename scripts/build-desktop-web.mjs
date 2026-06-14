// scripts/build-desktop-web.mjs
//
// Builds the FULL desktop SvelteKit UI as a static SPA for the "desktop UI in a
// browser" remote mode. Sets RIDGE_WEB_REMOTE so vite.config.js aliases
// @tauri-apps/api/* to the WS-backed shims (src/lib/transport/tauriShim) and
// svelte.config.js emits to web-remote-dist/ (outside static/, so adapter-static
// doesn't recursively copy the static dir into itself).
//
// Cross-platform env setup: inline `VAR=… cmd` doesn't work on Windows
// PowerShell, so we spawn vite with the env var injected here (mirrors the
// other scripts/*.mjs wrappers in this repo).

import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { dirname, resolve } from 'node:path';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const isWin = process.platform === 'win32';

const child = spawn(isWin ? 'npx.cmd' : 'npx', ['vite', 'build'], {
  cwd: root,
  stdio: 'inherit',
  env: { ...process.env, RIDGE_WEB_REMOTE: '1' },
  shell: isWin,
});

// 构建成功后清理陈旧的超大字体残留（含 Tauri target staging 增量副本）——
// 在 beforeBuildCommand 阶段、cargo bundle 之前跑，确保不会把旧 NotoColorEmoji.ttf
// 打进安装包 / 部署包。详见 scripts/prune-stale-fonts.mjs。
child.on('exit', async (code) => {
  if (code === 0) {
    try {
      await import('./prune-stale-fonts.mjs');
    } catch (e) {
      console.warn('[build-desktop-web] prune-stale-fonts failed:', e?.message ?? e);
    }
  }
  process.exit(code ?? 1);
});
