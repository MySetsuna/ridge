/**
 * 幂等地确保 dev 路径下存在最新的 tmux shim。
 * 被 tauri.conf.json 的 beforeDevCommand 调用，这样 `pnpm tauri dev`
 * 开箱即用，不用手动跑 build:teammate-shim。
 *
 * 判定策略：shim 存在且不比 src-tauri/src/bin/tmux.rs 旧 → skip；
 * 否则重新构建并拷贝（复用 build:teammate-shim 的两步）。
 */
import { existsSync, statSync } from 'fs';
import { execSync } from 'child_process';
import { join, dirname, resolve } from 'path';
import { fileURLToPath } from 'url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');

export function isTeammateShimStale({ rootDir = root, platform = process.platform, fsImpl = { existsSync, statSync } } = {}) {
  const binName = platform === 'win32' ? 'tmux.exe' : 'tmux';
  const shimPath = join(rootDir, 'dist', 'teammate-shim', binName);
  const sourcePath = join(rootDir, 'src-tauri', 'src', 'bin', 'tmux.rs');
  if (!fsImpl.existsSync(shimPath)) return true;
  try { return fsImpl.statSync(sourcePath).mtimeMs > fsImpl.statSync(shimPath).mtimeMs; }
  catch { return true; }
}

export function main({ rootDir = root, platform = process.platform, fsImpl = { existsSync, statSync }, exec = execSync, io = console } = {}) {
  const stale = isTeammateShimStale({ rootDir, platform, fsImpl });
  const binName = platform === 'win32' ? 'tmux.exe' : 'tmux';
  const shimPath = join(rootDir, 'dist', 'teammate-shim', binName);
  if (!stale) { io.log(`[ensure-teammate-shim] up-to-date at ${shimPath}`); return 0; }
  io.log('[ensure-teammate-shim] missing or source newer — rebuilding...');
  exec('cargo build --release --bin tmux --manifest-path src-tauri/Cargo.toml', { stdio: 'inherit', cwd: rootDir });
  exec('node scripts/copy-teammate-shim.mjs', { stdio: 'inherit', cwd: rootDir });
  return 0;
}

if (process.argv[1] && resolve(process.argv[1]) === resolve(fileURLToPath(import.meta.url))) process.exit(main());
