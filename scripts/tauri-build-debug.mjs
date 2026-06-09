// Debug 包构建：把 ridge-cloud 客户端（桌面端 + CLI）一并指向本地 cloud。
//
// 用法：
//   pnpm tauri:build:debug                                  # 默认指向 localhost:5173
//   RIDGE_CLOUD_BASE_DOMAIN=host:port pnpm tauri:build:debug  # 自定义 base
//
// 机制（单点 base，桌面端 + CLI 对齐）：
//   - 桌面端：RIDGE_CLOUD_BASE_DOMAIN 经 vite.config.js 的 define 注入到
//     src/lib/remote/cloud/apiClient.ts 的 BASE_DOMAIN（API + 信令 WS +
//     controller 入口域名都从这里取）。
//   - CLI   ：RIDGE_BASE_DOMAIN 经 packages/ridge-cli/src/config.rs 的
//     option_env! 在编译期烘焙（build.rs 已声明 rerun-if-env-changed）。
//   `pnpm tauri build` 的 before{Build,Bundle}Command 子进程继承本进程 env，
//   所以一次构建即可让桌面端 + remote SPA + tmux shim + CLI 全部对齐。
//
// 注意：子域信令 `{device}-{username}.localhost` 在 Chromium/WebView2 会自动
// 解析到 127.0.0.1，故子域模型在 localhost 同样可用；本地需运行一个监听
// 该 base（默认 https://localhost:5173）的 ridge-cloud 实例。
import { spawn } from 'child_process';
import fs from 'fs';
import path from 'path';
import { fileURLToPath } from 'url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const root = path.join(__dirname, '..');

const base =
  process.env.RIDGE_CLOUD_BASE_DOMAIN ||
  process.env.RIDGE_BASE_DOMAIN ||
  'localhost:5001';

const env = {
  ...process.env,
  RIDGE_CLOUD_BASE_DOMAIN: base, // 桌面端（vite define → apiClient BASE_DOMAIN）
  RIDGE_BASE_DOMAIN: base, // CLI（cargo option_env! 烘焙）
};

console.log(`[build-debug] ridge-cloud base → ${base}`);
console.log('[build-debug] building desktop + remote SPA + tmux shim + CLI via `tauri build` …');

const child = spawn('pnpm', ['tauri', 'build'], {
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
  renameDebugArtifacts(base);
});

/** 复制安装包到 release/ 并打 -debug 后缀，避免与生产包混淆。 */
function renameDebugArtifacts(baseDomain) {
  const pkg = JSON.parse(fs.readFileSync(path.join(root, 'package.json'), 'utf-8'));
  const version = pkg.version;
  // cargo 工作区 target 在仓库根（非 src-tauri/）：tauri 把安装包产到 <root>/target/release/bundle。
  const bundleDir = path.join(root, 'target', 'release', 'bundle');
  const outputDir = path.join(root, 'release');
  if (!fs.existsSync(outputDir)) fs.mkdirSync(outputDir);
  const safeBase = baseDomain.replace(/[^a-zA-Z0-9]+/g, '-');
  for (const folder of ['nsis', 'msi']) {
    const folderPath = path.join(bundleDir, folder);
    if (!fs.existsSync(folderPath)) continue;
    const ext = folder === 'nsis' ? 'exe' : 'msi';
    for (const file of fs.readdirSync(folderPath)) {
      if (!file.endsWith(`.${ext}`)) continue;
      const src = path.join(folderPath, file);
      const dest = path.join(outputDir, `ridge_${version}_x64-debug-${safeBase}-setup.${ext}`);
      fs.copyFileSync(src, dest);
      console.log(`[build-debug] → ${dest}`);
    }
  }
}
