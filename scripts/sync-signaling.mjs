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
import { gitTool } from './lib/toolPath.mjs';
import { rm, mkdir, cp, readdir, writeFile } from 'node:fs/promises';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');

export const srcRepo =
  process.env.RIDGE_SIGNALING_REPO || resolve(root, '..', 'ridge-signaling');

// wind vendor landing point.
const DEST = join(root, 'packages', 'remote', 'src', 'shared', 'cloud', 'signaling');
const GENERATED = join(DEST, 'generated');
const FIXTURES = join(DEST, 'fixtures');
const TERMINAL_FIXTURES = join(FIXTURES, 'terminal-v2');
const GENERATED_RUST = join(root, 'packages', 'ridge-term', 'src', 'remote_protocol_generated.rs');

/** Generated bindings to vendor (relative to `bindings/`), preserving subdirs. */
const BINDING_FILES = [
  'SignalMsg.ts',
  'Role.ts',
  'PaneRef.ts',
  'ActivateTerminalParams.ts',
  'TerminalHello.ts',
  'PointerAction.ts',
  'PointerEvent.ts',
  join('serde_json', 'JsonValue.ts'),
];

export function sourceError(repo = srcRepo, exists = existsSync) {
  if (!exists(repo)) return `[sync-signaling] ridge-signaling repo not found at ${repo}.\nCheck it out as a sibling of wind, or set RIDGE_SIGNALING_REPO to its path.`;
  if (!exists(join(repo, 'bindings')) || !exists(join(repo, 'fixtures', 'signaling')) || !exists(join(repo, 'src', 'terminal_v2.rs'))) return `[sync-signaling] ${repo} is missing bindings/, fixtures/, or src/terminal_v2.rs.\nRegenerate them in ridge-signaling first (ts-rs export + fixtures).`;
  return null;
}

export async function main({ repo = srcRepo, exists = existsSync, io = console } = {}) {
  const error = sourceError(repo, exists);
  if (error) { io.error(error); return false; }
  const sourceBindings = join(repo, 'bindings');
  const sourceFixtures = join(repo, 'fixtures', 'signaling');
  const sourceTerminalFixtures = join(repo, 'fixtures', 'terminal-v2');

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

  // 3) Terminal v2 Rust wire module + golden bytes. ridge-term is compiled on
  // both native and wasm targets, so this exact generated source is the codec
  // used by the host and mobile kernel.
  await cp(join(repo, 'src', 'terminal_v2.rs'), GENERATED_RUST);
  await mkdir(TERMINAL_FIXTURES, { recursive: true });
  const terminalFixtureNames = exists(sourceTerminalFixtures)
    ? (await readdir(sourceTerminalFixtures)).filter((f) => f.endsWith('.hex')).sort()
    : [];
  for (const name of terminalFixtureNames) {
    await cp(join(sourceTerminalFixtures, name), join(TERMINAL_FIXTURES, name));
  }

  // 4) SOURCE_REV: record the source commit (single line, no newline noise).
  const rev = execFileSync(gitTool(), ['rev-parse', 'HEAD'], {
    cwd: repo,
    encoding: 'utf8',
  }).trim();
  await writeFile(join(DEST, 'SOURCE_REV'), rev, 'utf8');

  io.log(
    `[sync-signaling] vendored ${BINDING_FILES.length} bindings + ${fixtureNames.length + terminalFixtureNames.length} fixtures + terminal v2 Rust codec\n` +
      `  from ${repo}\n` +
      `  → ${DEST}\n` +
      `  SOURCE_REV = ${rev}`,
  );
  return true;
}

if (process.argv[1]?.endsWith('sync-signaling.mjs')) {
  try {
    process.exit((await main()) ? 0 : 1);
  } catch (e) {
    console.error('[sync-signaling] failed:', e instanceof Error ? e.message : e);
    process.exit(1);
  }
}
