import { describe, expect, it, vi } from 'vitest';
import { execFileSync } from 'node:child_process';

import { validateBuildArtifacts } from './build-validate.mjs';
import { validateDesktopOnlyHosts } from './check-desktop-only-hosts.mjs';
import { main as checkCapabilityMatrix, validateCapabilityMatrix, TEAMMATE_REMOTE_REQUIRED } from './check-capability-matrix.mjs';
import { normalizeLcov } from './normalize-lcov.mjs';
import { renameArtifacts } from './post-build-rename.mjs';
import { copyTeammateShim } from './copy-teammate-shim.mjs';
import { isTeammateShimStale, main as ensureShim } from './ensure-teammate-shim.mjs';
import { pruneOutputs, walk } from './prune-stale-fonts.mjs';
import { collectEvidence, main as prodStatusMain, parseArgs } from './check-prod-status.mjs';
import { buildPlan as tauriBuildPlan, main as tauriBuildMain } from './tauri-build.mjs';
import { buildDebugPlan, renameDebugArtifacts, main as debugBuildMain } from './tauri-build-debug.mjs';
import { buildFlagFont, TWEMOJI_SHA256 } from './build-flag-font.mjs';
import { main as remoteDesktopBuild } from './build-remote-desktop.mjs';

const quietIo = () => ({ log: vi.fn(), error: vi.fn(), warn: vi.fn() });

describe('build and capability validation helpers', () => {
  it('accepts a complete build fixture and rejects malformed artifacts', () => {
    const root = 'C:/fixture';
    const files = new Map([
      [`${root}/build`, 'dir'],
      [`${root}/build/index.html`, '<html data-sveltekit><script>__RIDGE_BOOT_LOADER__ dismissBrandLoader brand-loader</script></html>'],
      [`${root}/ridge.theme`, JSON.stringify({ version: 1, themes: [{ id: 'dark', label: 'Dark', loader: { primary: '#1', secondary: '#2' }, colors: { bg: '#000' } }] })],
    ]);
    const key = (p) => p.replaceAll('\\', '/');
    const fs = { existsSync: (p) => files.has(key(p)), readFileSync: (p) => files.get(key(p)) };
    expect(validateBuildArtifacts({ root, fs, io: quietIo() })).toBe(0);
    files.set(`${root}/ridge.theme`, '{"version":0,"themes":[{}]}');
    expect(validateBuildArtifacts({ root, fs, io: quietIo() })).toBe(1);
    expect(validateBuildArtifacts({ root, args: ['--html-only'], fs, io: quietIo() })).toBe(0);
  });

  it('checks desktop-only host methods against both allowlist mirrors', () => {
    const desktop = ['host_list_snapshot', 'connect_host', 'disconnect_host', 'forget_host', 'attach_host_session']
      .map((name) => `"${name}"`).join('\n');
    expect(validateDesktopOnlyHosts({ desktop, rustAllow: 'REMOTE_ALLOWLIST = ["safe"]', tsAllow: "['safe']", io: quietIo() }).failed).toBe(0);
    expect(validateDesktopOnlyHosts({ desktop, rustAllow: '"connect_host"', tsAllow: "'connect_host'", io: quietIo() }).failed).toBe(2);
    expect(validateDesktopOnlyHosts({ desktop: '"connect_host"', rustAllow: '', tsAllow: '', io: quietIo() }).failed).toBe(1);
  });

  it('checks teammate required capabilities and host deny list', () => {
    const matrix = { capabilities: { teammate: { methods: [...TEAMMATE_REMOTE_REQUIRED] } } };
    expect(validateCapabilityMatrix({ matrix, rustAllow: 'REMOTE_ALLOWLIST = ["safe"]', io: quietIo() })).toBe(0);
    expect(validateCapabilityMatrix({ matrix: { capabilities: { teammate: { methods: ['connect_host'] } } }, rustAllow: 'REMOTE_ALLOWLIST = ["connect_host"]', io: quietIo() })).toBeGreaterThan(1);
  });

  it('keeps the capability checker executable as a CLI', () => {
    expect(checkCapabilityMatrix(process.cwd(), quietIo())).toBe(0);
    expect(execFileSync(process.execPath, ['scripts/check-capability-matrix.mjs'], { encoding: 'utf8' })).toContain(
      'check-capability-matrix: ok',
    );
  });
});

