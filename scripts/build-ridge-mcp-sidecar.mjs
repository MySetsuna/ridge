#!/usr/bin/env node

import { execFileSync } from 'node:child_process';
import { copyFileSync, mkdirSync, readFileSync, chmodSync, statSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';

const root = resolve(import.meta.dirname, '..');
const args = process.argv.slice(2);
const check = args.includes('--check');
const requireBuilt = args.includes('--require-built');

function valueAfter(name) {
  const index = args.indexOf(name);
  if (index < 0) return '';
  const value = args[index + 1];
  if (!value || value.startsWith('--')) throw new Error(`${name} requires a value`);
  return value;
}

function hostTriple() {
  const output = execFileSync('rustc', ['-vV'], {
    cwd: root,
    encoding: 'utf8',
    timeout: 10_000,
  });
  const match = /^host:\s*(\S+)$/m.exec(output);
  if (!match) throw new Error('rustc -vV did not report a host triple');
  return match[1];
}

const target =
  valueAfter('--target') ||
  process.env.RIDGE_MCP_TARGET?.trim() ||
  process.env.CARGO_BUILD_TARGET?.trim() ||
  hostTriple();
const tauri = JSON.parse(readFileSync(join(root, 'src-tauri', 'tauri.conf.json'), 'utf8'));
const version = String(tauri.version ?? '').trim();
if (!version) throw new Error('src-tauri/tauri.conf.json has no version');

const externalBin = tauri.bundle?.externalBin;
if (!Array.isArray(externalBin) || !externalBin.includes('binaries/ridge-mcp')) {
  throw new Error('tauri bundle.externalBin must contain binaries/ridge-mcp');
}

const windows = target.includes('windows');
const extension = windows ? '.exe' : '';
const source = join(root, 'target', target, 'release', `ridge-mcp${extension}`);
const destination = join(root, 'src-tauri', 'binaries', `ridge-mcp-${target}${extension}`);

if (!check) {
  execFileSync(
    'cargo',
    ['build', '--release', '--package', 'ridge-mcp-bridge', '--bin', 'ridge-mcp', '--target', target],
    {
      cwd: root,
      env: { ...process.env, RIDGE_MCP_BUNDLE_VERSION: version },
      stdio: 'inherit',
      timeout: 15 * 60_000,
    },
  );
  mkdirSync(dirname(destination), { recursive: true });
  copyFileSync(source, destination);
  if (!windows) chmodSync(destination, 0o755);
}

if (requireBuilt) {
  const stat = statSync(destination);
  if (!stat.isFile() || stat.size === 0) throw new Error(`invalid sidecar: ${destination}`);
  if (!windows && (stat.mode & 0o111) === 0) throw new Error(`sidecar is not executable: ${destination}`);
  if (!readFileSync(destination).includes(Buffer.from(version))) {
    throw new Error(`sidecar does not embed Ridge version ${version}: ${destination}`);
  }
  if (target === hostTriple()) {
    const reported = execFileSync(destination, ['--version'], {
      encoding: 'utf8',
      timeout: 10_000,
    }).trim();
    if (!reported.endsWith(` ${version}`)) {
      throw new Error(`sidecar version mismatch: expected ${version}, got ${reported}`);
    }
  }
}

console.log(
  JSON.stringify({
    ok: true,
    mode: check ? 'check' : 'build',
    target,
    version,
    externalBin: 'binaries/ridge-mcp',
    destination,
    requireBuilt,
  }),
);
