#!/usr/bin/env node
/** Deterministic post-build checks for the browser/PWA Remote shell. */
import { existsSync, mkdirSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const DIST = resolve(process.env.RIDGE_REMOTE_DIST || join(ROOT, 'remote-dist', 'mobile'));
const OUTPUT = resolve(
  process.env.RIDGE_PWA_EVIDENCE || join(ROOT, '.iteration', 'artifacts', 'remote-pwa-build.json'),
);

function read(name) {
  const path = join(DIST, name);
  if (!existsSync(path)) throw new Error(`missing Remote PWA artifact: ${path}`);
  return readFileSync(path, 'utf8');
}

function check(condition, message) {
  if (!condition) throw new Error(message);
}

function listFiles(root, out = []) {
  // The emitted directory is flat enough for normal assets, but recurse so a
  // future split chunk cannot reintroduce an in-app install hook unnoticed.
  for (const name of readdirSync(root, { withFileTypes: true })) {
    const path = join(root, name.name);
    if (name.isDirectory()) listFiles(path, out);
    else out.push(path);
  }
  return out;
}

const index = read('index.html');
const manifest = JSON.parse(read('manifest.webmanifest'));
const serviceWorker = read('sw.js');
const requiredIcons = ['icon-192.png', 'icon-512.png', 'icon-maskable-512.png'];
const checks = {
  indexViewportFitCover: /<meta\s+name=["']viewport["'][^>]*viewport-fit=cover/i.test(index),
  manifestStandalone: manifest.display === 'standalone',
  manifestScopeRoot: manifest.scope === '/' && manifest.start_url === '/',
  manifestIconsPresent: requiredIcons.every((icon) => existsSync(join(DIST, icon))),
  serviceWorkerGenerated: /precacheAndRoute|precache|workbox/i.test(serviceWorker),
  safeAreaCssPresent: false,
  noInAppInstallHook: true,
};
const assetText = listFiles(DIST)
  .filter((path) => /\.(?:js|css|html)$/i.test(path))
  .map((path) => readFileSync(path, 'utf8'))
  .join('\n');
checks.safeAreaCssPresent = /safe-area-inset-(?:top|bottom)/.test(assetText);
checks.noInAppInstallHook = !/(beforeinstallprompt|PwaInstallAction|deferredPrompt)/.test(assetText);

for (const [name, ok] of Object.entries(checks)) check(ok, `Remote PWA build check failed: ${name}`);

const evidence = {
  ok: true,
  dist: DIST,
  checks,
  manifest: {
    name: manifest.name,
    short_name: manifest.short_name,
    display: manifest.display,
    scope: manifest.scope,
    start_url: manifest.start_url,
    icons: manifest.icons?.map((icon) => ({ src: icon.src, sizes: icon.sizes })),
  },
};
mkdirSync(dirname(OUTPUT), { recursive: true });
writeFileSync(OUTPUT, `${JSON.stringify(evidence, null, 2)}\n`);
console.log(JSON.stringify({ evidence: OUTPUT, ...checks }));
