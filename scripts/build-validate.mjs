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

import { existsSync, readFileSync } from 'fs';
import { resolve, dirname } from 'path';
import { fileURLToPath } from 'url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = resolve(__dirname, '..');

const args = new Set(process.argv.slice(2));
const CHECK_HTML = !args.has('--theme-only');
const CHECK_THEME = !args.has('--html-only');

let exitCode = 0;

function fail(msg) {
  console.error(`  FAIL  ${msg}`);
  exitCode = 1;
}

function pass(msg) {
  console.log(`  PASS  ${msg}`);
}

console.log('');
console.log('─'.repeat(50));
console.log('  Build artifact validation');
console.log('─'.repeat(50));
console.log('');

// ── 1. index.html ────────────────────────────────────────────────────────

if (CHECK_HTML) {
  const buildDir = resolve(REPO_ROOT, 'build');
  const indexPath = resolve(buildDir, 'index.html');

  if (!existsSync(buildDir)) {
    fail(`build/ directory not found at ${buildDir}`);
  } else {
    pass(`build/ directory exists at ${buildDir}`);
  }

  if (!existsSync(indexPath)) {
    fail(`build/index.html not found at ${indexPath}`);
  } else {
    const html = readFileSync(indexPath, 'utf-8');
    if (html.length === 0) {
      fail('build/index.html is empty');
    } else {
      pass(`build/index.html exists (${html.length} bytes)`);
    }

    // Check that the splash boot inline script is included.
    if (html.includes('__RIDGE_BOOT_LOADER__')) {
      pass('index.html has splash boot script (reads __RIDGE_BOOT_* from init_script injection)');
    } else {
      fail('index.html missing __RIDGE_BOOT_LOADER__ splash boot code');
    }

    if (html.includes('dismissBrandLoader')) {
      pass('index.html has splash dismiss logic (dismissBrandLoader)');
    } else {
      fail('index.html missing dismissBrandLoader splash dismiss function');
    }

    if (html.includes('brand-loader')) {
      pass('index.html has SVG splash loader (#brand-loader)');
    } else {
      fail('index.html missing #brand-loader SVG element');
    }

    if (!html.includes('data-sveltekit')) {
      fail('index.html missing data-sveltekit attributes (may not be a valid SvelteKit build)');
    } else {
      pass('index.html has SvelteKit data attributes');
    }

    if (!html.includes('<html')) {
      fail('build/index.html missing <html> tag');
    } else {
      pass('index.html has valid HTML structure');
    }
  }
}

// ── 2. ridge.theme ───────────────────────────────────────────────────────

if (CHECK_THEME) {
  const themePath = resolve(REPO_ROOT, 'ridge.theme');

  if (!existsSync(themePath)) {
    fail(`ridge.theme not found at ${themePath}`);
  } else {
    const raw = readFileSync(themePath, 'utf-8');
    pass(`ridge.theme exists (${raw.length} bytes)`);

    try {
      const theme = JSON.parse(raw);

      if (typeof theme.version !== 'number' || theme.version < 1) {
        fail(`ridge.theme version is ${theme.version}, expected >= 1`);
      } else {
        pass(`ridge.theme version: ${theme.version}`);
      }

      if (!Array.isArray(theme.themes)) {
        fail('ridge.theme.themes is not an array');
      } else if (theme.themes.length === 0) {
        fail('ridge.theme has no theme entries (themes array is empty)');
      } else {
        pass(`ridge.theme has ${theme.themes.length} theme(s)`);

        // Validate each theme entry has required fields.
        for (const [i, t] of theme.themes.entries()) {
          const label = t.id || t.label || `#${i}`;
          if (!t.id || !t.label || !t.loader || !t.colors) {
            fail(`theme[${i}] "${label}" missing required fields (id, label, loader, colors)`);
          } else {
            if (!t.loader.primary || !t.loader.secondary) {
              fail(`theme[${i}] "${label}" loader missing primary/secondary`);
            }
            if (!t.colors.bg) {
              fail(`theme[${i}] "${label}" colors missing "bg"`);
            }
          }
        }
      }
    } catch (e) {
      fail(`ridge.theme is not valid JSON: ${e.message}`);
    }
  }
}

// ── Summary ──────────────────────────────────────────────────────────────

console.log('');
console.log('─'.repeat(50));
if (exitCode === 0) {
  console.log('  ✓ All checks passed');
} else {
  console.log('  ✗ Some checks failed');
}
console.log('─'.repeat(50));
console.log('');

process.exit(exitCode);
