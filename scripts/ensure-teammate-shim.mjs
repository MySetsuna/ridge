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
import { join, dirname } from 'path';
import { fileURLToPath } from 'url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const root = join(__dirname, '..');
const binName = process.platform === 'win32' ? 'tmux.exe' : 'tmux';
const shimPath = join(root, 'dist', 'teammate-shim', binName);
const sourcePath = join(root, 'src-tauri', 'src', 'bin', 'tmux.rs');

function isStale() {
  if (!existsSync(shimPath)) return true;
  try {
    return statSync(sourcePath).mtimeMs > statSync(shimPath).mtimeMs;
  } catch {
    return true;
  }
}

if (!isStale()) {
  console.log(`[ensure-teammate-shim] up-to-date at ${shimPath}`);
  process.exit(0);
}

console.log('[ensure-teammate-shim] missing or source newer — rebuilding...');
execSync(
  'cargo build --release --bin tmux --manifest-path src-tauri/Cargo.toml',
  { stdio: 'inherit', cwd: root }
);
execSync('node scripts/copy-teammate-shim.mjs', {
  stdio: 'inherit',
  cwd: root,
});
