import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const childProcess = vi.hoisted(() => ({ execFileSync: vi.fn() }));
const fileSystem = vi.hoisted(() => ({
  chmodSync: vi.fn(), copyFileSync: vi.fn(), existsSync: vi.fn(() => false), mkdirSync: vi.fn(), statSync: vi.fn(),
}));

vi.mock('node:child_process', () => childProcess);
vi.mock('node:fs', () => fileSystem);

import { hostTriple, main, sidecarPaths } from './build-rdg-sidecar.mjs';

beforeEach(() => {
  vi.clearAllMocks();
  childProcess.execFileSync.mockImplementation((command) => String(command).toLowerCase().endsWith('rustc.exe') || String(command).endsWith('/rustc') ? 'host: x86_64-pc-windows-msvc\n' : '');
  fileSystem.statSync.mockReturnValue({ isFile: () => true, size: 1, mode: 0o755 });
  vi.spyOn(console, 'log').mockImplementation(() => {});
});

afterEach(() => vi.restoreAllMocks());

describe('rdg sidecar build contract', () => {
  it('parses host and platform artifact paths', () => {
    expect(hostTriple()).toBe('x86_64-pc-windows-msvc');
    expect(sidecarPaths('x86_64-pc-windows-msvc').destination).toMatch(/rdg-x86_64-pc-windows-msvc\.exe$/);
    expect(sidecarPaths('aarch64-unknown-linux-gnu').windows).toBe(false);
  });

  it('checks an existing sidecar without building', () => {
    main(['--check', '--target', 'aarch64-unknown-linux-gnu']);
    expect(fileSystem.copyFileSync).not.toHaveBeenCalled();
    expect(console.log).toHaveBeenCalledWith(expect.stringContaining('"mode":"check"'));
  });

  it('builds requested and host targets and marks Unix output executable', () => {
    main(['--target', 'aarch64-unknown-linux-gnu']);
    expect(fileSystem.copyFileSync).toHaveBeenCalledTimes(2);
    expect(fileSystem.chmodSync).toHaveBeenCalledWith(expect.stringContaining('aarch64-unknown-linux-gnu'), 0o755);
  });

  it('rejects missing target values and invalid artifacts', () => {
    expect(() => main(['--target'])).toThrow('--target requires a value');
    fileSystem.statSync.mockReturnValueOnce({ isFile: () => false, size: 0, mode: 0 });
    expect(() => main(['--check'])).toThrow('invalid rdg sidecar');
  });
});
