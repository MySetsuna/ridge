import { describe, it, expect } from 'vitest';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { packBundle, buildManifest, resolveConfig, collectFiles } from './remoteArtifactBundle.mjs';

describe('packBundle（线格式须与 ridge-cloud parse_header 逐字节一致）', () => {
  it('frames u32 BE 头长 + JSON 头 + 文件体拼接', () => {
    const buf = packBundle(buildManifest({ version: '1', gitSha: 'a', builtAt: 't' }), [
      { path: 'desktop-app/index.html', bytes: Buffer.from('abc') },
      { path: 'mobile-app/index.html', bytes: Buffer.from('de') },
    ]);
    const n = buf.readUInt32BE(0);
    const header = JSON.parse(buf.subarray(4, 4 + n).toString('utf8'));
    expect(header.manifest).toEqual({ version: '1', gitSha: 'a', builtAt: 't' });
    expect(header.files).toEqual([
      { path: 'desktop-app/index.html', size: 3 },
      { path: 'mobile-app/index.html', size: 2 },
    ]);
    expect(buf.subarray(4 + n).toString()).toBe('abcde');
    // 总长 == 4 + headerLen + sum(sizes)（Rust 端 SizeMismatch 校验的等式）。
    expect(buf.length).toBe(4 + n + 5);
  });
});

describe('resolveConfig', () => {
  it('reads env + flags', () => {
    const c = resolveConfig({ RIDGE_CLOUD_ARTIFACT_URL: 'u', RIDGE_ARTIFACT_TOKEN: 't' }, ['--dry-run']);
    expect(c).toMatchObject({ url: 'u', token: 't', dryRun: true, build: true, rollback: false });
  });
  it('--no-build 关构建；--rollback 可带可选版本', () => {
    expect(resolveConfig({}, ['--no-build']).build).toBe(false);
    expect(resolveConfig({}, ['--rollback']).rollback).toBe(true);
    expect(resolveConfig({}, ['--rollback', '0.0.9+gabc']).rollback).toBe('0.0.9+gabc');
  });
});

describe('collectFiles', () => {
  it('递归收集，path 带 prefix + 正斜杠', () => {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'rab-'));
    fs.writeFileSync(path.join(dir, 'index.html'), 'x');
    fs.mkdirSync(path.join(dir, 'sub'));
    fs.writeFileSync(path.join(dir, 'sub', 'a.js'), 'y');
    const files = collectFiles(dir, 'desktop-app').map((f) => f.path).sort();
    expect(files).toEqual(['desktop-app/index.html', 'desktop-app/sub/a.js']);
    fs.rmSync(dir, { recursive: true, force: true });
  });
});
