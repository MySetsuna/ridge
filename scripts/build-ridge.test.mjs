import { describe, expect, it, vi } from 'vitest';

const childProcess = vi.hoisted(() => ({
  execFileSync: vi.fn(),
  spawn: vi.fn(),
}));
const fileSystem = vi.hoisted(() => ({
  readFileSync: vi.fn((file) => {
    if (String(file).endsWith('package.json')) return JSON.stringify({ version: '9.8.7' });
    if (String(file).endsWith('Cargo.toml')) return '[package]\nname = "ridge"\nversion = "0.0.1"\n';
    if (String(file).endsWith('path-env.wxs')) return '<Component Id="RidgePathEnvVar" Guid="old"><RegistryKey Key="Software\\\\tauri-app\\\\ridge"><RegistryValue Name="RidgePathEnv" /><Environment Id="RidgePathEnv" /></RegistryKey></Component>';
    throw new Error(`unexpected file: ${file}`);
  }),
  writeFileSync: vi.fn(),
  mkdtempSync: vi.fn(() => 'C:\\temp\\build-ridge-test'),
  rmSync: vi.fn(),
}));

vi.mock('node:child_process', () => childProcess);
vi.mock('node:fs', () => fileSystem);
import {
  buildTauriConfigOverride,
  deterministicGuid,
  main,
  parseCliArgs,
  resolveVersion,
  rewriteCargoTomlVersion,
  rewriteWxs,
  spawnTauriBuild,
  versionSlug,
} from './build-ridge.mjs';

