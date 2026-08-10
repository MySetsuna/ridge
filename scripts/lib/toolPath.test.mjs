import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { cargoTool } from './toolPath.mjs';

const extension = process.platform === 'win32' ? '.exe' : '';
const tempDirs = [];

afterEach(() => {

  for (const dir of tempDirs.splice(0)) rmSync(dir, { recursive: true, force: true });
  vi.unstubAllEnvs();
});

describe('cargoTool', () => {
  it('uses an explicit Ridge override', () => {
    vi.stubEnv('RIDGE_WASM_PACK_PATH', 'C:\\ridge-tools\\wasm-pack.exe');
    expect(cargoTool('wasm-pack')).toBe('C:\\ridge-tools\\wasm-pack.exe');
  });

  it('uses a tool under the configured Cargo home', () => {
    const dir = mkdtempSync(join(tmpdir(), 'ridge-tool-path-'));
    tempDirs.push(dir);
    const cargoHome = join(dir, 'cargo-home');
    const tool = join(cargoHome, 'bin', `wasm-pack${extension}`);
    mkdirSync(join(cargoHome, 'bin'), { recursive: true });
    writeFileSync(tool, 'fixture');
    vi.stubEnv('CARGO_HOME', cargoHome);

    expect(cargoTool('wasm-pack')).toBe(tool);
  });

  it('falls back to PATH when Cargo home differs', () => {
    const dir = mkdtempSync(join(tmpdir(), 'ridge-tool-path-'));
    tempDirs.push(dir);
    const pathDir = join(dir, 'bin');
    const pathTool = join(pathDir, `wasm-pack${extension}`);
    mkdirSync(pathDir, { recursive: true });
    writeFileSync(pathTool, 'fixture');
    vi.stubEnv('CARGO_HOME', join(dir, 'other-cargo-home'));
    vi.stubEnv('PATH', pathDir);

    expect(cargoTool('wasm-pack')).toBe(pathTool);
  });
});
