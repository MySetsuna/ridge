// 生产 release 出包（桌面 app + CLI），带「本机可选」提速：
//   - sccache：装了就经 RUSTC_WRAPPER 启用，跨 clean / 跨分支缓存 crate 编译。
//   - lld-link：在 PATH 上就用作 MSVC 链接器（装：scoop install llvm）。
//   两者都按「存在即用」：没装则照常普通构建，绝不破坏 CI / 没装工具的同事。
//   默认只打 nsis（跳过 msi 翻倍打包）；RIDGE_BUNDLES 可覆盖（如 "nsis,msi"）。
//
// 说明（2026-06-22 修构建链路）：旧 `tauri:build` = `pnpm tauri build &&
// post-build-rename`，恒打 nsis+msi、无 sccache/lld。本包装在不改 profile / 产物
// 的前提下加这三项可选提速。注意 [profile.release] 仍是 opt-z+fatLTO+cgu1（为
// wasm 体积），故 lld 收益有限（瓶颈在 LTO 代码生成而非链接）；sccache 主要省
// 重复/clean 构建的依赖重编。
import { spawn, spawnSync } from 'child_process';
import path from 'path';
import { fileURLToPath } from 'url';

const root = path.join(path.dirname(fileURLToPath(import.meta.url)), '..');

export function hasBin(name, spawnSyncImpl = spawnSync, platform = process.platform) {
  const probe = platform === 'win32' ? 'where' : 'which';
  return spawnSyncImpl(probe, [name], { stdio: 'ignore', shell: true }).status === 0;
}

export function buildPlan(envSource = process.env, platform = process.platform, spawnSyncImpl = spawnSync) {
  const env = { ...envSource };
  const sccache = hasBin('sccache', spawnSyncImpl, platform);
  const lld = hasBin('lld-link', spawnSyncImpl, platform);
  if (sccache) env.RUSTC_WRAPPER = 'sccache';
  if (lld) env.CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER = 'lld-link';
  const bundles = envSource.RIDGE_BUNDLES || 'nsis';
  return { env, bundles, args: ['tauri', 'build', '--bundles', bundles], sccache, lld };
}

export function main({ envSource = process.env, platform = process.platform, spawnImpl = spawn, spawnSyncImpl = spawnSync, io = console, now = Date.now } = {}) {
  const plan = buildPlan(envSource, platform, spawnSyncImpl);
  io.log(plan.sccache ? '[tauri-build] sccache: ON (RUSTC_WRAPPER=sccache)' : '[tauri-build] sccache: off (not installed — `scoop install sccache` to enable)');
  io.log(plan.lld ? '[tauri-build] lld-link: ON (faster MSVC linker)' : '[tauri-build] lld-link: off (not on PATH — `scoop install llvm` to enable)');
  io.log(`[tauri-build] bundles=${plan.bundles}`);
  io.log(`[tauri-build] running: pnpm ${plan.args.join(' ')} …`);
  const startedAt = now();
  return new Promise((resolve) => {
    const child = spawnImpl('pnpm', plan.args, { cwd: root, env: plan.env, stdio: 'inherit', shell: true });
    child.on('exit', (code) => {
      if (code !== 0) { io.error(`[tauri-build] tauri build failed (exit ${code})`); resolve(code ?? 1); return; }
      io.log(`[tauri-build] build finished in ${((now() - startedAt) / 60000).toFixed(1)} min`);
      const rename = spawnSyncImpl('node', [path.join('scripts', 'post-build-rename.mjs')], { cwd: root, stdio: 'inherit', shell: true });
      resolve(rename.status ?? 0);
    });
  });
}

if (process.argv[1] && path.resolve(process.argv[1]) === path.resolve(fileURLToPath(import.meta.url))) {
  main().then((code) => process.exit(code));
}
