#!/usr/bin/env node
/**
 * OP-USER-RAIL: code-side gates for production status + smoke evidence.
 * Fail-closed without real credentials; does not claim physical smoke passed.
 *
 * Usage:
 *   node scripts/check-user-rail-gates.mjs
 *   RIDGE_ARTIFACT_TOKEN=... RIDGE_CLOUD_BASE=... node scripts/check-user-rail-gates.mjs --live
 */
import { existsSync, readFileSync } from 'node:fs';
import { resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const root = resolve(__dirname, '..');

const requiredDocs = [
  'docs/plans/user-verification-checklist.md',
  'docs/plans/cloud-remote-physical-smoke-runbook.md',
  'scripts/check-prod-status.mjs',
  'scripts/validate-remote-smoke-evidence.mjs',
  'scripts/check-desktop-only-hosts.mjs',
  'src-tauri/src/hosts/outbound.rs',
  'src-tauri/src/hosts/lan_transport.rs',
  'src-tauri/src/hosts/desktop_surface.rs',
  'packages/ridge-core/src/process_guard.rs',
  'src/lib/stores/gitGuardStats.ts',
];

let failed = 0;
function ok(msg) {
  console.log(`OK  ${msg}`);
}
function bad(msg) {
  console.error(`FAIL ${msg}`);
  failed += 1;
}

for (const rel of requiredDocs) {
  const p = resolve(root, rel);
  if (existsSync(p)) ok(`artifact present: ${rel}`);
  else bad(`missing required artifact: ${rel}`);
}

// Fake credentials must fail closed (no network claim of success).
const token = process.env.RIDGE_ARTIFACT_TOKEN || '';
const live = process.argv.includes('--live');
if (!live) {
  if (!token || token === 'fake' || token === 'test') {
    ok('no live token: fail-closed path (expected for CI / local without secrets)');
  } else {
    ok('token present but --live not set: skipping network (safe default)');
  }
} else {
  if (!token || token === 'fake') {
    bad('--live requires real RIDGE_ARTIFACT_TOKEN (not fake)');
  } else {
    ok('--live mode: caller must run check-prod-status.mjs separately with real env');
  }
}

// Evidence schema example must validate structure when present.
const example = resolve(root, 'docs/plans/examples/remote-smoke-evidence.example.json');
if (existsSync(example)) {
  try {
    const j = JSON.parse(readFileSync(example, 'utf8'));
    if (j && typeof j === 'object') ok('smoke evidence example parses as JSON object');
    else bad('smoke evidence example not an object');
  } catch (e) {
    bad(`smoke evidence example invalid JSON: ${e.message}`);
  }
} else {
  // non-fatal: schema may live elsewhere
  ok('no example json (optional)');
}

if (failed > 0) {
  console.error(`\ncheck-user-rail-gates: ${failed} failure(s)`);
  process.exit(1);
}
console.log('\ncheck-user-rail-gates: all code-side gates ok');
process.exit(0);
