#!/usr/bin/env node

import { execFileSync } from 'node:child_process';
import { copyFileSync, mkdirSync, chmodSync, statSync, readFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';

const root = resolve(import.meta.dirname, '..');
const args = process.argv.slice(2);
const check = args.includes('--check');

function valueAfter(name) {
  const index = args.indexOf(name);
  if (index < 0) return '';
  const value = args[index + 1];
  if (!value || value.startsWith('--')) throw new Error(`${name} requires a value`);
  return value;
}

function hostTriple() {
  const output = execFileSync('rustc', ['-vV'], { cwd: root, encoding: 'utf8', timeout: 10_000 });
  const match = /^host:\s*(\S+)$/m.exec(output);
  if (!match) throw new Error('rustc -vV did not report a host triple');
  return match[1];
}

const detectedHost = hostTriple();
const target = valueAfter('--target') || process.env.RIDGE_RDG_TARGET?.trim() || detectedHost;

function paths(forTarget) {
  const windows = forTarget.includes('windows');
  const extension = windows ? '.exe' : '';
  return {
    windows,
    source: join(root, 'target', forTarget, 'release', `rdg${extension}`),
    destination: join(root, 'src-tauri', 'binaries', `rdg-${forTarget}${extension}`),
  };
}

if (!check) {
  for (const buildTarget of new Set([target, detectedHost])) {
    const current = paths(buildTarget);
    execFileSync('cargo', ['build', '--release', '--package', 'ridge-cli', '--bin', 'rdg', '--target', buildTarget], {
      cwd: root,
      stdio: 'inherit',
      timeout: 15 * 60_000,
    });
    mkdirSync(dirname(current.destination), { recursive: true });
    copyFileSync(current.source, current.destination);
    if (!current.windows) chmodSync(current.destination, 0o755);
  }
}

const current = paths(target);
const stat = statSync(current.destination);
if (!stat.isFile() || stat.size === 0) throw new Error(`invalid rdg sidecar: ${current.destination}`);
if (!current.windows && (stat.mode & 0o111) === 0) throw new Error(`sidecar is not executable: ${current.destination}`);

console.log(JSON.stringify({ ok: true, mode: check ? 'check' : 'build', target, destination: current.destination }));
