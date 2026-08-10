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
export const MAX_BYTES = 800 * 1024; // 800 KB ceiling — design red line (actual ≈ 77 KB)

// Pinned upstream release asset (NOT a moving branch ref) for reproducible
// builds, plus a content hash so a tampered/changed download is rejected.
export const TWEMOJI_URL =
  'https://github.com/mozilla/twemoji-colr/releases/download/v0.7.0/Twemoji.Mozilla.ttf';
export const TWEMOJI_SHA256 =
  '6d90152ee0d29e82fe2a87793af5aa4b7ad13e6538360889e141e81ed299ee8e';

// Download the source font into a build cache (re-used across runs).
export function sha256(filePath, fsImpl) {
  return createHash('sha256').update((fsImpl?.readFileSync ?? readFileSync)(filePath)).digest('hex');
}

export function buildFlagFont({ rootDir = root, fsImpl, execFileSyncImpl = execFileSync, hashFile = sha256, io = console } = {}) {
  const fs = fsImpl ?? { existsSync, statSync, mkdirSync, copyFileSync, readFileSync, rmSync };
  const outRemote = resolve(rootDir, 'src/remote/public/fonts/flags.woff2');
  const outDesktop = resolve(rootDir, 'static/fonts/flags.woff2');
  const cacheDir = resolve(rootDir, 'node_modules/.cache/flag-font-dl');
  const srcFont = resolve(cacheDir, 'Twemoji.Mozilla.ttf');
  fs.mkdirSync(cacheDir, { recursive: true });
  if (!fs.existsSync(srcFont) || hashFile(srcFont, fs) !== TWEMOJI_SHA256) {
    io.log(`Downloading Twemoji (Mozilla COLRv0) from ${TWEMOJI_URL}`);
    execFileSyncImpl('curl', ['-sSL', '-o', srcFont, TWEMOJI_URL], { stdio: 'inherit' });
    const got = hashFile(srcFont, fs);
    if (got !== TWEMOJI_SHA256) {
      io.error(`Source font hash mismatch.\n  expected ${TWEMOJI_SHA256}\n  got      ${got}`);
      fs.rmSync(srcFont, { force: true });
      return false;
    }
  }
  fs.mkdirSync(dirname(outRemote), { recursive: true });
  fs.mkdirSync(dirname(outDesktop), { recursive: true });
  try {
    execFileSyncImpl('pyftsubset', [srcFont, '--unicodes=U+1F1E6-1F1FF,U+1F3F4,U+E0020-E007F', '--layout-features=*', '--drop-tables=vmtx,vhea', '--flavor=woff2', `--output-file=${outRemote}`], { stdio: 'inherit' });
  } catch (err) {
    if (err?.code === 'ENOENT') io.error('pyftsubset not found. Install with: pip install fonttools brotli');
    return false;
  }
  const bytes = fs.statSync(outRemote).size;
  const kb = (bytes / 1024).toFixed(1);
  if (bytes > MAX_BYTES) {
    io.error(`flags.woff2 is ${kb} KB — exceeds the ${MAX_BYTES / 1024} KB red line. Re-evaluate subset configuration.`);
    fs.rmSync(outRemote, { force: true });
    return false;
  }
  fs.copyFileSync(outRemote, outDesktop);
  io.log(`flags.woff2: ${kb} KB (<= ${MAX_BYTES / 1024} KB) OK → remote + desktop`);
  return true;
}

if (process.argv[1] && resolve(process.argv[1]) === resolve(fileURLToPath(import.meta.url))) process.exit(buildFlagFont() ? 0 : 1);
