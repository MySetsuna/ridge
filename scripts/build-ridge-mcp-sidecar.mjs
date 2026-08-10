#!/usr/bin/env node

import { execFileSync } from 'node:child_process';
import { chmodSync, copyFileSync, mkdirSync, readFileSync, statSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { cargoTool } from './lib/toolPath.mjs';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');

function valueAfter(args, name) {
  const index = args.indexOf(name);
  if (index < 0) return '';
  const value = args[index + 1];
  if (!value || value.startsWith('--')) throw new Error(`${name} requires a value`);
  return value;
}

export function hostTriple() {
  const output = execFileSync(cargoTool('rustc'), ['-vV'], {
    cwd: root,
    encoding: 'utf8',
    timeout: 10_000,
  });
  const match = /^host:\s*(\S+)$/m.exec(output);
  if (!match) throw new Error('rustc -vV did not report a host triple');
  return match[1];
}

export function sidecarPaths(forTarget) {
  const windows = forTarget.includes('windows');
  const extension = windows ? '.exe' : '';
  return {
    windows,
    source: join(root, 'target', forTarget, 'release', `ridge-mcp${extension}`),
    destination: join(root, 'src-tauri', 'binaries', `ridge-mcp-${forTarget}${extension}`),
  };
}

function loadBundleConfig() {
  const tauri = JSON.parse(readFileSync(join(root, 'src-tauri', 'tauri.conf.json'), 'utf8'));
  const version = String(tauri.version ?? '').trim();
  if (!version) throw new Error('src-tauri/tauri.conf.json has no version');

  const externalBin = tauri.bundle?.externalBin;
  if (!Array.isArray(externalBin) || !externalBin.includes('binaries/ridge-mcp')) {
    throw new Error('tauri bundle.externalBin must contain binaries/ridge-mcp');
  }
  return { version, externalBin: 'binaries/ridge-mcp' };
}

export function main(args = process.argv.slice(2)) {
  const check = args.includes('--check');
  const requireBuilt = args.includes('--require-built');
  const detectedHost = hostTriple();
  const target =
    valueAfter(args, '--target') ||
    process.env.RIDGE_MCP_TARGET?.trim() ||
    process.env.CARGO_BUILD_TARGET?.trim() ||
    detectedHost;
  const { version, externalBin } = loadBundleConfig();
  const { windows, destination } = sidecarPaths(target);

  if (!check) {
    for (const buildTarget of new Set([target, detectedHost])) {
      const paths = sidecarPaths(buildTarget);
      execFileSync(
        cargoTool('cargo'),
        ['build', '--release', '--package', 'ridge-mcp-bridge', '--bin', 'ridge-mcp', '--target', buildTarget],
        {
          cwd: root,
          env: { ...process.env, RIDGE_MCP_BUNDLE_VERSION: version },
          stdio: 'inherit',
          timeout: 15 * 60_000,
        },
      );
      mkdirSync(dirname(paths.destination), { recursive: true });
      copyFileSync(paths.source, paths.destination);
      if (!paths.windows) chmodSync(paths.destination, 0o755);
    }
  }

  if (requireBuilt) {
    const stat = statSync(destination);
    if (!stat.isFile() || stat.size === 0) throw new Error(`invalid sidecar: ${destination}`);
    if (!windows && (stat.mode & 0o111) === 0) throw new Error(`sidecar is not executable: ${destination}`);
    if (!readFileSync(destination).includes(Buffer.from(version))) {
      throw new Error(`sidecar does not embed Ridge version ${version}: ${destination}`);
    }
    if (target === detectedHost) {
      const reported = execFileSync(destination, ['--version'], {
        encoding: 'utf8',
        timeout: 10_000,
      }).trim();
      if (!reported.endsWith(` ${version}`)) {
        throw new Error(`sidecar version mismatch: expected ${version}, got ${reported}`);
      }
    }
  }

  console.log(JSON.stringify({ ok: true, mode: check ? 'check' : 'build', target, version, externalBin, destination, requireBuilt }));
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    main();
  } catch (error) {
    console.error(error);
    process.exitCode = 1;
  }
}
