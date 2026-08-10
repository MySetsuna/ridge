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
