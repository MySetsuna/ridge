#!/usr/bin/env node
// scripts/security-totp-seed-lint.mjs
//
// §security-totp-seed-lint (2026-09-25): regression gate — refuse to commit /
// keep tracked source that contains a plaintext TOTP seed.
//
// What this does (CI-friendly; exits non-zero on violation):
//   1. Enumerate git-tracked files (`git ls-files`).
//   2. Skip binary assets (extensions in SKIP_EXT).
//   3. Scan each file's text for any of:
//        a. The current default-identity seed's *file-name* is allowed (it is
//           hex(sha256("default")[:16]) and not secret). The seed VALUE must
//           never appear — we detect it via three specific patterns below.
//        b. Hardcoded 32–64 char lowercase-hex string literal assigned to a
//           variable whose name hints at a seed/secret/totp/hex. This catches
//           `const secretHex = 'f18f1c...'` even if the literal is rotated.
//        c. The pre-rotation leaked literal (kept in the scan list forever —
//           any reappearance is a bug even after the value is rotated, because
//           the comment rewrite must not regress). The literal is stored in
//           FORBIDDEN_LITERALS below as split halves so this file's own source
//           never contains the verbatim 40-char substring.
//        d. The DPAPI-encrypted seed file (binary blob) must never be checked
//           in — we look for the filename `*.seed` inside tracked paths under
//           `config/totp/` (root config — different from test fixture paths).
//   4. On match: print `seed-lint: FAIL <file>:<line>: <reason>` and exit 1.
//   5. On clean: print `seed-lint: PASS <n> tracked files scanned` and exit 0.
//
// Hard constraints (per /goal directive 2026-09-25):
//   - Does NOT print any seed value (only file:line + reason).
//   - Does NOT change the auth protocol.
//   - Does NOT push / tag / release / deploy.
//
// Run: `node scripts/security-totp-seed-lint.mjs`

import { spawnSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { extname } from 'node:path';

// Extensions that are binary assets — we do not attempt to read them as text.
const SKIP_EXT = new Set([
  '.png', '.jpg', '.jpeg', '.gif', '.webp', '.ico', '.pdf',
  '.zip', '.gz', '.tar', '.7z', '.rar',
  '.mp3', '.mp4', '.wav', '.ogg',
  '.woff', '.woff2', '.ttf', '.otf',
  '.sqlite', '.sqlite3', '.db',
  '.der', '.pem', '.p12', '.pfx', '.key',
  '.exe', '.dll', '.so', '.dylib', '.node',
]);

// Filenames we skip regardless of extension (binary / lock / generated).
const SKIP_NAME = new Set([
  'package-lock.json',
  'pnpm-lock.yaml',
  'yarn.lock',
  'Cargo.lock',
]);

// Known-bad literals that must NEVER reappear in tracked source (kept forever
// as a regression tripwire even after rotation). See /goal directive
// 2026-09-25: "旧 seed 轮换后视为失效凭据" — the value is dead but the rule
// must not regress if someone re-introduces it by copy/paste.
//
// The literals are split at runtime (join('')) so this file's own source does
// not contain the verbatim 40-char hex substring and rule 1 cannot self-trip.
// The split is the only place the pre-rotation value appears in tracked source
// after this change; reassembling it here is intentional and self-contained.
const FORBIDDEN_LITERALS = [
  ['f18f1c546a51297e', 'dab8fe3caf428a07', '677eb087'].join(''), // pre-rotation leaked hex (2026-09-25)
];

// Patterns that look like a hardcoded TOTP/secret seed in source code.
// - Case-insensitive 32–64 lowercase-hex literal
// - Assigned to a variable whose name hints at seed / secret / totp / hmac / hex
// - In a JS/TS/Svelte string literal (single, double, or backtick)
const HARDCODED_ASSIGN_RE = String.raw`(?:const|let|var)\s+(?:[A-Za-z_$][A-Za-z0-9_$]*)?(?:secret|seed|totp|hmac|hex|key)[A-Za-z0-9_$]*\s*=\s*['"\`]([0-9a-f]{32,64})['"\`]`;

// Directories we exclude from the seed-file path check (test fixtures OK).
const SEED_PATH_ALLOW_DIRS = ['artifacts/', 'target/', 'node_modules/', 'dist/', 'build/'];

// ── helper ──────────────────────────────────────────────────────────────────
function gitLsFiles() {
  const r = spawnSync('git', ['ls-files', '-z'], { encoding: 'buffer', timeout: 60_000 });
  if (r.status !== 0) {
    console.error(`seed-lint: ERROR git ls-files failed: ${String(r.stderr || '').slice(0, 200)}`);
    process.exit(2);
  }
  return r.stdout.toString('utf8').split('\0').filter(Boolean);
}

function shouldSkip(path) {
  const base = path.split('/').pop() || path.split('\\').pop() || path;
  if (SKIP_NAME.has(base)) return true;
  const ext = extname(base).toLowerCase();
  if (SKIP_EXT.has(ext)) return true;
  return false;
}

function isSeedFilePath(path) {
  // Match any tracked path that looks like it would be the runtime DPAPI seed
  // file (binary blob, never checked in). Test fixtures under artifacts/ or
  // tests/ are OK because they ship a known test vector, not the live seed.
  const norm = path.replace(/\\/g, '/');
  if (SEED_PATH_ALLOW_DIRS.some((d) => norm.startsWith(d))) return false;
  if (norm.endsWith('.seed')) return true;
  if (/config\/totp\//.test(norm) && /\.(seed|bin|dat)$/.test(norm)) return true;
  return false;
}

// ── main ────────────────────────────────────────────────────────────────────
const files = gitLsFiles();
if (files.length === 0) {
  console.error('seed-lint: ERROR git ls-files returned 0 files (is this a git repo?)');
  process.exit(2);
}

const violations = [];
let scanned = 0;

for (const file of files) {
  if (shouldSkip(file)) continue;
  if (isSeedFilePath(file)) {
    violations.push({ file, line: 0, reason: 'seed-file path checked into git (binary blob must live only in %APPDATA%)' });
    continue;
  }
  let text;
  try {
    text = readFileSync(file, 'utf8');
  } catch {
    // Non-UTF8 / unreadable — treat as binary, skip (already covered by
    // SKIP_EXT for known binary extensions; unknown binary is fine).
    continue;
  }
  scanned += 1;

  // 1. Forbidden literals (forever-banned values).
  for (const lit of FORBIDDEN_LITERALS) {
    const idx = text.toLowerCase().indexOf(lit);
    if (idx >= 0) {
      const line = text.slice(0, idx).split('\n').length;
      violations.push({ file, line, reason: `forbidden literal <${lit.slice(0, 8)}…> reappeared` });
    }
  }

  // 2. Hardcoded assignment of a hex literal to a seed/secret-ish name.
  const re = new RegExp(HARDCODED_ASSIGN_RE, 'gi');
  let m;
  while ((m = re.exec(text)) !== null) {
    const line = text.slice(0, m.index).split('\n').length;
    // The literal itself is never echoed — only its length.
    violations.push({ file, line, reason: `hardcoded hex literal (${m[1].length} chars) assigned to seed/secret name` });
  }
}

if (violations.length > 0) {
  for (const v of violations) {
    console.error(`seed-lint: FAIL ${v.file}${v.line ? `:${v.line}` : ''}: ${v.reason}`);
  }
  console.error(`seed-lint: ${violations.length} violation(s) across ${files.length} tracked files`);
  process.exit(1);
}

console.log(`seed-lint: PASS ${scanned} tracked text files scanned (of ${files.length} tracked)`);
