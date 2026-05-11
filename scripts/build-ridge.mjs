#!/usr/bin/env node
// Ridge multi-version installer build wrapper.
//
// Usage:
//   pnpm build:release                # uses package.json version
//   pnpm build:release -- -r 0.1.4    # override version
//   pnpm build:release -- --release 0.1.4
//
// What it does:
//   1. Resolves a target version (CLI arg or package.json fallback).
//   2. Temporarily rewrites Cargo.toml and wix/path-env.wxs so that:
//        - Cargo crate version matches.
//        - WiX Component Id / Guid / registry key are unique per version,
//          so MSIs of different versions don't collide.
//   3. Spawns `tauri build --config '{...}'` with productName / identifier /
//      version overrides so NSIS/MSI install dir, app identifier and bundle
//      filenames carry the version. Each version installs side-by-side.
//   4. Restores Cargo.toml and path-env.wxs on exit (success or failure).
//
// productName format: `ridge <version>` (e.g. `ridge 0.1.4`) — installs into
// `C:\Program Files\ridge 0.1.4\`. Version slug `0_1_4` is used for identifier
// suffix and WiX Component Id (Wix identifiers can't contain `.`).

import { execSync, spawn } from 'node:child_process';
import { readFileSync, writeFileSync, mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { createHash } from 'node:crypto';
import { fileURLToPath } from 'node:url';
import { dirname, resolve, join } from 'node:path';

const here = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(here, '..');
const pkgJsonPath = resolve(repoRoot, 'package.json');
const cargoTomlPath = resolve(repoRoot, 'src-tauri', 'Cargo.toml');
const wxsPath = resolve(repoRoot, 'src-tauri', 'wix', 'path-env.wxs');

function readText(p) {
  return readFileSync(p, 'utf8');
}

function parseCliArgs() {
  // Manual scan: consume -r/--release [value], forward everything else to tauri.
  const argv = process.argv.slice(2);
  let release;
  const extraTauriArgs = [];
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === '-r' || a === '--release') {
      release = argv[i + 1];
      i++;
      continue;
    }
    if (a.startsWith('--release=')) {
      release = a.slice('--release='.length);
      continue;
    }
    extraTauriArgs.push(a);
  }
  return { release, extraTauriArgs };
}

function resolveVersion(cliVersion) {
  if (cliVersion) {
    if (!/^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$/.test(cliVersion)) {
      throw new Error(`Invalid -r/--release value: ${cliVersion} (expected x.y.z)`);
    }
    return cliVersion;
  }
  const pkg = JSON.parse(readText(pkgJsonPath));
  if (!pkg.version) throw new Error('package.json has no version');
  return pkg.version;
}

function versionSlug(version) {
  // Wix Component Id and identifier suffix must be safe: replace `.` and `+`/`-`.
  return version.replace(/[.+-]/g, '_');
}

function deterministicGuid(seed) {
  // SHA-256 → first 16 bytes → format as UUID. Same input → same GUID, so
  // re-builds of the same version keep the upgrade anchor stable.
  const hex = createHash('sha256').update(seed).digest('hex').slice(0, 32);
  return [
    hex.slice(0, 8),
    hex.slice(8, 12),
    hex.slice(12, 16),
    hex.slice(16, 20),
    hex.slice(20, 32),
  ].join('-').toUpperCase();
}

function rewriteCargoTomlVersion(originalText, newVersion) {
  // Replace ONLY the [package] version line, never workspace member versions.
  // Cargo.toml here is small and well-formed; a simple line scan is reliable.
  const lines = originalText.split('\n');
  let inPackage = false;
  let replaced = false;
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];
    const sectionMatch = line.match(/^\s*\[([^\]]+)\]\s*$/);
    if (sectionMatch) {
      inPackage = sectionMatch[1] === 'package';
      continue;
    }
    if (inPackage && /^\s*version\s*=/.test(line)) {
      lines[i] = `version = "${newVersion}"`;
      replaced = true;
      break;
    }
  }
  if (!replaced) throw new Error('Failed to locate [package] version in Cargo.toml');
  return lines.join('\n');
}

function rewriteWxs(originalText, slug, guid) {
  // Replace component Id, registry sub-key, registry value Name, Environment Id,
  // and Component Guid. Each one is uniquely matchable in this small file.
  return originalText
    .replace(/Id="RidgePathEnvVar"/g, `Id="RidgePathEnvVar_${slug}"`)
    .replace(/Guid="[^"]+"/g, `Guid="${guid}"`)
    .replace(/Key="Software\\tauri-app\\ridge"/g, `Key="Software\\tauri-app\\ridge_${slug}"`)
    .replace(/Name="RidgePathEnv"/g, `Name="RidgePathEnv_${slug}"`)
    .replace(/Id="RidgePathEnv"/g, `Id="RidgePathEnv_${slug}"`);
}

