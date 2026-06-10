// Build the flag-only color-emoji subset for the web-remote.
//
// Source: static/fonts/NotoColorEmoji.ttf (the desktop's bundled COLRv1 face,
// kept in-repo). Output: src/remote/public/fonts/flags.woff2 — only Regional
// Indicator pairs + subdivision-flag tag sequences, so it stays tiny and is
// fetched on-demand via a unicode-range @font-face.
//
// Requires fonttools on PATH:  pip install fonttools brotli
import { execFileSync } from 'node:child_process';
import { existsSync, statSync, mkdirSync, rmSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, resolve } from 'node:path';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const src = resolve(root, 'static/fonts/NotoColorEmoji.ttf');
const out = resolve(root, 'src/remote/public/fonts/flags.woff2');
const MAX_BYTES = 800 * 1024; // 800 KB ceiling — see docs/superpowers/specs/2026-06-10-remote-emoji-font-diet-design.md §7

if (!existsSync(src)) {
  console.error(`Source font missing: ${src}`);
  process.exit(1);
}
mkdirSync(dirname(out), { recursive: true });

// --layout-features=* keeps GSUB so Noto can ligate the RI pairs (and the
// U+1F3F4 + tag sequences) into single flag glyphs. COLR/CPAL ride along with
// the kept glyphs so the flags stay colored.
try {
  execFileSync(
    'pyftsubset',
    [
      src,
      '--unicodes=U+1F1E6-1F1FF,U+1F3F4,U+E0020-E007F',
      '--layout-features=*',
      '--flavor=woff2',
      `--output-file=${out}`,
    ],
    { stdio: 'inherit' },
  );
} catch (err) {
  if (err && err.code === 'ENOENT') {
    console.error('pyftsubset not found. Install it with: pip install fonttools brotli');
  }
  process.exit(1);
}

const bytes = statSync(out).size;
const kb = (bytes / 1024).toFixed(1);
if (bytes > MAX_BYTES) {
  console.error(
    `flags.woff2 is ${kb} KB — exceeds the ${MAX_BYTES / 1024} KB red line. ` +
      `Re-evaluate (design section 7).`,
  );
  rmSync(out);
  process.exit(1);
}
console.log(`flags.woff2: ${kb} KB (<= ${MAX_BYTES / 1024} KB) OK`);
