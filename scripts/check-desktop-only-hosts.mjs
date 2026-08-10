#!/usr/bin/env node
/**
 * OP-CAP-PARITY / OP-PROTO-DOC: desktop-only hosts methods must not appear
 * in ridge-core REMOTE_ALLOWLIST or TS REMOTE_ALLOWLIST mirror.
 */
import { readFileSync } from 'node:fs';
import { resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
export const DESKTOP_ONLY_HOST_METHODS = [
  'host_list_snapshot', 'connect_host', 'disconnect_host', 'forget_host',
  'attach_host_session', 'detach_host_session', 'list_host_sessions',
  'inject_host_output', 'get_outbound_stats',
];

export function validateDesktopOnlyHosts({ desktop, rustAllow, tsAllow, io = console } = {}) {
  const methods = [...desktop.matchAll(/"([a-z_]+)"/g)]
    .map((m) => m[1]).filter((m) => DESKTOP_ONLY_HOST_METHODS.includes(m));
  let failed = 0;
  for (const m of methods) {
    if (rustAllow.includes(`"${m}"`)) { io.error(`FAIL rust REMOTE_ALLOWLIST contains desktop-only ${m}`); failed++; }
    else io.log(`OK  rust deny ${m}`);
    if (tsAllow.includes(`'${m}'`) || tsAllow.includes(`"${m}"`)) { io.error(`FAIL ts REMOTE_ALLOWLIST contains desktop-only ${m}`); failed++; }
    else io.log(`OK  ts deny ${m}`);
  }
  if (methods.length < 5) { io.error('FAIL expected >=5 desktop-only host methods'); failed++; }
  return { failed, methods };
}

export function main(repoRoot = root, io = console) {
  const read = (rel) => readFileSync(resolve(repoRoot, rel), 'utf8');
  const result = validateDesktopOnlyHosts({
    desktop: read('src-tauri/src/hosts/desktop_surface.rs'),
    rustAllow: read('packages/ridge-core/src/capability.rs'),
    tsAllow: read('packages/remote/src/shared/cloud/remoteAllowlist.ts'),
    io,
  });
  if (result.failed) io.error(`check-desktop-only-hosts: ${result.failed} failure(s)`);
  else io.log('check-desktop-only-hosts: ok');
  return result.failed ? 1 : 0;
}

if (process.argv[1] && resolve(process.argv[1]) === resolve(fileURLToPath(import.meta.url))) {
  process.exit(main());
}
