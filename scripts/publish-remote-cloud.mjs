#!/usr/bin/env node
// pnpm publish:remote-cloud —— 构建单一 Remote 产物，打成 bundle 直发云端持久卷。
//
// 部署解耦（2026-07-11 设计稿）：不再拷进 ridge-cloud 仓库/镜像，改经鉴权端点
// POST /api/v1/remote-artifacts 上传 → 云端原子换 current → static_host 换即生效、
// 零 ridge-cloud 重部署。
//
// 用法：
//   RIDGE_CLOUD_ARTIFACT_URL=https://9527127.xyz/api/v1/remote-artifacts \
//   RIDGE_ARTIFACT_TOKEN=<token> pnpm publish:remote-cloud
// flag：
//   --no-build       跳过构建，用现有 remote-dist 产物
//   --dry-run        只打包不上传，落 build/remote-artifact-<ver>.bundle
//   --rollback [ver] 回滚 current 到上一个（或指定）release
import { execSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import {
  collectFiles,
  packBundle,
  buildManifest,
  writeArtifactMetadata,
  readArtifactMetadata,
  resolveConfig,
} from './lib/remoteArtifactBundle.mjs';

const ROOT = path.resolve(fileURLToPath(new URL('..', import.meta.url)));
const REMOTE_DIST = path.join(ROOT, 'remote-dist');

const die = (msg) => {
  console.error('✗ ' + msg);
  process.exit(1);
};
const sh = (cmd) => execSync(cmd, { cwd: ROOT, stdio: 'pipe' }).toString().trim();
const run = (cmd) => execSync(cmd, { cwd: ROOT, stdio: 'inherit' });

function pkgVersion() {
  return JSON.parse(fs.readFileSync(path.join(ROOT, 'package.json'), 'utf8')).version;
}
function gitSha() {
  try {
    return sh('git rev-parse --short HEAD');
  } catch {
    return 'nogit';
  }
}

async function report(res) {
  const text = await res.text();
  if (!res.ok) die(`HTTP ${res.status}: ${text}`);
  let data;
  try {
    data = JSON.parse(text);
  } catch {
    data = text;
  }
  console.log('✓ 已发布：', JSON.stringify(data?.data ?? data));
}

async function main() {
  const cfg = resolveConfig(process.env, process.argv.slice(2));

  // ── 回滚 ──
  if (cfg.rollback) {
    if (!cfg.url) die('缺 RIDGE_CLOUD_ARTIFACT_URL');
    if (!cfg.token) die('缺 RIDGE_ARTIFACT_TOKEN');
    const to = typeof cfg.rollback === 'string' ? cfg.rollback : undefined;
    console.log(`· 回滚 current${to ? ` → ${to}` : '（到上一个 release）'} …`);
    const res = await fetch(`${cfg.url}/rollback`, {
      method: 'POST',
      headers: { Authorization: `Bearer ${cfg.token}`, 'Content-Type': 'application/json' },
      body: JSON.stringify(to ? { to } : {}),
    });
    await report(res);
    return;
  }

  // ── 构建 ──
  if (cfg.build) {
    console.log('· 构建 Remote 统一产物…');
    run('pnpm build:remote');
  }
  for (const dist of ['desktop', 'mobile'].map((kind) => path.join(REMOTE_DIST, kind))) {
    if (!fs.existsSync(path.join(dist, 'index.html'))) {
      die(`产物缺失：${dist}/index.html 不存在（先构建，或去掉 --no-build）`);
    }
  }

  // ── 打包 ──
  const roots = ['desktop', 'mobile'].map((kind) => path.join(REMOTE_DIST, kind));
  const expectedVersion = pkgVersion();
  const expectedSha = gitSha();
  let manifest;
  if (cfg.build) {
    manifest = buildManifest({
      version: expectedVersion,
      gitSha: expectedSha,
      builtAt: new Date().toISOString(),
    });
    // Publish a tiny public fingerprint beside each UA-specific root. The
    // cloud API manifest proves what was uploaded; this file proves what the
    // static desktop/mobile entrypoint is actually serving after activation.
    for (const dist of roots) writeArtifactMetadata(dist, manifest);
  } else {
    // --no-build is intentionally strict: never stamp the current SHA onto a
    // stale directory and call it a fresh artifact. Both UA roots must carry
    // the same fingerprint and it must match this checkout.
    const metadata = roots.map(readArtifactMetadata);
    if (metadata.some((value) => !value)) {
      die('--no-build requires artifact.json in both desktop and mobile roots');
    }
    const first = metadata[0];
    if (metadata.some((value) => JSON.stringify(value) !== JSON.stringify(first))) {
      die('--no-build requires matching desktop/mobile artifact.json fingerprints');
    }
    if (first.version !== expectedVersion || first.gitSha !== expectedSha) {
      die(
        `--no-build artifact mismatch: roots=${first.version}+g${first.gitSha}, `
        `checkout=${expectedVersion}+g${expectedSha}`,
      );
    }
    manifest = first;
  }
  const files = collectFiles(REMOTE_DIST, 'remote-app');
  const bundle = packBundle(manifest, files);
  const ver = `${manifest.version}+g${manifest.gitSha}`;
  console.log(
    `· bundle：${files.length} 文件，${(bundle.length / 1048576).toFixed(2)} MiB，版本 ${ver}`
  );

  // ── dry-run：落盘 ──
  if (cfg.dryRun) {
    const out = path.join(ROOT, 'build', `remote-artifact-${ver}.bundle`);
    fs.mkdirSync(path.dirname(out), { recursive: true });
    fs.writeFileSync(out, bundle);
    console.log(`· --dry-run：已落盘 ${out}（未上传）`);
    return;
  }

  // ── 上传 ──
  if (!cfg.url) {
    die('缺 RIDGE_CLOUD_ARTIFACT_URL（如 https://9527127.xyz/api/v1/remote-artifacts）');
  }
  if (!cfg.token) die('缺 RIDGE_ARTIFACT_TOKEN');
  console.log(`· 上传到 ${cfg.url} …`);
  const res = await fetch(cfg.url, {
    method: 'POST',
    headers: {
      Authorization: `Bearer ${cfg.token}`,
      'Content-Type': 'application/octet-stream',
    },
    body: bundle,
  });
  await report(res);
}

main().catch((e) => die(e?.stack || String(e)));
