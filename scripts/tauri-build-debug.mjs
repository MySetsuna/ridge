// Debug 包构建：出一版指向本地 cloud 的可安装包，用于在真 WebView2 上验证
// （桌面端 + remote SPA + tmux shim + CLI 一并对齐到同一 cloud base）。
//
// 用法：
//   pnpm tauri:build:debug                                    # 默认 = debug profile（快）
//   RIDGE_BUILD_RELEASE=1 pnpm tauri:build:debug              # release profile（生产级，慢）
//   RIDGE_CLOUD_BASE_DOMAIN=host:port pnpm tauri:build:debug  # 自定义 cloud base
//
// 为什么默认 debug profile（2026-06-22 修构建链路）：
//   旧版恒跑 `tauri build`（release profile），把整个 Rust 工作区在独立的
//   target/release 里**冷编译 + 优化**，验证一个包要十几~几十分钟。验证用包
//   不需要 release 优化——WebView2 present 期的闪/乱是 GPU 呈现行为，与 cargo
//   优化级别无关。改用 `--debug`：
//     · 复用 `tauri dev` 已编好的**热 target/debug**（桌面 crate 多数无需重编）；
//     · 跳过 LTO / codegen 优化；
//     · 仍是打包后的真 app 跑真 WebView2，忠实复现 present 期症状。
//   再加 `--bundles nsis`（只打主安装包，跳过 msi 翻倍打包）。
//   需要生产级优化包（如对外分发 / 性能测）时设 RIDGE_BUILD_RELEASE=1。
//
// 机制（单点 cloud base，桌面端 + CLI 对齐）：
//   - 桌面端：RIDGE_CLOUD_BASE_DOMAIN 经 vite.config.js 的 define 注入
//     apiClient 的 BASE_DOMAIN。
//   - CLI   ：RIDGE_BASE_DOMAIN 经 ridge-cli/src/config.rs 的 option_env!
//     编译期烘焙。
//   tauri 的 before{Build,Bundle}Command 子进程继承本进程 env。
import { spawn, spawnSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.join(path.dirname(fileURLToPath(import.meta.url)), '..');

export function hasBin(name, spawnSyncImpl = spawnSync, platform = process.platform) {
  const probe = platform === 'win32' ? 'where' : 'which';
  return spawnSyncImpl(probe, [name], { stdio: 'ignore', shell: true }).status === 0;
}

export function buildDebugPlan(envSource = process.env, platform = process.platform, spawnSyncImpl = spawnSync) {
  const base = envSource.RIDGE_CLOUD_BASE_DOMAIN || envSource.RIDGE_BASE_DOMAIN || 'localhost:5001';
  const releaseProfile = envSource.RIDGE_BUILD_RELEASE === '1';
  const profileDir = releaseProfile ? 'release' : 'debug';
  const bundles = releaseProfile ? 'nsis,msi' : 'nsis';
  const env = { ...envSource, RIDGE_CLOUD_BASE_DOMAIN: base, RIDGE_BASE_DOMAIN: base };
  const sccache = hasBin('sccache', spawnSyncImpl, platform);
  const lld = hasBin('lld-link', spawnSyncImpl, platform);
  if (sccache) env.RUSTC_WRAPPER = 'sccache';
  if (lld) env.CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER = 'lld-link';
  const args = ['tauri', 'build'];
  if (!releaseProfile) args.push('--debug');
  args.push('--bundles', bundles);
  return { base, profileDir, bundles, env, args, releaseProfile, sccache, lld };
}

function bundleExtension(folder) {
  if (folder === 'nsis') return 'exe';
  if (folder === 'msi') return 'msi';
  return null;
}

function copyBundleFolder(folder, bundleDir, outputDir, version, safeBase, fsImpl, io) {
  const folderPath = path.join(bundleDir, folder);
  if (!fsImpl.existsSync(folderPath)) return 0;
  const ext = bundleExtension(folder);
  if (!ext) return 0;
  let copied = 0;
  for (const file of fsImpl.readdirSync(folderPath)) {
    if (!file.endsWith(`.${ext}`)) continue;
    const dest = path.join(outputDir, `ridge_${version}_x64-debug-${safeBase}-setup.${ext}`);
    fsImpl.copyFileSync(path.join(folderPath, file), dest);
    io.log(`[build-debug] → ${dest}`);
    copied++;
  }
  return copied;
}

export function renameDebugArtifacts(baseDomain, profDir, bundleList, { rootDir = root, fsImpl = fs, io = console } = {}) {
  const version = JSON.parse(fsImpl.readFileSync(path.join(rootDir, 'package.json'), 'utf-8')).version;
  const bundleDir = path.join(rootDir, 'target', profDir, 'bundle');
  const outputDir = path.join(rootDir, 'release');
  if (!fsImpl.existsSync(outputDir)) fsImpl.mkdirSync(outputDir);
  const safeBase = baseDomain.replace(/[^a-zA-Z0-9]+/g, '-');
  const copied = bundleList
    .split(',')
    .map((folder) => folder.trim())
    .reduce((total, folder) => total + copyBundleFolder(folder, bundleDir, outputDir, version, safeBase, fsImpl, io), 0);
  if (copied === 0) io.warn(`[build-debug] WARN: no installer found under ${bundleDir} (folders: ${bundleList})`);
  return copied;
}

function completeBuild(code, plan, startedAt, { rootDir, fsImpl, io, now, resolve }) {
  if (code !== 0) {
    io.error(`[build-debug] tauri build failed (exit ${code})`);
    resolve(code ?? 1);
    return;
  }
  io.log(`[build-debug] build finished in ${((now() - startedAt) / 60000).toFixed(1)} min`);
  renameDebugArtifacts(plan.base, plan.profileDir, plan.bundles, { rootDir, fsImpl, io });
  resolve(0);
}

export function main({ envSource = process.env, platform = process.platform, spawnImpl = spawn, spawnSyncImpl = spawnSync, fsImpl = fs, rootDir = root, io = console, now = Date.now } = {}) {
  const plan = buildDebugPlan(envSource, platform, spawnSyncImpl);
  io.log(`[build-debug] profile=${plan.profileDir} bundles=${plan.bundles} cloud-base=${plan.base}`);
  io.log(`[build-debug] running: pnpm ${plan.args.join(' ')} …`);
  const startedAt = now();
  return new Promise((resolve) => {
    const child = spawnImpl('pnpm', plan.args, { cwd: root, env: plan.env, stdio: 'inherit', shell: true });
    child.on('exit', (code) => completeBuild(code, plan, startedAt, { rootDir, fsImpl, io, now, resolve }));
  });
}

if (process.argv[1] && path.resolve(process.argv[1]) === path.resolve(fileURLToPath(import.meta.url))) {
  process.exit(await main());
}
