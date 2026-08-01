#!/usr/bin/env node

import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

const root = resolve(import.meta.dirname, '..');
const read = (path) => readFileSync(resolve(root, path), 'utf8');
const jsonVersion = (path) => JSON.parse(read(path)).version;
const manifestVersion = (path) => read(path).match(/^version = "([^"]+)"\s*$/m)?.[1];
const packageVersion = (path, name) => {
  const block = read(path)
    .split('[[package]]')
    .find((candidate) => new RegExp(`^\\s*name = "${name}"\\s*$`, 'm').test(candidate));
  return block?.match(/^\s*version = "([^"]+)"\s*$/m)?.[1];
};

const versions = new Map([
  ['package.json', jsonVersion('package.json')],
  ['src-tauri/tauri.conf.json', jsonVersion('src-tauri/tauri.conf.json')],
  ['src-tauri/Cargo.toml', manifestVersion('src-tauri/Cargo.toml')],
  ['Cargo.lock ridge package', packageVersion('Cargo.lock', 'ridge')],
]);
const expected = versions.get('package.json');
const mismatches = [...versions].filter(([, version]) => version !== expected);
if (!expected || mismatches.length > 0) {
  console.error('release version mismatch:', Object.fromEntries(versions));
  process.exit(1);
}
console.log(`release version contract OK: ${expected}`);