function buildTauriConfigOverride(version, slug) {
  // identifier must not contain underscores: replace with hyphens
  const identifierSlug = slug.replace(/_/g, '-');
  return {
    productName: `ridge ${version}`,
    version,
    identifier: `com.tauri-app.ridge.v${identifierSlug}`,
    // Prevent circular build: pass an env flag so beforeBuildCommand can skip
    // when called from this script.
    build: {
      beforeBuildCommand: 'node -e process.exit(Number(!process.env.RIDGE_BUILD_SKIP))',
    },
    bundle: {
      windows: {
        wix: {
          componentRefs: [`RidgePathEnvVar_${slug}`],
        },
      },
    },
  };
}

function spawnTauriBuild(configPath, extraArgs) {
  return new Promise((resolveSpawn, rejectSpawn) => {
    // Spawn tauri CLI directly from node_modules/.bin. On Windows, .cmd files
    // require shell:true (Node CVE-2024-27980 mitigation); we sidestep arg-
    // escaping pitfalls by passing only the config FILE PATH (Tauri 2 accepts
    // either JSON string or file path; both `--config` and the file path are
    // shell-safe ASCII).
    const isWin = process.platform === 'win32';
    const binDir = resolve(repoRoot, 'node_modules', '.bin');
    const tauriBin = resolve(binDir, isWin ? 'tauri.cmd' : 'tauri');
    const args = ['build', '--config', configPath, ...extraArgs];
    const child = spawn(tauriBin, args, {
      cwd: repoRoot,
      stdio: 'inherit',
      shell: isWin,
      env: { ...process.env, RIDGE_BUILD_SKIP: '1' },
    });
    child.on('error', rejectSpawn);
    child.on('exit', (code) => {
      if (code === 0) resolveSpawn();
      else rejectSpawn(new Error(`tauri build exited with code ${code}`));
    });
  });
}

async function main() {
  const { release, extraTauriArgs } = parseCliArgs();
  const version = resolveVersion(release);
  const slug = versionSlug(version);
  const guid = deterministicGuid(`ridge-path-env:${version}`);

  console.log(`[build-ridge] target version = ${version} (slug=${slug})`);
  console.log(`[build-ridge] productName = "ridge ${version}"`);
  const identifierSlug = slug.replace(/_/g, '-');
  console.log(`[build-ridge] identifier  = com.tauri-app.ridge.v${identifierSlug}`);

  // Build frontend first — tauri needs ../build to exist.
  // Set RIDGE_BUILD_SKIP to avoid circular re-entry if vite triggers npm run build.
  console.log('[build-ridge] Building frontend (vite build)...');
  execSync('npx vite build', {
    cwd: repoRoot,
    stdio: 'inherit',
    env: { ...process.env, RIDGE_BUILD_SKIP: '1' },
  });

  const cargoOriginal = readText(cargoTomlPath);
  const wxsOriginal = readText(wxsPath);

  const cargoNew = rewriteCargoTomlVersion(cargoOriginal, version);
  const wxsNew = rewriteWxs(wxsOriginal, slug, guid);
  const configOverride = buildTauriConfigOverride(version, slug);

  const tmpDir = mkdtempSync(join(tmpdir(), 'build-ridge-'));
  const configPath = join(tmpDir, 'tauri.override.conf.json');
  writeFileSync(configPath, JSON.stringify(configOverride, null, 2), 'utf8');

  let cargoTouched = false;
  let wxsTouched = false;
  try {
    if (cargoNew !== cargoOriginal) {
      writeFileSync(cargoTomlPath, cargoNew, 'utf8');
      cargoTouched = true;
    }
    if (wxsNew !== wxsOriginal) {
      writeFileSync(wxsPath, wxsNew, 'utf8');
      wxsTouched = true;
    }
    await spawnTauriBuild(configPath, extraTauriArgs);
  } finally {
    if (cargoTouched) writeFileSync(cargoTomlPath, cargoOriginal, 'utf8');
    if (wxsTouched) writeFileSync(wxsPath, wxsOriginal, 'utf8');
    rmSync(tmpDir, { recursive: true, force: true });
  }
}

main().catch((err) => {
  console.error('[build-ridge] FAILED:', err.message);
  process.exit(1);
});
