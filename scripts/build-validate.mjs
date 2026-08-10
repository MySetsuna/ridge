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
import { resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = resolve(__dirname, '..');

function validateHtmlArtifact(root, fs, fail, pass) {
  const buildDir = resolve(root, 'build');
  const indexPath = resolve(buildDir, 'index.html');
  if (!fs.existsSync(buildDir)) fail(`build/ directory not found at ${buildDir}`);
  else pass(`build/ directory exists at ${buildDir}`);
  if (!fs.existsSync(indexPath)) {
    fail(`build/index.html not found at ${indexPath}`);
    return;
  }
  const html = fs.readFileSync(indexPath, 'utf-8');
  if (html.length === 0) fail('build/index.html is empty');
  else pass(`build/index.html exists (${html.length} bytes)`);
  const checks = [
    ['__RIDGE_BOOT_LOADER__', 'index.html has splash boot script (reads __RIDGE_BOOT_* from init_script injection)', 'index.html missing __RIDGE_BOOT_LOADER__ splash boot code'],
    ['dismissBrandLoader', 'index.html has splash dismiss logic (dismissBrandLoader)', 'index.html missing dismissBrandLoader splash dismiss function'],
    ['brand-loader', 'index.html has SVG splash loader (#brand-loader)', 'index.html missing #brand-loader SVG element'],
    ['data-sveltekit', 'index.html has SvelteKit data attributes', 'index.html missing data-sveltekit attributes (may not be a valid SvelteKit build)'],
    ['<html', 'index.html has valid HTML structure', 'build/index.html missing <html> tag'],
  ];
  for (const [needle, ok, missing] of checks) {
    if (html.includes(needle)) pass(ok);
    else fail(missing);
  }
}

function validateThemeEntry(theme, index, fail, pass) {
  const label = theme.id || theme.label || `#${index}`;
  if (!theme.id || !theme.label || !theme.loader || !theme.colors) {
    fail(`theme[${index}] "${label}" missing required fields (id, label, loader, colors)`);
    return;
  }
  if (!theme.loader.primary || !theme.loader.secondary) fail(`theme[${index}] "${label}" loader missing primary/secondary`);
  if (!theme.colors.bg) fail(`theme[${index}] "${label}" colors missing "bg"`);
}

function validateThemeArtifact(root, fs, fail, pass) {
  const themePath = resolve(root, 'ridge.theme');
  if (!fs.existsSync(themePath)) {
    fail(`ridge.theme not found at ${themePath}`);
    return;
  }
  const raw = fs.readFileSync(themePath, 'utf-8');
  pass(`ridge.theme exists (${raw.length} bytes)`);
  try {
    const theme = JSON.parse(raw);
    if (typeof theme.version !== 'number' || theme.version < 1) fail(`ridge.theme version is ${theme.version}, expected >= 1`);
    else pass(`ridge.theme version: ${theme.version}`);
    if (!Array.isArray(theme.themes)) {
      fail('ridge.theme.themes is not an array');
      return;
    }
    if (theme.themes.length === 0) {
      fail('ridge.theme has no theme entries (themes array is empty)');
      return;
    }
    pass(`ridge.theme has ${theme.themes.length} theme(s)`);
    theme.themes.forEach((entry, index) => validateThemeEntry(entry, index, fail, pass));
  } catch (e) {
    fail(`ridge.theme is not valid JSON: ${e.message}`);
  }
}

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

  if (checkHtml) validateHtmlArtifact(root, fs, fail, pass);
  if (checkTheme) validateThemeArtifact(root, fs, fail, pass);
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
