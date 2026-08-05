import { describe, it, expect } from 'vitest';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {
  packBundle,
  buildManifest,
  resolveConfig,
  collectFiles,
  writeArtifactMetadata,
  readArtifactMetadata,
} from './remoteArtifactBundle.mjs';

describe('packBundle（线格式须与 ridge-cloud parse_header 逐字节一致）', () => {
  it('frames u32 BE 头长 + JSON 头 + 文件体拼接', () => {
    const buf = packBundle(buildManifest({ version: '1', gitSha: 'a', builtAt: 't' }), [
      { path: 'remote-app/desktop/index.html', bytes: Buffer.from('abc') },
      { path: 'remote-app/mobile/index.html', bytes: Buffer.from('de') },
    ]);
    const n = buf.readUInt32BE(0);
    const header = JSON.parse(buf.subarray(4, 4 + n).toString('utf8'));
    expect(header.manifest).toEqual({ version: '1', gitSha: 'a', builtAt: 't' });
    expect(header.files).toEqual([
      { path: 'remote-app/desktop/index.html', size: 3 },
      { path: 'remote-app/mobile/index.html', size: 2 },
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
    const files = collectFiles(dir, 'remote-app').map((f) => f.path).sort();
    expect(files).toEqual(['remote-app/index.html', 'remote-app/sub/a.js']);
    fs.rmSync(dir, { recursive: true, force: true });
  });
});

describe('writeArtifactMetadata', () => {
  it('writes a stable public fingerprint matching the upload manifest', () => {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'ram-'));
    const manifest = { version: '0.1.60', gitSha: 'abc1234', builtAt: '2026-08-05T00:00:00Z' };
    expect(writeArtifactMetadata(dir, manifest)).toBe(path.join(dir, 'artifact.json'));
    expect(JSON.parse(fs.readFileSync(path.join(dir, 'artifact.json'), 'utf8'))).toEqual({
      artifactSchema: 1,
      ...manifest,
    });
    expect(readArtifactMetadata(dir)).toEqual(manifest);
    fs.writeFileSync(path.join(dir, 'artifact.json'), '{"artifactSchema":1,"version":1}', 'utf8');
    expect(readArtifactMetadata(dir)).toBeNull();
    fs.rmSync(dir, { recursive: true, force: true });
  });
});
