import { beforeEach, afterEach, describe, expect, it, vi } from 'vitest';
const helpers = vi.hoisted(() => ({ buildSidecar: vi.fn(), hostTriple: vi.fn(), sidecarPaths: vi.fn(), verifySidecar: vi.fn() }));
const config = vi.hoisted(() => ({ readFileSync: vi.fn(), copyFileSync: vi.fn() }));
vi.mock('./build-ridge-mcp-sidecar.mjs', () => helpers);
vi.mock('node:fs', () => config);
import { main } from './build-ridge-kernel-sidecar.mjs';
beforeEach(() => {
  vi.clearAllMocks();
  helpers.hostTriple.mockReturnValue('x86_64-pc-windows-msvc');
  helpers.sidecarPaths.mockReturnValue({ windows: true, destination: 'kernel.exe' });
  config.readFileSync.mockReturnValue(JSON.stringify({ version: '0.1.90', bundle: { externalBin: ['../target/runtime-binaries/ridge-kernel'] } }));
  vi.spyOn(console, 'log').mockImplementation(() => {});
});
afterEach(() => vi.restoreAllMocks());
describe('independent Runtime packaging', () => {
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
