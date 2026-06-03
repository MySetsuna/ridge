#!/usr/bin/env node
// Generate the remote PWA icon set from the Ridge mark.
//
// Renders a crisper, higher-contrast variant of static/ridge-mark.svg onto a
// solid dark (#0d1117) background at the sizes the web app manifest +
// apple-touch-icon need, writing them to src/remote/public/ so the remote Vite
// build (vite.remote.config.js) copies them to static/remote/ root.
//
//   node scripts/gen-remote-icons.mjs

import sharp from 'sharp';
import path from 'node:path';
import { mkdirSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(__dirname, '..');
const outDir = path.resolve(root, 'src/remote/public');
mkdirSync(outDir, { recursive: true });

// Brand background (matches manifest background_color / app shell).
const BG = { r: 0x0d, g: 0x11, b: 0x17, alpha: 1 };

// The mark, with thicker strokes + stronger fills so it stays legible when
// scaled down to a launcher icon. Transparent background — composited onto BG.
const markSvg = `<svg viewBox="0 0 32 32" fill="none" xmlns="http://www.w3.org/2000/svg">
  <rect x="2.5" y="2.5" width="27" height="27" rx="6" stroke="#7fb069" stroke-width="2.4"/>
  <line x1="16" y1="3.5" x2="16" y2="28.5" stroke="#7fb069" stroke-width="2.4"/>
  <line x1="3.5" y1="16" x2="28.5" y2="16" stroke="#7fb069" stroke-width="2.4"/>
  <rect x="4.5" y="4.5" width="9.5" height="9.5" rx="2" fill="#7fb069" fill-opacity="0.34"/>
  <rect x="18" y="18" width="9.5" height="9.5" rx="2" fill="#d97757" fill-opacity="0.42"/>
</svg>`;

/**
 * @param {number} size   output square size in px
 * @param {number} scale  fraction of `size` the mark occupies (safe zone for maskable)
 * @param {string} file   output filename
 */
async function emit(size, scale, file) {
  const markPx = Math.round(size * scale);
  const mark = await sharp(Buffer.from(markSvg))
    .resize(markPx, markPx, { fit: 'contain', background: { r: 0, g: 0, b: 0, alpha: 0 } })
    .png()
    .toBuffer();
  await sharp({ create: { width: size, height: size, channels: 4, background: BG } })
    .composite([{ input: mark, gravity: 'center' }])
    .png()
    .toFile(path.join(outDir, file));
  console.log(`[icons] wrote ${file} (${size}px, mark ${markPx}px)`);
}

await Promise.all([
  emit(192, 0.64, 'icon-192.png'),
  emit(512, 0.64, 'icon-512.png'),
  // Maskable: keep the mark inside the ~80% safe zone so launchers can crop.
  emit(512, 0.54, 'icon-maskable-512.png'),
  // Apple touch icon (iOS rounds it for us) — slightly larger mark reads well.
  emit(180, 0.66, 'apple-touch-icon.png'),
  emit(48, 0.72, 'favicon.png'),
]);

console.log('[icons] done →', path.relative(root, outDir));