describe('build artifact helpers', () => {
  it('normalizes only LCOV source path separators', () => {
    expect(normalizeLcov('SF:C:\\code\\wind\\src\\a.ts\nDA:1,1\nSF:src/b.ts')).toBe('SF:C:/code/wind/src/a.ts\nDA:1,1\nSF:src/b.ts');
  });

  it('renames only current-version installer artifacts', () => {
    const copied = [];
    const fs = {
      readFileSync: () => JSON.stringify({ version: '1.2.3' }),
      existsSync: (p) => { const key = p.replaceAll('\\', '/'); return key.endsWith('/release') || key.endsWith('/nsis') || key.endsWith('/msi'); },
      mkdirSync: vi.fn(),
      readdirSync: (p) => p.replaceAll('\\', '/').endsWith('/nsis')
        ? ['ridge_1.2.3_x64-setup.exe', 'ridge_0.0.1_x64-setup.exe', 'note.txt']
        : ['ridge_1.2.3_x64.msi'],
      copyFileSync: (from, to) => copied.push({ from, to }),
    };
    expect(renameArtifacts({ rootDir: 'C:/repo', fsImpl: fs, io: quietIo() })).toBe(2);
    expect(copied.map(({ to }) => to.replaceAll('\\', '/'))).toEqual(['C:/repo/release/ridge_1.2.3_x64-setup.exe', 'C:/repo/release/ridge_1.2.3_x64-setup.msi']);
  });

  it('copies an existing teammate shim and fails closed when absent', () => {
    const io = quietIo();
    const fs = { existsSync: vi.fn(() => true), mkdirSync: vi.fn(), copyFileSync: vi.fn() };
    expect(copyTeammateShim({ rootDir: 'C:/repo', platform: 'win32', fsImpl: fs, io })).toBe(true);
    expect(fs.copyFileSync.mock.calls[0].map((p) => p.replaceAll('\\', '/'))).toEqual(['C:/repo/target/release/tmux.exe', 'C:/repo/dist/teammate-shim/tmux.exe']);
    expect(copyTeammateShim({ rootDir: 'C:/repo', platform: 'linux', fsImpl: { existsSync: () => false }, io })).toBe(false);
  });

  it('rebuilds stale shim once and skips a fresh one', () => {
    const fs = { existsSync: () => true, statSync: (p) => ({ mtimeMs: p.includes('tmux.rs') ? 2 : 1 }) };
    expect(isTeammateShimStale({ rootDir: 'C:/repo', platform: 'win32', fsImpl: fs })).toBe(true);
    const exec = vi.fn();
    expect(ensureShim({ rootDir: 'C:/repo', platform: 'win32', fsImpl: fs, exec, io: quietIo() })).toBe(0);
    expect(exec).toHaveBeenCalledTimes(2);
    const freshFs = { existsSync: () => true, statSync: () => ({ mtimeMs: 1 }) };
    expect(ensureShim({ rootDir: 'C:/repo', platform: 'win32', fsImpl: freshFs, exec, io: quietIo() })).toBe(0);
    expect(exec).toHaveBeenCalledTimes(2);
  });
});

describe('font cleanup helper', () => {
  it('walks nested files, removes oversized fonts, and ignores missing dirs', () => {
    const tree = {
      root: [{ name: 'nested', isDirectory: () => true }, { name: 'small.ttf', isDirectory: () => false }],
      'root/nested': [{ name: 'large.woff2', isDirectory: () => false }, { name: 'readme.txt', isDirectory: () => false }],
    };
    const removed = [];
    const fs = {
      readdirSync: (p) => { const key = p.replaceAll('\\', '/'); if (!tree[key]) throw new Error('missing'); return tree[key]; },
      statSync: (p) => ({ size: p.endsWith('large.woff2') ? 2 * 1024 * 1024 : 1 }),
      rmSync: (p) => removed.push(p),
    };
    expect(walk('root', [], fs).map((p) => p.replaceAll('\\', '/'))).toEqual(['root/nested/large.woff2', 'root/nested/readme.txt', 'root/small.ttf']);
    expect(pruneOutputs({ dirs: ['root', 'missing'], fsImpl: fs, io: quietIo() })).toBe(1);
    expect(removed.map((p) => p.replaceAll('\\', '/'))).toEqual(['root/nested/large.woff2']);
  });
});

describe('production status probe', () => {
  it('parses help/base URL and preserves an unverified missing-token line', async () => {
    expect(parseArgs(['--base-url', 'https://cloud/'])).toEqual({ help: false, baseUrl: 'https://cloud' });
    const io = quietIo();
    const result = await collectEvidence({
      args: ['--base-url', 'https://cloud/'],
      env: {},
      now: new Date('2026-08-10T00:00:00.000Z'),
      fetchImpl: async (url) => ({ ok: true, status: 200, json: async () => ({ ok: true, data: { url } }) }),
      io,
    });
    expect(result.exitCode).toBe(0);
    expect(result.evidence.service.status).toBe('通过');
    expect(result.evidence.artifacts.detail).toBe('缺 RIDGE_ARTIFACT_TOKEN');
    expect(await prodStatusMain(['--help'], { io })).toBe(0);
  });

  it('fails only attempted probes and never places the bearer token in evidence', async () => {
    const result = await collectEvidence({
      args: ['--base-url', 'https://cloud'],
      env: { RIDGE_ARTIFACT_TOKEN: 'secret-token' },
      fetchImpl: async (url) => {
        if (url.endsWith('/health')) throw new Error('offline');
        return { ok: false, status: 503, json: async () => ({ ok: false, error: 'down' }) };
      },
    });
    expect(result.exitCode).toBe(1);
    expect(JSON.stringify(result.evidence)).not.toContain('secret-token');
    expect((await collectEvidence({ args: [], io: quietIo() })).exitCode).toBe(0);
  });
});

