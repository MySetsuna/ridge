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

const srcRepo =
  process.env.RIDGE_SIGNALING_REPO || resolve(root, '..', 'ridge-signaling');

// wind vendor landing point.
const DEST = join(root, 'src', 'lib', 'remote', 'cloud', 'signaling');
const GENERATED = join(DEST, 'generated');
const FIXTURES = join(DEST, 'fixtures');

// Sources inside ridge-signaling.
const SRC_BINDINGS = join(srcRepo, 'bindings');
const SRC_FIXTURES = join(srcRepo, 'fixtures', 'signaling');

/** Generated bindings to vendor (relative to `bindings/`), preserving subdirs. */
const BINDING_FILES = ['SignalMsg.ts', 'Role.ts', join('serde_json', 'JsonValue.ts')];

async function main() {
  if (!existsSync(srcRepo)) {
    console.error(
      `[sync-signaling] ridge-signaling repo not found at ${srcRepo}.\n` +
        `Check it out as a sibling of wind, or set RIDGE_SIGNALING_REPO to its path.`,
    );
    process.exit(1);
  }
  if (!existsSync(SRC_BINDINGS) || !existsSync(SRC_FIXTURES)) {
    console.error(
      `[sync-signaling] ${srcRepo} is missing bindings/ or fixtures/signaling/.\n` +
        `Regenerate them in ridge-signaling first (ts-rs export + fixtures).`,
    );
    process.exit(1);
  }

  // 1) generated/: clear then copy the fixed binding set (verbatim, keep ts-rs header).
  await rm(GENERATED, { recursive: true, force: true });
  await mkdir(join(GENERATED, 'serde_json'), { recursive: true });
  for (const rel of BINDING_FILES) {
    const from = join(SRC_BINDINGS, rel);
    if (!existsSync(from)) {
      console.error(`[sync-signaling] missing binding ${from} in ridge-signaling.`);
      process.exit(1);
    }
    await cp(from, join(GENERATED, rel));
  }

  // 2) fixtures/: clear then mirror every *.json (clearing first drops files that
  //    were removed upstream, so the vendored set never lingers stale).
  await rm(FIXTURES, { recursive: true, force: true });
  await mkdir(FIXTURES, { recursive: true });
  const fixtureNames = (await readdir(SRC_FIXTURES)).filter((f) => f.endsWith('.json')).sort();
  for (const name of fixtureNames) {
    await cp(join(SRC_FIXTURES, name), join(FIXTURES, name));
  }

  // 3) SOURCE_REV: record the source commit (single line, no newline noise).
  const rev = execFileSync('git', ['rev-parse', 'HEAD'], {
    cwd: srcRepo,
    encoding: 'utf8',
  }).trim();
  await writeFile(join(DEST, 'SOURCE_REV'), rev, 'utf8');

  console.log(
    `[sync-signaling] vendored ${BINDING_FILES.length} bindings + ${fixtureNames.length} fixtures\n` +
      `  from ${srcRepo}\n` +
      `  → ${DEST}\n` +
      `  SOURCE_REV = ${rev}`,
  );
}

main().catch((e) => {
  console.error('[sync-signaling] failed:', e instanceof Error ? e.message : e);
  process.exit(1);
});
