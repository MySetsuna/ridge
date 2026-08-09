import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const fileSystem = vi.hoisted(() => ({ readFileSync: vi.fn() }));
vi.mock('node:fs', () => fileSystem);

import { main, validateVersions, versionSet } from './check-release-version.mjs';

const files = {
  'package.json': JSON.stringify({ version: '1.2.3' }),
  'src-tauri/tauri.conf.json': JSON.stringify({ version: '1.2.3' }),
  'src-tauri/Cargo.toml': 'version = "1.2.3"\n',
  'Cargo.lock': '[[package]]\nname = "ridge"\nversion = "1.2.3"\n',
};

beforeEach(() => {
  vi.clearAllMocks();
  fileSystem.readFileSync.mockImplementation((file) => {
    const normalized = String(file).replaceAll('\\', '/');
    if (normalized.endsWith('/package.json')) return files['package.json'];
    if (normalized.endsWith('/tauri.conf.json')) return files['src-tauri/tauri.conf.json'];
    if (normalized.endsWith('/src-tauri/Cargo.toml')) return files['src-tauri/Cargo.toml'];
    if (normalized.endsWith('/Cargo.lock')) return files['Cargo.lock'];
    return undefined;
  });
  vi.spyOn(console, 'log').mockImplementation(() => {});
  vi.spyOn(console, 'error').mockImplementation(() => {});
});

afterEach(() => vi.restoreAllMocks());

describe('release version contract', () => {
  it('extracts all package, Tauri, manifest, and lockfile versions', () => {
    expect(versionSet('C:/repo')).toEqual(new Map([
      ['package.json', '1.2.3'],
      ['src-tauri/tauri.conf.json', '1.2.3'],
      ['src-tauri/Cargo.toml', '1.2.3'],
      ['Cargo.lock ridge package', '1.2.3'],
    ]));
  });

  it('accepts matching versions and reports mismatches without exiting the test process', () => {
    const versions = new Map([['package.json', '1.0.0'], ['Cargo.lock ridge package', '1.0.0']]);
    expect(validateVersions(versions)).toMatchObject({ expected: '1.0.0', mismatches: [], ok: true });
    expect(main('C:/repo')).toBe(true);
    expect(console.log).toHaveBeenCalledWith('release version contract OK: 1.2.3');
  });

  it('fails closed for missing or divergent versions', () => {
    expect(validateVersions(new Map([['package.json', ''], ['Cargo.lock ridge package', '1.0.0']])).ok).toBe(false);
    expect(validateVersions(new Map([['package.json', '1.0.0'], ['Cargo.lock ridge package', '2.0.0']])).mismatches).toHaveLength(1);
    fileSystem.readFileSync.mockImplementation((file) => String(file).endsWith('Cargo.toml') ? 'version = "9.9.9"' : files['package.json']);
    expect(main('C:/repo')).toBe(false);
    expect(console.error).toHaveBeenCalledWith('release version mismatch:', expect.any(Object));
  });
});