describe('Tauri build command plans', () => {
  it('selects optional local accelerators and propagates bundle arguments', async () => {
    const probes = [];
    const probe = (command, args) => { probes.push(args[0]); return { status: args[0] === 'sccache' ? 0 : 1 }; };
    const plan = tauriBuildPlan({ RIDGE_BUNDLES: 'nsis,msi' }, 'win32', probe);
    expect(plan.args).toEqual(['tauri', 'build', '--bundles', 'nsis,msi']);
    expect(plan.env.RUSTC_WRAPPER).toBe('sccache');
    expect(plan.env.CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER).toBeUndefined();
    expect(probes).toEqual(['sccache', 'lld-link']);

    const spawn = vi.fn(() => ({ on: (_event, callback) => callback(4) }));
    expect(await tauriBuildMain({ envSource: {}, platform: 'linux', spawnImpl: spawn, spawnSyncImpl: () => ({ status: 1 }), io: quietIo(), now: () => 0 })).toBe(4);
  });

  it('builds debug/release plans and renames only supported installer formats', async () => {
    const debug = buildDebugPlan({ RIDGE_CLOUD_BASE_DOMAIN: 'cloud.example:5001' }, 'linux', () => ({ status: 0 }));
    expect(debug).toMatchObject({ base: 'cloud.example:5001', profileDir: 'debug', bundles: 'nsis', args: ['tauri', 'build', '--debug', '--bundles', 'nsis'] });
    const copied = [];
    const fs = {
      readFileSync: () => JSON.stringify({ version: '2.0.0' }),
      existsSync: () => true,
      mkdirSync: vi.fn(),
      readdirSync: (p) => p.includes('nsis') ? ['ridge.exe'] : ['ridge.msi'],
      copyFileSync: (from, to) => copied.push([from, to]),
    };
    expect(renameDebugArtifacts('cloud.example:5001', 'debug', 'nsis,msi,deb', { rootDir: 'C:/repo', fsImpl: fs, io: quietIo() })).toBe(2);
    expect(copied).toHaveLength(2);
    const spawn = vi.fn(() => ({ on: (_event, callback) => callback(0) }));
    const io = quietIo();
    expect(await debugBuildMain({ envSource: {}, platform: 'linux', spawnImpl: spawn, spawnSyncImpl: () => ({ status: 1 }), fsImpl: { readFileSync: () => JSON.stringify({ version: '2.0.0' }), existsSync: () => false, mkdirSync: vi.fn() }, rootDir: 'C:/repo', io, now: () => 0 })).toBe(0);
    expect(io.warn).toHaveBeenCalled();
  });
});

describe('flag font build guard', () => {
  const fontFs = (exists = true, size = 10) => ({
    existsSync: () => exists,
    readFileSync: () => Buffer.from('font'),
    statSync: () => ({ size }),
    mkdirSync: vi.fn(),
    copyFileSync: vi.fn(),
    rmSync: vi.fn(),
  });

  it('accepts a hashed cached source and mirrors a bounded subset', () => {
    const fs = fontFs();
    expect(buildFlagFont({ rootDir: 'C:/repo', fsImpl: fs, hashFile: () => TWEMOJI_SHA256, execFileSyncImpl: vi.fn(), io: quietIo() })).toBe(true);
    expect(fs.copyFileSync).toHaveBeenCalled();
  });

  it('rejects a bad download, missing subset tool, and oversized output', () => {
    const bad = fontFs(false);
    expect(buildFlagFont({ rootDir: 'C:/repo', fsImpl: bad, hashFile: () => 'bad', execFileSyncImpl: vi.fn(), io: quietIo() })).toBe(false);
    const missingTool = fontFs();
    const toolError = Object.assign(new Error('missing'), { code: 'ENOENT' });
    expect(buildFlagFont({ rootDir: 'C:/repo', fsImpl: missingTool, hashFile: () => TWEMOJI_SHA256, execFileSyncImpl: () => { throw toolError; }, io: quietIo() })).toBe(false);
    expect(buildFlagFont({ rootDir: 'C:/repo', fsImpl: fontFs(true, 2 * 1024 * 1024), hashFile: () => TWEMOJI_SHA256, execFileSyncImpl: vi.fn(), io: quietIo() })).toBe(false);
  });
});

describe('remote desktop build wrapper', () => {
  it('prunes after a successful Vite build and preserves failures', async () => {
    const spawn = vi.fn(() => ({ on: (_event, callback) => callback(0) }));
    const prune = vi.fn();
    expect(await remoteDesktopBuild({ spawnImpl: spawn, prune, io: quietIo() })).toBe(0);
    expect(prune).toHaveBeenCalledOnce();
    expect(await remoteDesktopBuild({ spawnImpl: () => ({ on: (_event, callback) => callback(null) }), prune, io: quietIo() })).toBe(1);
  });
});
