import { beforeEach, afterEach, describe, expect, it, vi } from 'vitest';
const helpers = vi.hoisted(() => ({ buildSidecar: vi.fn(), hostTriple: vi.fn(), sidecarPaths: vi.fn(), verifySidecar: vi.fn() }));
const config = vi.hoisted(() => ({ readFileSync: vi.fn(), copyFileSync: vi.fn() }));
vi.mock('./build-ridge-mcp-sidecar.mjs', () => helpers);
vi.mock('node:fs', () => config);
import { main } from './build-ridge-kernel-sidecar.mjs';
beforeEach(() => {
  vi.clearAllMocks();
  vi.stubEnv('CARGO_BUILD_TARGET', '');
  vi.stubEnv('TAURI_ENV_TARGET_TRIPLE', '');
  helpers.hostTriple.mockReturnValue('x86_64-pc-windows-msvc');
  helpers.sidecarPaths.mockReturnValue({ windows: true, destination: 'kernel.exe' });
  config.readFileSync.mockReturnValue(JSON.stringify({ version: '0.1.90', bundle: { externalBin: ['../target/runtime-binaries/ridge-kernel'] } }));
  vi.spyOn(console, 'log').mockImplementation(() => {});
});
afterEach(() => { vi.restoreAllMocks(); vi.unstubAllEnvs(); });
describe('independent Runtime packaging', () => {
  it.each([
    { args: [], cargo: '', expected: 'x86_64-apple-darwin' },
    { args: [], cargo: 'aarch64-apple-darwin', expected: 'aarch64-apple-darwin' },
    { args: ['--target', 'x86_64-unknown-linux-gnu'], cargo: 'aarch64-apple-darwin', expected: 'x86_64-unknown-linux-gnu' },
  ])('resolves the Tauri hook target with explicit overrides: $expected', ({ args, cargo, expected }) => {
    helpers.hostTriple.mockReturnValue('aarch64-apple-darwin');
    helpers.sidecarPaths.mockReturnValue({ windows: false, destination: 'kernel' });
    vi.stubEnv('TAURI_ENV_TARGET_TRIPLE', 'x86_64-apple-darwin');
    vi.stubEnv('CARGO_BUILD_TARGET', cargo);
    main(args);
    expect(helpers.buildSidecar).toHaveBeenCalledWith(expected, 'aarch64-apple-darwin', '0.1.90', expect.any(Object));
    expect(helpers.verifySidecar).toHaveBeenCalledWith('kernel', false, expected, 'aarch64-apple-darwin', '0.1.90', 'ridge-kernel');
  });

  it('builds and verifies the Runtime matching the desktop package version', () => {
    main(['--debug']);
    expect(helpers.buildSidecar).toHaveBeenCalledWith('x86_64-pc-windows-msvc', 'x86_64-pc-windows-msvc', '0.1.90', {
      binary: 'ridge-kernel', packageName: 'ridge-kernel', versionEnv: 'RIDGE_KERNEL_BUNDLE_VERSION', profile: 'debug', implicitHost: true, stagingDir: 'target/runtime-binaries',
    });
    expect(helpers.verifySidecar).toHaveBeenCalledWith('kernel.exe', true, 'x86_64-pc-windows-msvc', 'x86_64-pc-windows-msvc', '0.1.90', 'ridge-kernel');
    expect(config.copyFileSync).toHaveBeenCalledWith('kernel.exe', expect.stringMatching(/ridge-kernel-installer\.exe$/));
  });
  it('honors the selected target and checks existing artifacts without rebuilding', () => {
    main(['--target', 'aarch64-unknown-linux-gnu', '--check', '--require-built']);
    expect(helpers.buildSidecar).not.toHaveBeenCalled();
    expect(helpers.verifySidecar).toHaveBeenCalledWith('kernel.exe', true, 'aarch64-unknown-linux-gnu', 'x86_64-pc-windows-msvc', '0.1.90', 'ridge-kernel');
  });
  it('refuses incomplete bundle declarations and propagates build/verification failure', () => {
    expect(() => main(['--target'])).toThrow('--target requires a value');
    config.readFileSync.mockReturnValueOnce(JSON.stringify({ version: '0.1.90', bundle: { externalBin: [] } }));
    expect(() => main([])).toThrow('binaries/ridge-kernel');
    helpers.verifySidecar.mockImplementationOnce(() => { throw Error('sidecar version mismatch'); });
    expect(() => main(['--check', '--require-built'])).toThrow('sidecar version mismatch');
  });
});
