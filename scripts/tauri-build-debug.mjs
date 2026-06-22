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
import { spawn, spawnSync } from 'child_process';
import fs from 'fs';
import path from 'path';
import { fileURLToPath } from 'url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const root = path.join(__dirname, '..');

const base =
  process.env.RIDGE_CLOUD_BASE_DOMAIN ||
  process.env.RIDGE_BASE_DOMAIN ||
  'localhost:5001';

// 默认 debug profile（快、复用热 target/debug）；RIDGE_BUILD_RELEASE=1 → 生产 release。
const releaseProfile = process.env.RIDGE_BUILD_RELEASE === '1';
const profileDir = releaseProfile ? 'release' : 'debug';
// 验证包默认只打 nsis（主安装包）；release 生产包打 nsis+msi。
const bundles = releaseProfile ? 'nsis,msi' : 'nsis';

// 本机可选提速：装了就用，没装则照常（不破坏 CI / 无工具环境）。
function hasBin(name) {
  const probe = process.platform === 'win32' ? 'where' : 'which';
  return spawnSync(probe, [name], { stdio: 'ignore', shell: true }).status === 0;
}

const env = {
  ...process.env,
  RIDGE_CLOUD_BASE_DOMAIN: base, // 桌面端（vite define → apiClient BASE_DOMAIN）
  RIDGE_BASE_DOMAIN: base, // CLI（cargo option_env! 烘焙）
};
if (hasBin('sccache')) {
  env.RUSTC_WRAPPER = 'sccache';
  console.log('[build-debug] sccache: ON');
}
if (hasBin('lld-link')) {
  env.CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER = 'lld-link';
  console.log('[build-debug] lld-link: ON');
}

const args = ['tauri', 'build'];
if (!releaseProfile) args.push('--debug');
args.push('--bundles', bundles);

console.log(`[build-debug] profile=${profileDir} bundles=${bundles} cloud-base=${base}`);
console.log(`[build-debug] running: pnpm ${args.join(' ')} …`);
const startedAt = Date.now();

const child = spawn('pnpm', args, {
  cwd: root,
  env,
  stdio: 'inherit',
  shell: true,
});

child.on('exit', (code) => {
  if (code !== 0) {
    console.error(`[build-debug] tauri build failed (exit ${code})`);
    process.exit(code ?? 1);
  }
  const mins = ((Date.now() - startedAt) / 60000).toFixed(1);
  console.log(`[build-debug] build finished in ${mins} min`);
  renameDebugArtifacts(base, profileDir, bundles);
});

/** 复制安装包到 release/ 并打 -debug 后缀，避免与生产包混淆。 */
function renameDebugArtifacts(baseDomain, profDir, bundleList) {
  const pkg = JSON.parse(fs.readFileSync(path.join(root, 'package.json'), 'utf-8'));
  const version = pkg.version;
  // cargo 工作区 target 在仓库根：tauri 把安装包产到 <root>/target/<profile>/bundle。
  // --debug → target/debug/bundle；release → target/release/bundle。
  const bundleDir = path.join(root, 'target', profDir, 'bundle');
  const outputDir = path.join(root, 'release');
  if (!fs.existsSync(outputDir)) fs.mkdirSync(outputDir);
  const safeBase = baseDomain.replace(/[^a-zA-Z0-9]+/g, '-');
  const folders = bundleList.split(',').map((b) => b.trim());
  let copied = 0;
  for (const folder of folders) {
    const folderPath = path.join(bundleDir, folder);
    if (!fs.existsSync(folderPath)) continue;
    const ext = folder === 'nsis' ? 'exe' : folder === 'msi' ? 'msi' : null;
    if (!ext) continue;
    for (const file of fs.readdirSync(folderPath)) {
      if (!file.endsWith(`.${ext}`)) continue;
      const src = path.join(folderPath, file);
      const dest = path.join(outputDir, `ridge_${version}_x64-debug-${safeBase}-setup.${ext}`);
      fs.copyFileSync(src, dest);
      console.log(`[build-debug] → ${dest}`);
      copied++;
    }
  }
  if (copied === 0) {
    console.warn(`[build-debug] WARN: no installer found under ${bundleDir} (folders: ${folders.join(',')})`);
  }
}
