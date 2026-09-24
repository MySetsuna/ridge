#!/usr/bin/env node
// scripts/security-totp-rotate.mjs
//
// §security-totp-rotate (2026-09-25): rotate the DPAPI-encrypted TOTP seed for
// the "default" identity (file name hex(sha256("default")[:16])).
//
// What this does:
//   1. Generate 20 fresh random bytes (RFC 6238 standard 160-bit secret).
//   2. DPAPI-encrypt with CurrentUser scope via System.Security.
//   3. Atomically replace %APPDATA%\ridge\config\totp\<identity>.seed.
//   4. Zero the in-memory plaintext buffer.
//
// What this does NOT do (hard constraints from /goal directive):
//   - Does NOT print, log, or persist the new seed value to stdout / report.
//   - Does NOT change the auth protocol (RFC 6238 + DPAPI CurrentUser).
//   - Does NOT touch system trust / CA.
//   - Does NOT push / tag / release / deploy.
//
// The rotation invalidates every prior pairing of the "default" identity — that
// is intentional per user authorization (2026-09-25): "允许旧配对失效".
//
// Output: one status line only ("rotated: identity=… bytes=…"). No seed hex,
// no seed digest. Do not extend the output.

import { spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, renameSync, unlinkSync, writeFileSync, readFileSync, mkdtempSync } from 'node:fs';
import { randomBytes } from 'node:crypto';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const IDENTITY = process.env.RIDGE_TOTP_IDENTITY ?? 'default';
// File name is hex(sha256(identity)[:16]) — for "default" this is fixed.
// The identity name is not secret; the seed value is. Never echo the seed.
const IDENTITY_HEX = '37a8eec1ce19687d';
const SEED_DIR = `${process.env.APPDATA}/ridge/config/totp`;
const SEED_FILE = `${SEED_DIR}/${IDENTITY_HEX}.seed`;

function die(msg) {
  // Failure path: report only the error; never echo any plaintext secret.
  console.error(`rotate-failed: ${msg}`);
  process.exit(1);
}

// 1. Generate 20 fresh random bytes in a Node Buffer. Never printed.
const plaintext = randomBytes(20);
if (plaintext.length !== 20) die('randomBytes(20) returned wrong length');

// 2. Hand plaintext to a temp PowerShell script via a binary stdin file, so
//    argv / env / shell never see the bytes. PowerShell reads exactly 20 bytes,
//    DPAPI-encrypts, writes ciphertext to a sibling temp file, then zeroes its
//    own buffer. The parent renames the ciphertext into place.
const tmpRoot = mkdtempSync(join(tmpdir(), 'ridge-totp-rotate-'));
const stdinFile = join(tmpRoot, 'in.bin');
const cipherFile = join(tmpRoot, 'out.bin');
const psFile = join(tmpRoot, 'protect.ps1');
writeFileSync(stdinFile, plaintext);

const psScript = `
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Security
$inPath  = [System.IO.Path]::GetFullPath('${stdinFile.replace(/\\/g, '/')}')
$outPath = [System.IO.Path]::GetFullPath('${cipherFile.replace(/\\/g, '/')}')
$buf = [System.IO.File]::ReadAllBytes($inPath)
if ($buf.Length -ne 20) { [Console]::Error.WriteLine('unexpected-plaintext-length'); exit 2 }
$enc = [System.Security.Cryptography.ProtectedData]::Protect($buf, $null, [System.Security.Cryptography.DataProtectionScope]::CurrentUser)
[System.IO.File]::WriteAllBytes($outPath, $enc)
for ($i = 0; $i -lt $buf.Length; $i++) { $buf[$i] = 0 }
[System.IO.File]::Delete($inPath)
exit 0
`;
writeFileSync(psFile, psScript);

// 3. Run the protection script. stdin / stdout are not used for secrets.
const proc = spawnSync(
  'powershell',
  ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', psFile],
  { encoding: 'utf8', timeout: 30_000, windowsHide: true },
);

// 4. Zero the plaintext buffer regardless of success.
for (let i = 0; i < plaintext.length; i += 1) plaintext[i] = 0;

try {
  if (existsSync(stdinFile)) unlinkSync(stdinFile);
} catch { /* ignore */ }

if (proc.status !== 0) {
  try {
    if (existsSync(cipherFile)) unlinkSync(cipherFile);
    if (existsSync(psFile)) unlinkSync(psFile);
  } catch { /* ignore */ }
  die(`dpapi-protect exit=${proc.status} stderr=${String(proc.stderr || '').slice(0, 120)}`);
}

if (!existsSync(cipherFile)) {
  try { if (existsSync(psFile)) unlinkSync(psFile); } catch { /* ignore */ }
  die('dpapi-protect produced no ciphertext');
}

// 5. Atomic replace. The old seed is rotated away (not preserved) — per user
//    authorization, prior pairings are allowed to invalidate.
if (!existsSync(SEED_DIR)) mkdirSync(SEED_DIR, { recursive: true });
try {
  renameSync(cipherFile, SEED_FILE);
} catch (e) {
  try {
    if (existsSync(cipherFile)) unlinkSync(cipherFile);
    if (existsSync(psFile)) unlinkSync(psFile);
  } catch { /* ignore */ }
  die(`rename failed: ${String(e).slice(0, 120)}`);
}

try {
  if (existsSync(psFile)) unlinkSync(psFile);
} catch { /* ignore */ }

// 6. Status line — must not contain any seed material, plaintext or digest.
console.log(`rotated: identity=${IDENTITY} bytes=20 path=${IDENTITY_HEX}.seed`);
