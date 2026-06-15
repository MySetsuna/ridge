// Build the flag-only color-emoji subset shared by the desktop terminal AND the
// web-remote.
//
// Source: Twemoji (Mozilla COLRv0 build) — flat, low-node-count flag glyphs that
// render via Canvas2D `fillText` in WebView2 / Chromium (COLR/CPAL), unlike the
// newer SVGinOT Twemoji whose `SVG ` table Chromium can't rasterize. We take ONLY
// Regional Indicator pairs + subdivision-flag tag sequences, drop the vertical
// metrics the terminal never uses, and emit a tiny on-demand woff2.
//
// Why Twemoji over Noto: ordinary emoji now come from the OS on both surfaces, so
// this font ONLY draws flags — there is no "match Noto's other emoji" constraint
// anymore, freeing us to pick the smallest flag source. Twemoji's flat geometry
// subsets to ~77KB vs ~699KB for the equivalent Noto subset.
//
// Output (identical bytes, two publish roots):
//   - src/remote/public/fonts/flags.woff2  (web-remote, served at /fonts/)
//   - static/fonts/flags.woff2             (desktop SvelteKit/Tauri, served at /fonts/)
//
// Requires fonttools on PATH:  pip install fonttools brotli
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { existsSync, statSync, mkdirSync, copyFileSync, readFileSync, rmSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, resolve } from 'node:path';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const outRemote = resolve(root, 'src/remote/public/fonts/flags.woff2');
const outDesktop = resolve(root, 'static/fonts/flags.woff2');
const MAX_BYTES = 800 * 1024; // 800 KB ceiling — design red line (actual ≈ 77 KB)

// Pinned upstream release asset (NOT a moving branch ref) for reproducible
// builds, plus a content hash so a tampered/changed download is rejected.
const TWEMOJI_URL =
  'https://github.com/mozilla/twemoji-colr/releases/download/v0.7.0/Twemoji.Mozilla.ttf';
const TWEMOJI_SHA256 =
  '6d90152ee0d29e82fe2a87793af5aa4b7ad13e6538360889e141e81ed299ee8e';

// Download the source font into a build cache (re-used across runs).
const cacheDir = resolve(root, 'node_modules/.cache/flag-font-dl');
mkdirSync(cacheDir, { recursive: true });
const srcFont = resolve(cacheDir, 'Twemoji.Mozilla.ttf');

const sha256 = (path) => createHash('sha256').update(readFileSync(path)).digest('hex');

if (!existsSync(srcFont) || sha256(srcFont) !== TWEMOJI_SHA256) {
  console.log(`Downloading Twemoji (Mozilla COLRv0) from ${TWEMOJI_URL}`);
  execFileSync('curl', ['-sSL', '-o', srcFont, TWEMOJI_URL], { stdio: 'inherit' });
  const got = sha256(srcFont);
  if (got !== TWEMOJI_SHA256) {
    console.error(`Source font hash mismatch.\n  expected ${TWEMOJI_SHA256}\n  got      ${got}`);
    rmSync(srcFont, { force: true });
    process.exit(1);
  }
}

mkdirSync(dirname(outRemote), { recursive: true });
mkdirSync(dirname(outDesktop), { recursive: true });

// --layout-features=* keeps GSUB so the RI pairs (and the U+1F3F4 + tag
// sequences) ligate into single flag glyphs. COLR/CPAL ride along with the kept
// glyphs so the flags stay colored. --drop-tables=vmtx,vhea removes the vertical
// metrics a horizontal terminal never consults.
try {
  execFileSync(
    'pyftsubset',
    [
      srcFont,
      '--unicodes=U+1F1E6-1F1FF,U+1F3F4,U+E0020-E007F',
      '--layout-features=*',
      '--drop-tables=vmtx,vhea',
      '--flavor=woff2',
      `--output-file=${outRemote}`,
    ],
    { stdio: 'inherit' },
  );
} catch (err) {
  if (err && err.code === 'ENOENT') {
    console.error('pyftsubset not found. Install with: pip install fonttools brotli');
  }
  process.exit(1);
}

const bytes = statSync(outRemote).size;
const kb = (bytes / 1024).toFixed(1);
if (bytes > MAX_BYTES) {
  console.error(
    `flags.woff2 is ${kb} KB — exceeds the ${MAX_BYTES / 1024} KB red line. ` +
      `Re-evaluate subset configuration.`,
  );
  rmSync(outRemote, { force: true });
  process.exit(1);
}

// Mirror the identical artifact to the desktop publish root.
copyFileSync(outRemote, outDesktop);
console.log(`flags.woff2: ${kb} KB (<= ${MAX_BYTES / 1024} KB) OK → remote + desktop`);
