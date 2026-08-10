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

export const REQUIRED_DOCS = [
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

function checkRequiredDocs(rootDir, fsImpl, ok, bad) {
  for (const rel of REQUIRED_DOCS) {
    const present = fsImpl.existsSync(resolve(rootDir, rel));
    (present ? ok : bad)(present ? `artifact present: ${rel}` : `missing required artifact: ${rel}`);
  }
}

function checkCredentialMode(args, env, ok, bad) {
  const token = env.RIDGE_ARTIFACT_TOKEN || '';
  if (!args.includes('--live')) {
    const message = !token || token === 'fake' || token === 'test'
      ? 'no live token: fail-closed path (expected for CI / local without secrets)'
      : 'token present but --live not set: skipping network (safe default)';
    ok(message);
    return;
  }
  if (!token || token === 'fake') bad('--live requires real RIDGE_ARTIFACT_TOKEN (not fake)');
  else ok('--live mode: caller must run check-prod-status.mjs separately with real env');
}

function checkEvidenceExample(rootDir, fsImpl, ok, bad) {
  const example = resolve(rootDir, 'docs/plans/examples/remote-smoke-evidence.example.json');
  if (!fsImpl.existsSync(example)) {
    ok('no example json (optional)');
    return;
  }
  try {
    const value = JSON.parse(fsImpl.readFileSync(example, 'utf8'));
    if (value && typeof value === 'object') ok('smoke evidence example parses as JSON object');
    else bad('smoke evidence example not an object');
  } catch (error) {
    bad(`smoke evidence example invalid JSON: ${error.message}`);
  }
}

export function runGates({ rootDir = root, args = [], env = process.env, fsImpl = { existsSync, readFileSync }, io = console } = {}) {
  let failed = 0;
  const ok = (msg) => io.log(`OK  ${msg}`);
  const bad = (msg) => { io.error(`FAIL ${msg}`); failed += 1; };
  checkRequiredDocs(rootDir, fsImpl, ok, bad);
  checkCredentialMode(args, env, ok, bad);
  checkEvidenceExample(rootDir, fsImpl, ok, bad);
  if (failed) io.error(`\ncheck-user-rail-gates: ${failed} failure(s)`);
  else io.log('\ncheck-user-rail-gates: all code-side gates ok');
  return failed;
}

export function main(args = process.argv.slice(2), options = {}) {
  return runGates({ args, ...options });
}

if (process.argv[1] && resolve(process.argv[1]) === resolve(fileURLToPath(import.meta.url))) process.exit(main() ? 1 : 0);
