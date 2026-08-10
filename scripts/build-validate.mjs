#!/usr/bin/env node
// Build artifact validation: check that index.html and ridge.theme
// are present and well-formed BEFORE bundling. Called as part of CI
// or manually after a build.
//
// Usage:
//   node scripts/build-validate.mjs               # validate everything
//   node scripts/build-validate.mjs --html-only    # skip theme check
//   node scripts/build-validate.mjs --theme-only   # skip html check
//
// Exit code: 0 = pass, 1 = fail.

import { existsSync, readFileSync } from 'node:fs';
import { resolve, dirname } from 'path';
import { fileURLToPath } from 'url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = resolve(__dirname, '..');

export function validateBuildArtifacts({
  root = REPO_ROOT,
  args = [],
  fs = { existsSync, readFileSync },
  io = console,
} = {}) {
  const checkHtml = !args.includes('--theme-only');
  const checkTheme = !args.includes('--html-only');
  let exitCode = 0;
  const fail = (msg) => { io.error(`  FAIL  ${msg}`); exitCode = 1; };
  const pass = (msg) => io.log(`  PASS  ${msg}`);

  if (checkHtml) {
    const buildDir = resolve(root, 'build');
    const indexPath = resolve(buildDir, 'index.html');
    if (!fs.existsSync(buildDir)) fail(`build/ directory not found at ${buildDir}`);
    else pass(`build/ directory exists at ${buildDir}`);
    if (!fs.existsSync(indexPath)) fail(`build/index.html not found at ${indexPath}`);
    else {
      const html = fs.readFileSync(indexPath, 'utf-8');
      if (html.length === 0) fail('build/index.html is empty');
      else pass(`build/index.html exists (${html.length} bytes)`);
      if (html.includes('__RIDGE_BOOT_LOADER__')) pass('index.html has splash boot script (reads __RIDGE_BOOT_* from init_script injection)');
      else fail('index.html missing __RIDGE_BOOT_LOADER__ splash boot code');
      if (html.includes('dismissBrandLoader')) pass('index.html has splash dismiss logic (dismissBrandLoader)');
      else fail('index.html missing dismissBrandLoader splash dismiss function');
      if (html.includes('brand-loader')) pass('index.html has SVG splash loader (#brand-loader)');
      else fail('index.html missing #brand-loader SVG element');
      if (html.includes('data-sveltekit')) pass('index.html has SvelteKit data attributes');
      else fail('index.html missing data-sveltekit attributes (may not be a valid SvelteKit build)');
      if (html.includes('<html')) pass('index.html has valid HTML structure');
      else fail('build/index.html missing <html> tag');
    }
  }

  if (checkTheme) {
    const themePath = resolve(root, 'ridge.theme');
    if (!fs.existsSync(themePath)) fail(`ridge.theme not found at ${themePath}`);
    else {
      const raw = fs.readFileSync(themePath, 'utf-8');
      pass(`ridge.theme exists (${raw.length} bytes)`);
      try {
        const theme = JSON.parse(raw);
        if (typeof theme.version !== 'number' || theme.version < 1) fail(`ridge.theme version is ${theme.version}, expected >= 1`);
        else pass(`ridge.theme version: ${theme.version}`);
        if (!Array.isArray(theme.themes)) fail('ridge.theme.themes is not an array');
        else if (theme.themes.length === 0) fail('ridge.theme has no theme entries (themes array is empty)');
        else {
          pass(`ridge.theme has ${theme.themes.length} theme(s)`);
          for (const [i, t] of theme.themes.entries()) {
            const label = t.id || t.label || `#${i}`;
            if (!t.id || !t.label || !t.loader || !t.colors) fail(`theme[${i}] "${label}" missing required fields (id, label, loader, colors)`);
            else {
              if (!t.loader.primary || !t.loader.secondary) fail(`theme[${i}] "${label}" loader missing primary/secondary`);
              if (!t.colors.bg) fail(`theme[${i}] "${label}" colors missing "bg"`);
            }
          }
        }
      } catch (e) { fail(`ridge.theme is not valid JSON: ${e.message}`); }
    }
  }
  return exitCode;
}

export function main(args = process.argv.slice(2), root = REPO_ROOT, io = console) {
  io.log('', '─'.repeat(50), '  Build artifact validation', '─'.repeat(50), '');
  const exitCode = validateBuildArtifacts({ root, args, io });
  io.log('', '─'.repeat(50), exitCode === 0 ? '  ✓ All checks passed' : '  ✗ Some checks failed', '─'.repeat(50), '');
  return exitCode;
}

if (process.argv[1] && resolve(process.argv[1]) === resolve(fileURLToPath(import.meta.url))) {
  process.exit(main());
}
