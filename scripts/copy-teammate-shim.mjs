/**
 * 将 tmux 从 Cargo target/release 复制到 dist/teammate-shim/，
 * 与主程序安装包（src-tauri/target/release/bundle/）输出目录区分。
 */
import { copyFileSync, mkdirSync, existsSync } from 'fs';
import { join, dirname } from 'path';
import { fileURLToPath } from 'url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const root = join(__dirname, '..');
const releaseDir = join(root, 'src-tauri', 'target', 'release');
const binName = process.platform === 'win32' ? 'tmux.exe' : 'tmux';
const targetName = binName;
const from = join(releaseDir, binName);
const toDir = join(root, 'dist', 'teammate-shim');
const to = join(toDir, targetName);

if (!existsSync(from)) {
  console.error(`[copy-teammate-shim] 未找到 ${from}，请先执行: cargo build --release --bin tmux`);
  process.exit(1);
}

mkdirSync(toDir, { recursive: true });
copyFileSync(from, to);
console.log(`[copy-teammate-shim] ${from} -> ${to}`);
