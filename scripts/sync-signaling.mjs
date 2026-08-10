// scripts/sync-signaling.mjs
//
// Vendor the signaling SSOT (`ridge-signaling`) into wind as the TS side's
// single source of truth for the remote-control signaling protocol.
//
// `ridge-signaling` owns `SignalMsg`/`Role` (+ error codes) and emits ts-rs TS
// bindings + golden fixtures + Rust-side cross-language conformance. The Rust
// ends (ridge-cloud, ridge-cli) already `pub use ridge_signaling::*` at a locked
// rev. The two TS providers (ridgeCloudProvider.ts host / controllerCloudProvider.ts
// controller) used to HAND-WRITE their `SignalIn` type — a manual mirror that no
// test could catch drifting. This script closes that loop: it copies the
// generated bindings + fixtures into `src/lib/remote/cloud/signaling/` and records
// the source commit in `SOURCE_REV`, so the vendored copy is "locked" the same way
// the Rust side locks the crate rev. `drift.test.ts` then fails loudly if anyone
// regenerates ridge-signaling without re-running this sync.
//
// ridge-signaling repo path: env `RIDGE_SIGNALING_REPO`, else sibling
// `../ridge-signaling`. Missing repo → error + exit (checkout it first).

import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { dirname, resolve, join } from 'node:path';
import { existsSync } from 'node:fs';
import { rm, mkdir, cp, readdir, writeFile } from 'node:fs/promises';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');

export const srcRepo =
  process.env.RIDGE_SIGNALING_REPO || resolve(root, '..', 'ridge-signaling');

// wind vendor landing point.
const DEST = join(root, 'packages', 'remote', 'src', 'shared', 'cloud', 'signaling');
const GENERATED = join(DEST, 'generated');
const FIXTURES = join(DEST, 'fixtures');

/** Generated bindings to vendor (relative to `bindings/`), preserving subdirs. */
const BINDING_FILES = ['SignalMsg.ts', 'Role.ts', join('serde_json', 'JsonValue.ts')];

export function sourceError(repo = srcRepo, exists = existsSync) {
  if (!exists(repo)) return `[sync-signaling] ridge-signaling repo not found at ${repo}.\nCheck it out as a sibling of wind, or set RIDGE_SIGNALING_REPO to its path.`;
  if (!exists(join(repo, 'bindings')) || !exists(join(repo, 'fixtures', 'signaling'))) return `[sync-signaling] ${repo} is missing bindings/ or fixtures/signaling/.\nRegenerate them in ridge-signaling first (ts-rs export + fixtures).`;
  return null;
}

export async function main({ repo = srcRepo, exists = existsSync, io = console } = {}) {
  const error = sourceError(repo, exists);
  if (error) { io.error(error); return false; }
  const sourceBindings = join(repo, 'bindings');
  const sourceFixtures = join(repo, 'fixtures', 'signaling');

  // 1) generated/: clear then copy the fixed binding set (verbatim, keep ts-rs header).
  await rm(GENERATED, { recursive: true, force: true });
  await mkdir(join(GENERATED, 'serde_json'), { recursive: true });
  for (const rel of BINDING_FILES) {
    const from = join(sourceBindings, rel);
    if (!exists(from)) {
      throw new Error(`[sync-signaling] missing binding ${from} in ridge-signaling.`);
    }
    await cp(from, join(GENERATED, rel));
  }

  // 2) fixtures/: clear then mirror every *.json (clearing first drops files that
  //    were removed upstream, so the vendored set never lingers stale).
  await rm(FIXTURES, { recursive: true, force: true });
  await mkdir(FIXTURES, { recursive: true });
  const fixtureNames = (await readdir(sourceFixtures)).filter((f) => f.endsWith('.json')).sort();
  for (const name of fixtureNames) {
    await cp(join(sourceFixtures, name), join(FIXTURES, name));
  }

  // 3) SOURCE_REV: record the source commit (single line, no newline noise).
  const rev = execFileSync('git', ['rev-parse', 'HEAD'], {
    cwd: repo,
    encoding: 'utf8',
  }).trim();
  await writeFile(join(DEST, 'SOURCE_REV'), rev, 'utf8');

  io.log(
    `[sync-signaling] vendored ${BINDING_FILES.length} bindings + ${fixtureNames.length} fixtures\n` +
      `  from ${repo}\n` +
      `  → ${DEST}\n` +
      `  SOURCE_REV = ${rev}`,
  );
  return true;
}

if (process.argv[1] && process.argv[1].endsWith('sync-signaling.mjs')) {
  main().then((ok) => process.exit(ok ? 0 : 1)).catch((e) => { console.error('[sync-signaling] failed:', e instanceof Error ? e.message : e); process.exit(1); });
}
