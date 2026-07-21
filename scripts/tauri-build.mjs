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

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const root = path.join(__dirname, '..');

function hasBin(name) {
  const probe = process.platform === 'win32' ? 'where' : 'which';
  return spawnSync(probe, [name], { stdio: 'ignore', shell: true }).status === 0;
}

const env = { ...process.env };
if (hasBin('sccache')) {
  env.RUSTC_WRAPPER = 'sccache';
  console.log('[tauri-build] sccache: ON (RUSTC_WRAPPER=sccache)');
} else {
  console.log('[tauri-build] sccache: off (not installed — `scoop install sccache` to enable)');
}
if (hasBin('lld-link')) {
  env.CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER = 'lld-link';
  console.log('[tauri-build] lld-link: ON (faster MSVC linker)');
} else {
  console.log('[tauri-build] lld-link: off (not on PATH — `scoop install llvm` to enable)');
}

const bundles = process.env.RIDGE_BUNDLES || 'nsis';
const args = ['tauri', 'build', '--bundles', bundles];
console.log(`[tauri-build] bundles=${bundles}`);
console.log(`[tauri-build] running: pnpm ${args.join(' ')} …`);
const startedAt = Date.now();

const child = spawn('pnpm', args, { cwd: root, env, stdio: 'inherit', shell: true });
child.on('exit', (code) => {
  if (code !== 0) {
    console.error(`[tauri-build] tauri build failed (exit ${code})`);
    process.exit(code ?? 1);
  }
  const mins = ((Date.now() - startedAt) / 60000).toFixed(1);
  console.log(`[tauri-build] build finished in ${mins} min`);
  // 沿用既有产物重命名（target/release/bundle → release/）。
  const rename = spawnSync('node', [path.join('scripts', 'post-build-rename.mjs')], {
    cwd: root,
    stdio: 'inherit',
    shell: true,
  });
  process.exit(rename.status ?? 0);
});
