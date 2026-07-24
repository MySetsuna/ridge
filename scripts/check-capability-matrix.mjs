#!/usr/bin/env node
/**
 * C9 product path: validate docs/capability-matrix.json against
 * TEAMMATE_REMOTE_REQUIRED and desktop-host forbidden set (parity with
 * ridge-core capability_matrix_guard).
 */
import { readFileSync } from 'node:fs';
import { resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');

const TEAMMATE_REMOTE_REQUIRED = [
  'get_teammate_topology',
  'list_hitl_pending',
  'list_hitl_audit_remote',
  'resolve_hitl_remote',
  'get_orchestration_health',
];

const DESKTOP_HOST_FORBIDDEN = [
  'connect_host',
  'attach_host_session',
  'detach_host_session',
  'pump_host_output',
  'step_host_reconnect',
  'cancel_host_reconnect',
  'get_outbound_stats',
  'bind_mock_outbound_and_list',
];

const matrix = JSON.parse(
  readFileSync(resolve(root, 'docs/capability-matrix.json'), 'utf8'),
);
const teammate = matrix?.capabilities?.teammate?.methods ?? [];
const rustAllow = readFileSync(
  resolve(root, 'packages/ridge-core/src/capability.rs'),
  'utf8',
);

let failed = 0;
for (const m of TEAMMATE_REMOTE_REQUIRED) {
  if (!teammate.includes(m)) {
    console.error(`FAIL matrix teammate missing ${m}`);
    failed++;
  } else {
    console.log(`OK  matrix teammate has ${m}`);
  }
}
for (const m of DESKTOP_HOST_FORBIDDEN) {
  if (teammate.includes(m)) {
    console.error(`FAIL matrix teammate leaks desktop host ${m}`);
    failed++;
  }
  if (rustAllow.includes(`"${m}"`)) {
    // REMOTE_ALLOWLIST must not include desktop host methods
    // (allowlist is a list of strings — crude scan)
    const re = new RegExp(`"${m}"`);
    // Only flag if inside REMOTE_ALLOWLIST region roughly
    const idx = rustAllow.indexOf('REMOTE_ALLOWLIST');
    const slice = rustAllow.slice(idx, idx + 8000);
    if (slice.includes(`"${m}"`)) {
      console.error(`FAIL rust REMOTE_ALLOWLIST contains ${m}`);
      failed++;
    } else {
      console.log(`OK  rust deny ${m}`);
    }
  } else {
    console.log(`OK  rust deny ${m}`);
  }
}

if (failed) {
  console.error(`check-capability-matrix: ${failed} failure(s)`);
  process.exit(1);
}
console.log('check-capability-matrix: ok');
process.exit(0);