describe('build-ridge pure build contract', () => {
  it('parses release overrides while preserving tauri arguments', () => {
    const original = process.argv;
    process.argv = ['node', 'build-ridge.mjs', '-r', '1.2.3', '--verbose', '--release=2.0.0'];
    try {
      expect(parseCliArgs()).toEqual({ release: '2.0.0', extraTauriArgs: ['--verbose'] });
    } finally {
      process.argv = original;
    }
  });

  it('validates semantic release versions and reads the package fallback', () => {
    expect(resolveVersion('1.2.3-beta.1')).toBe('1.2.3-beta.1');
    expect(() => resolveVersion('1.2')).toThrow('expected x.y.z');
    expect(resolveVersion()).toMatch(/^\d+\.\d+\.\d+$/);
  });

  it('derives stable installer identities and rewrites only targeted metadata', () => {
    expect(versionSlug('1.2.3-beta+7')).toBe('1_2_3_beta_7');
    const guid = deterministicGuid('ridge-path-env:1.2.3');
    expect(guid).toMatch(/^[0-9A-F]{8}(?:-[0-9A-F]{4}){3}-[0-9A-F]{12}$/);
    expect(deterministicGuid('ridge-path-env:1.2.3')).toBe(guid);

    const cargo = '[package]\nname = "ridge"\nversion = "0.0.1"\n\n[workspace]\nversion = "9.9.9"\n';
    expect(rewriteCargoTomlVersion(cargo, '1.2.3')).toContain('version = "1.2.3"');
    expect(rewriteCargoTomlVersion(cargo, '1.2.3')).toContain('[workspace]\nversion = "9.9.9"');
    expect(() => rewriteCargoTomlVersion('[dependencies]\nserde = "1"\n', '1.2.3'))
      .toThrow('Failed to locate');

    const wxs = '<Component Id="RidgePathEnvVar" Guid="old"><RegistryKey Key="Software\\tauri-app\\ridge"><RegistryValue Name="RidgePathEnv" /><Environment Id="RidgePathEnv" /></RegistryKey></Component>';
    const rewritten = rewriteWxs(wxs, '1_2_3', guid);
    expect(rewritten).toContain('RidgePathEnvVar_1_2_3');
    expect(rewritten).toContain(`Guid="${guid}"`);
    expect(rewritten).toContain('Software\\tauri-app\\ridge_1_2_3');
    expect(rewritten).toContain('RidgePathEnv_1_2_3');
  });

  it('builds the side-by-side Tauri override contract', () => {
    expect(buildTauriConfigOverride('1.2.3', '1_2_3')).toEqual(expect.objectContaining({
      productName: 'ridge 1.2.3',
      version: '1.2.3',
      identifier: 'com.tauri-app.ridge.v1-2-3',
      build: expect.objectContaining({ beforeBuildCommand: expect.stringContaining('RIDGE_BUILD_SKIP') }),
      bundle: expect.objectContaining({
        targets: ['nsis', 'msi', 'dmg', 'appimage', 'deb'],
        windows: { wix: { componentRefs: ['RidgePathEnvVar_1_2_3'] } },
      }),
    }));
  });

  it('resolves and rejects from the tauri child exit contract', async () => {
    childProcess.spawn.mockImplementationOnce(() => ({
      on(event, callback) {
        if (event === 'exit') callback(0);
        return this;
      },
    }));
    await expect(spawnTauriBuild('override.json', ['--debug'])).resolves.toBeUndefined();
    expect(childProcess.spawn).toHaveBeenCalledWith(
      expect.stringContaining('tauri.cmd'),
      ['build', '--config', 'override.json', '--debug'],
      expect.objectContaining({ shell: true, env: expect.objectContaining({ RIDGE_BUILD_SKIP: '1' }) }),
    );

    childProcess.spawn.mockImplementationOnce(() => ({
      on(event, callback) {
        if (event === 'exit') callback(2);
        return this;
      },
    }));
    await expect(spawnTauriBuild('override.json', [])).rejects.toThrow('code 2');
  });

  it('builds both frontends and restores versioned files after success', async () => {
    const original = process.argv;
    const log = vi.spyOn(console, 'log').mockImplementation(() => {});
    process.argv = ['node', 'build-ridge.mjs', '--release', '1.2.3'];
    childProcess.execFileSync.mockReset();
    childProcess.spawn.mockImplementationOnce(() => ({
      on(event, callback) {
        if (event === 'exit') queueMicrotask(() => callback(0));
        return this;
      },
    }));
    fileSystem.writeFileSync.mockClear();
    fileSystem.rmSync.mockClear();

    try {
      await main();
      expect(childProcess.execFileSync).toHaveBeenCalledTimes(2);
      expect(childProcess.execFileSync).toHaveBeenNthCalledWith(1, process.execPath, expect.arrayContaining(['build']), expect.objectContaining({
        env: expect.objectContaining({ RIDGE_BUILD_SKIP: '1' }),
      }));
      expect(fileSystem.writeFileSync).toHaveBeenCalledWith(
        expect.stringContaining('Cargo.toml'),
        expect.stringContaining('version = "1.2.3"'),
        'utf8',
      );
      expect(fileSystem.writeFileSync).toHaveBeenCalledWith(
        expect.stringContaining('Cargo.toml'),
        expect.stringContaining('version = "0.0.1"'),
        'utf8',
      );
      expect(fileSystem.rmSync).toHaveBeenCalledWith('C:\\temp\\build-ridge-test', { recursive: true, force: true });
    } finally {
      process.argv = original;
      log.mockRestore();
    }
  });

  it('restores versioned files when tauri build fails', async () => {
    const original = process.argv;
    const log = vi.spyOn(console, 'log').mockImplementation(() => {});
    process.argv = ['node', 'build-ridge.mjs', '-r', '1.2.4'];
    childProcess.execFileSync.mockReset();
    childProcess.spawn.mockImplementationOnce(() => ({
      on(event, callback) {
        if (event === 'exit') queueMicrotask(() => callback(7));
        return this;
      },
    }));
    fileSystem.writeFileSync.mockClear();
    fileSystem.rmSync.mockClear();

    try {
      await expect(main()).rejects.toThrow('code 7');
      expect(fileSystem.writeFileSync).toHaveBeenCalledWith(
        expect.stringContaining('Cargo.toml'),
        expect.stringContaining('version = "0.0.1"'),
        'utf8',
      );
      expect(fileSystem.rmSync).toHaveBeenCalledWith('C:\\temp\\build-ridge-test', { recursive: true, force: true });
    } finally {
      process.argv = original;
      log.mockRestore();
    }
  });
});
