#!/usr/bin/env node

import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(fileURLToPath(new URL('..', import.meta.url)));

function read(rootDir, relativePath) {
  return readFileSync(resolve(rootDir, relativePath), 'utf8');
}

export function versionSet(rootDir = root) {
  const jsonVersion = (path) => JSON.parse(read(rootDir, path)).version;
  const manifestVersion = (path) => read(rootDir, path)
    .split(/\r?\n/)
    .find((line) => line.trim().startsWith('version = "'))
    ?.trim()
    .slice('version = "'.length, -1);
  const packageVersion = (path, name) => {
    const block = read(rootDir, path)
      .split('[[package]]')
      .find((candidate) => candidate.split(/\r?\n/).some((line) => line.trim() === `name = "${name}"`));
    const versionLine = block?.split(/\r?\n/).find((line) => line.trim().startsWith('version = "'));
    return versionLine?.trim().slice('version = "'.length, -1);
  };
  return new Map([
    ['package.json', jsonVersion('package.json')],
    ['src-tauri/tauri.conf.json', jsonVersion('src-tauri/tauri.conf.json')],
    ['src-tauri/Cargo.toml', manifestVersion('src-tauri/Cargo.toml')],
    ['Cargo.lock ridge package', packageVersion('Cargo.lock', 'ridge')],
  ]);
}

export function validateVersions(versions) {
  const expected = versions.get('package.json');
  const mismatches = [...versions].filter(([, version]) => version !== expected);
  return { expected, mismatches, ok: Boolean(expected) && mismatches.length === 0 };
}

export function main(rootDir = root) {
  const versions = versionSet(rootDir);
  const result = validateVersions(versions);
  if (!result.ok) {
    console.error('release version mismatch:', Object.fromEntries(versions));
    return false;
  }
  console.log(`release version contract OK: ${result.expected}`);
  return true;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  process.exitCode = main() ? 0 : 1;
}
