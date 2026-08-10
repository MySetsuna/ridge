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

export const TEAMMATE_REMOTE_REQUIRED = [
  'get_teammate_topology',
  'list_hitl_pending',
  'list_hitl_audit_remote',
  'resolve_hitl_remote',
  'get_orchestration_health',
];

export const DESKTOP_HOST_FORBIDDEN = [
  'connect_host',
  'attach_host_session',
  'detach_host_session',
  'pump_host_output',
  'step_host_reconnect',
  'cancel_host_reconnect',
  'get_outbound_stats',
  'bind_mock_outbound_and_list',
];

export function validateCapabilityMatrix({ matrix, rustAllow, io = console } = {}) {
  const teammate = matrix?.capabilities?.teammate?.methods ?? [];
  let failed = 0;
  for (const m of TEAMMATE_REMOTE_REQUIRED) {
    if (!teammate.includes(m)) { io.error(`FAIL matrix teammate missing ${m}`); failed++; }
    else io.log(`OK  matrix teammate has ${m}`);
  }
  for (const m of DESKTOP_HOST_FORBIDDEN) {
    if (teammate.includes(m)) { io.error(`FAIL matrix teammate leaks desktop host ${m}`); failed++; }
    const idx = rustAllow.indexOf('REMOTE_ALLOWLIST');
    const slice = rustAllow.slice(idx, idx + 8000);
    if (slice.includes(`"${m}"`)) { io.error(`FAIL rust REMOTE_ALLOWLIST contains ${m}`); failed++; }
    else io.log(`OK  rust deny ${m}`);
  }
  return failed;
}

export function main(repoRoot = root, io = console) {
  const matrix = JSON.parse(readFileSync(resolve(repoRoot, 'docs/capability-matrix.json'), 'utf8'));
  const rustAllow = readFileSync(resolve(repoRoot, 'packages/ridge-core/src/capability.rs'), 'utf8');
  const failed = validateCapabilityMatrix({ matrix, rustAllow, io });
  if (failed) io.error(`check-capability-matrix: ${failed} failure(s)`);
  else io.log('check-capability-matrix: ok');
  return failed ? 1 : 0;
}

const isMain = process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url);
if (isMain) process.exitCode = main();
