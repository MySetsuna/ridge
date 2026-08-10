/**
 * 将 tmux 从 Cargo target/release 复制到 dist/teammate-shim/，
 * 与主程序安装包（target/release/bundle/）输出目录区分。
 *
 * 注：工作区合并（S1）后产物目录在工作区根 target/，不再是 src-tauri/target/。
 */
import { copyFileSync, mkdirSync, existsSync } from 'fs';
import { join, dirname, resolve } from 'path';
import { fileURLToPath } from 'url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');

export function copyTeammateShim({ rootDir = root, platform = process.platform, fsImpl = { existsSync, mkdirSync, copyFileSync }, io = console } = {}) {
  const binName = platform === 'win32' ? 'tmux.exe' : 'tmux';
  const from = join(rootDir, 'target', 'release', binName);
  const to = join(rootDir, 'dist', 'teammate-shim', binName);
  if (!fsImpl.existsSync(from)) {
    io.error(`[copy-teammate-shim] 未找到 ${from}，请先执行: cargo build --release --bin tmux`);
    return false;
  }
  fsImpl.mkdirSync(join(rootDir, 'dist', 'teammate-shim'), { recursive: true });
  fsImpl.copyFileSync(from, to);
  io.log(`[copy-teammate-shim] ${from} -> ${to}`);
  return true;
}

if (process.argv[1] && resolve(process.argv[1]) === resolve(fileURLToPath(import.meta.url))) {
  process.exit(copyTeammateShim() ? 0 : 1);
}
