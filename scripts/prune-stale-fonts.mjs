// scripts/prune-stale-fonts.mjs
//
// 构建产物清理守卫：从 web-remote 输出目录 *以及* 它们的 Tauri 资源 STAGING 副本
// （target/{debug,release}/remote-dist）里删除陈旧的超大字体。
//
// 为什么需要：vite/SvelteKit 会清空各自的 outDir，但 Tauri 的资源 staging 是
// **增量拷贝**——源码里删掉的字体（如 ce6e679 移除的 4.8MB NotoColorEmoji.ttf）会
// 滞留在 staging 里，被打进安装包 / 被云端部署继续 serve（公网远控仍请求该字体的
// 根因）。beforeBuildCommand 先于 cargo bundle 跑：此处清掉上一轮遗留的 staging
// 大字体，Tauri 再从干净源码重新 stage，最终 staging 即干净。
//
// 合法的 web-remote 字体仅余小型 UI 字体（如 codicon ~70KB）；>1MB 的
// 字体一律视为回归残留。按体积清理，绝不误伤 UI 字体。

import { readdirSync, statSync, rmSync } from 'node:fs';
import { join, resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const MAX_FONT_BYTES = 1024 * 1024; // 1MB —— codicon 远低于此
const FONT_RE = /\.(ttf|otf|woff2?)$/i;

export const SCAN_DIRS = [
  'remote-dist',
  'target/debug/remote-dist',
  'target/release/remote-dist',
].map((p) => join(root, p));

/** @param {string} dir @param {string[]} out */
export function walk(dir, out, fsImpl) {
  out ??= [];
  const fs = fsImpl ?? { readdirSync };
  let entries;
  try {
    entries = fs.readdirSync(dir, { withFileTypes: true });
  } catch {
    return out; // 目录不存在（未构建/未 stage）—— 跳过
  }
  for (const e of entries) {
    const p = join(dir, e.name);
    if (e.isDirectory()) walk(p, out, fs);
    else out.push(p);
  }
  return out;
}

function pruneFont(file, fs, io) {
  if (!FONT_RE.test(file)) return false;
  let size = 0;
  try {
    size = fs.statSync(file).size;
  } catch {
    return false;
  }
  if (size <= MAX_FONT_BYTES) return false;
  try {
    fs.rmSync(file);
    io.log(
      `[prune-stale-fonts] removed ${file} (${(size / 1024 / 1024).toFixed(2)}MB)`
    );
    return true;
  } catch (e) {
    io.warn(`[prune-stale-fonts] failed to remove ${file}:`, e.message);
    return false;
  }
}

export function pruneOutputs({ dirs = SCAN_DIRS, fsImpl, io = console } = {}) {
  const fs = fsImpl ?? { readdirSync, statSync, rmSync };
  let pruned = 0;
  for (const dir of dirs) {
    for (const file of walk(dir, [], fs)) {
      if (pruneFont(file, fs, io)) pruned += 1;
    }
  }
  if (pruned === 0) io.log('[prune-stale-fonts] no oversized fonts found (clean)');
  return pruned;
}

if (process.argv[1] && resolve(process.argv[1]) === resolve(fileURLToPath(import.meta.url))) pruneOutputs();
