#!/usr/bin/env node
/**
 * OP-CAP-PARITY / OP-PROTO-DOC: desktop-only hosts methods must not appear
 * in ridge-core REMOTE_ALLOWLIST or TS REMOTE_ALLOWLIST mirror.
 */
import { readFileSync } from 'node:fs';
import { resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');

function read(rel) {
  return readFileSync(resolve(root, rel), 'utf8');
}

const desktop = read('src-tauri/src/hosts/desktop_surface.rs');
const methods = [...desktop.matchAll(/"([a-z_]+)"/g)]
  .map((m) => m[1])
  .filter((m) =>
    [
      'host_list_snapshot',
      'connect_host',
      'disconnect_host',
      'forget_host',
      'attach_host_session',
      'detach_host_session',
      'list_host_sessions',
      'inject_host_output',
      'get_outbound_stats',
    ].includes(m),
  );

const rustAllow = read('packages/ridge-core/src/capability.rs');
const tsAllow = read('packages/remote/src/shared/cloud/remoteAllowlist.ts');

let failed = 0;
for (const m of methods) {
  if (rustAllow.includes(`"${m}"`)) {
    console.error(`FAIL rust REMOTE_ALLOWLIST contains desktop-only ${m}`);
    failed++;
  } else {
    console.log(`OK  rust deny ${m}`);
  }
  if (tsAllow.includes(`'${m}'`) || tsAllow.includes(`"${m}"`)) {
    console.error(`FAIL ts REMOTE_ALLOWLIST contains desktop-only ${m}`);
    failed++;
  } else {
    console.log(`OK  ts deny ${m}`);
  }
}

if (methods.length < 5) {
  console.error('FAIL expected >=5 desktop-only host methods');
  failed++;
}

if (failed) {
  console.error(`check-desktop-only-hosts: ${failed} failure(s)`);
  process.exit(1);
}
console.log('check-desktop-only-hosts: ok');
process.exit(0);
