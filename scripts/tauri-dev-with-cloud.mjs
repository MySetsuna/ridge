// tauri dev with local ridge-cloud
// 启动 Main (5173) + Remote (5174) Vite servers，然后运行 tauri dev (不重复启动 Vite)

import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import path from 'node:path';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(__dirname, '..');

// 设置 cloud 环境变量
process.env.RIDGE_CLOUD_BASE_DOMAIN = 'localhost:5001';

console.log('═══════════════════════════════════════');
console.log('  Tauri Dev with Local Ridge Cloud');
console.log('═══════════════════════════════════════');
console.log('  RIDGE_CLOUD_BASE_DOMAIN=localhost:5001');
console.log('  Main Vite:  http://localhost:5173');
console.log('  Remote Vite: http://localhost:5174');
console.log('═══════════════════════════════════════');

let mainVitePid = null;
let remoteVitePid = null;

// 清理函数
function cleanup() {
    console.log('\n🛑 停止所有服务...');
    if (mainVitePid) process.kill(mainVitePid);
    if (remoteVitePid) process.kill(remoteVitePid);
    process.exit(0);
}
process.on('SIGINT', cleanup);
process.on('SIGTERM', cleanup);

// 1. 启动 Main Vite (5173)
console.log('🚀 启动 Main Vite dev server (5173)...');
const mainVite = spawn('pnpm', ['exec', 'vite', 'dev'], {
    cwd: root,
    stdio: 'inherit',
    shell: true,
    env: { ...process.env }
});
mainVitePid = mainVite.pid;

// 等待一下
await new Promise(r => setTimeout(r, 2000));

// 2. 启动 Remote Vite (5174)
console.log('🚀 启动 Remote Vite dev server (5174)...');
const remoteVite = spawn('pnpm', ['exec', 'vite', 'dev', '--config', 'vite.remote.config.js'], {
    cwd: root,
    stdio: 'inherit',
    shell: true,
    env: { ...process.env }
});
remoteVitePid = remoteVite.pid;

// 等待 remote 启动
await new Promise(r => setTimeout(r, 2000));

// 3. 运行 tauri dev (但不让它启动 Vite)
// 通过设置 TAURI_SKIP_VITE_DEV=1
console.log('🚀 启动 Tauri dev...');
const tauri = spawn('pnpm', ['tauri', 'dev'], {
    cwd: root,
    stdio: 'inherit',
    shell: true,
    env: {
        ...process.env,
        TAURI_SKIP_VITE_DEV: '1'
    }
});

tauri.on('exit', (code) => {
    console.log(`\n🏁 Tauri dev 退出 (code: ${code})`);
    cleanup();
});