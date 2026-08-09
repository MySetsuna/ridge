import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const childProcess = vi.hoisted(() => ({ execFileSync: vi.fn() }));
const fileSystem = vi.hoisted(() => ({
  chmodSync: vi.fn(),
  copyFileSync: vi.fn(),
  mkdirSync: vi.fn(),
  readFileSync: vi.fn(),
  statSync: vi.fn(),
}));

vi.mock('node:child_process', () => childProcess);
vi.mock('node:fs', () => fileSystem);

import { hostTriple, main, sidecarPaths } from './build-ridge-mcp-sidecar.mjs';

const config = JSON.stringify({ version: '0.1.61', bundle: { externalBin: ['binaries/ridge-mcp'] } });

beforeEach(() => {
  vi.clearAllMocks();
  childProcess.execFileSync.mockImplementation((command, args) => {
    if (command === 'rustc') return 'host: x86_64-pc-windows-msvc\n';
    if (Array.isArray(args) && args[0] === '--version') return 'ridge-mcp 0.1.61\n';
    return '';
  });
  fileSystem.readFileSync.mockImplementation((file) => (
    String(file).endsWith('tauri.conf.json') ? config : Buffer.from('ridge-mcp 0.1.61')
  ));
  fileSystem.statSync.mockReturnValue({ isFile: () => true, size: 20, mode: 0o755 });
  vi.spyOn(console, 'log').mockImplementation(() => {});
});

afterEach(() => vi.restoreAllMocks());

describe('ridge-mcp sidecar build contract', () => {
  it('resolves host triples and platform-specific artifact paths', () => {
    expect(hostTriple()).toBe('x86_64-pc-windows-msvc');
    expect(sidecarPaths('x86_64-pc-windows-msvc')).toMatchObject({ windows: true });
    expect(sidecarPaths('aarch64-unknown-linux-gnu')).toMatchObject({ windows: false });
    expect(sidecarPaths('aarch64-unknown-linux-gnu').destination).toMatch(/ridge-mcp-aarch64-unknown-linux-gnu$/);
  });

  it('runs a check without building and reports the selected target', () => {
    main(['--check', '--target', 'aarch64-unknown-linux-gnu']);
    expect(childProcess.execFileSync).toHaveBeenCalledTimes(1);
    expect(fileSystem.copyFileSync).not.toHaveBeenCalled();
    expect(console.log).toHaveBeenCalledWith(expect.stringContaining('"mode":"check"'));
  });

  it('builds both requested and host sidecars, preserving executable bits', () => {
    main(['--target', 'aarch64-unknown-linux-gnu']);
    expect(childProcess.execFileSync).toHaveBeenCalledTimes(3);
    expect(fileSystem.copyFileSync).toHaveBeenCalledTimes(2);
    expect(fileSystem.chmodSync).toHaveBeenCalledOnce();
    expect(fileSystem.chmodSync).toHaveBeenCalledWith(expect.stringContaining('aarch64-unknown-linux-gnu'), 0o755);
  });

  it('validates a host sidecar and its reported version', () => {
    main(['--check', '--require-built']);
    expect(fileSystem.statSync).toHaveBeenCalledOnce();
    expect(childProcess.execFileSync).toHaveBeenCalledWith(expect.stringContaining('ridge-mcp-x86_64-pc-windows-msvc.exe'), ['--version'], expect.anything());
  });

  it('fails closed for malformed arguments, bundle metadata, and artifacts', () => {
    expect(() => main(['--target'])).toThrow('--target requires a value');

    fileSystem.readFileSync.mockImplementationOnce(() => JSON.stringify({ version: '0.1.61', bundle: {} }));
    expect(() => main(['--check'])).toThrow('externalBin');

    fileSystem.statSync.mockReturnValueOnce({ isFile: () => false, size: 0, mode: 0 });
    expect(() => main(['--check', '--require-built'])).toThrow('invalid sidecar');
  });
});
