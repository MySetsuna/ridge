#!/usr/bin/env node
// Bundle the same independent Runtime used by CLI, without requiring a CLI install.
import { copyFileSync, readFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { buildSidecar, hostTriple, sidecarPaths, verifySidecar } from './build-ridge-mcp-sidecar.mjs';
const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const externalBin = '../target/runtime-binaries/ridge-kernel';
export function main(args = process.argv.slice(2)) {
  const targetIndex = args.indexOf('--target');
  const explicit = targetIndex < 0 ? '' : args[targetIndex + 1];
  if (targetIndex >= 0 && (!explicit || explicit.startsWith('--'))) throw new Error('--target requires a value');
  const detectedHost = hostTriple();
  const target = explicit || process.env.CARGO_BUILD_TARGET?.trim() || detectedHost;
  const profile = args.includes('--debug') ? 'debug' : 'release';
  const config = JSON.parse(readFileSync(join(root, 'src-tauri', 'tauri.conf.json'), 'utf8'));
  const version = String(config.version || '').trim();
  if (!version || !config.bundle?.externalBin?.includes(externalBin)) {
    throw new Error(`Tauri bundle must declare a version and ${externalBin}`);
  }
  if (!args.includes('--check')) {
    buildSidecar(target, detectedHost, version, { binary: 'ridge-kernel', packageName: 'ridge-kernel',
      versionEnv: 'RIDGE_KERNEL_BUNDLE_VERSION', profile, implicitHost: true, stagingDir: 'target/runtime-binaries' });
  }
  const { windows, destination } = sidecarPaths(target, 'ridge-kernel', profile, target, 'target/runtime-binaries');
  // Every build verifies the staged artifact; checks can verify it without rebuilding.
  if (!args.includes('--check') || args.includes('--require-built')) {
    verifySidecar(destination, windows, target, detectedHost, version, 'ridge-kernel');
    if (windows && !args.includes('--check')) {
      copyFileSync(destination, join(root, 'target/runtime-binaries/ridge-kernel-installer.exe'));
    }
  }
  const result = { ok: true, target, profile, version, externalBin, destination };
  console.log(JSON.stringify(result));
  return result;
}
if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try { main(); } catch (error) { console.error(error); process.exitCode = 1; }
}
