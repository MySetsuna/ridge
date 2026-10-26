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

export function validateVersions(versions, expectedTag) {
  const expected = versions.get('package.json');
  const mismatches = [...versions].filter(([, version]) => version !== expected);
  if (expectedTag !== undefined && expectedTag !== `v${expected}`) {
    mismatches.push(['release tag', expectedTag]);
  }
  return { expected, mismatches, ok: Boolean(expected) && mismatches.length === 0 };
}

export function main(rootDir = root, expectedTag) {
  const versions = versionSet(rootDir);
  const result = validateVersions(versions, expectedTag);
  if (!result.ok) {
    console.error('release version mismatch:', Object.fromEntries([...versions, ['release tag', expectedTag]]));
    return false;
  }
  console.log(`release version contract OK: ${result.expected}`);
  return true;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  process.exitCode = main(root, process.env.RELEASE_TAG) ? 0 : 1;
}
