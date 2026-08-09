// Remote 产物 bundle 打包纯函数（部署解耦，2026-07-11 设计稿 §5.1）。
//
// 线格式（与 ridge-cloud src/api/remote_artifacts.rs 逐字节一致）：
//   [u32 BE header_len][header_len 字节 UTF-8 JSON 头][文件体按序拼接]
//   JSON 头 = { manifest:{version,gitSha,builtAt}, files:[{path,size}] }
// 纯 Node Buffer，无第三方依赖。
import fs from 'node:fs';
import path from 'node:path';

/** 组 manifest（字段名与 Rust 端 serde rename 对齐：gitSha/builtAt）。 */
export function buildManifest({ version, gitSha, builtAt }) {
  return { version, gitSha, builtAt };
}

/**
 * Write a public, payload-free artifact fingerprint into one publish root.
 * The static host UA-splits desktop/mobile roots, so each root needs its own
 * copy. Keep the file tiny and identical to the upload manifest fields so a
 * post-activation probe can prove which Git commit is actually being served.
 */
export function writeArtifactMetadata(dir, manifest) {
  const file = path.join(dir, 'artifact.json');
  fs.writeFileSync(
    file,
    `${JSON.stringify({ artifactSchema: 1, ...manifest }, null, 2)}\n`,
    'utf8',
  );
  return file;
}

/** Read and validate a previously built root fingerprint for --no-build. */
export function readArtifactMetadata(dir) {
  try {
    const value = JSON.parse(fs.readFileSync(path.join(dir, 'artifact.json'), 'utf8'));
    if (
      value?.artifactSchema !== 1
      || typeof value.version !== 'string'
      || typeof value.gitSha !== 'string'
      || typeof value.builtAt !== 'string'
    ) return null;
    return {
      version: value.version,
      gitSha: value.gitSha,
      builtAt: value.builtAt,
    };
  } catch {
    return null;
  }
}

/**
 * 递归收集 dir 下所有文件，path = `<prefix>/<相对路径>`（正斜杠）。
 * 返回 [{ path, abs }]，abs 供 packBundle 读字节。
 */
export function collectFiles(dir, prefix) {
  const out = [];
  const walk = (cur, rel) => {
    for (const name of fs.readdirSync(cur).sort()) {
      const abs = path.join(cur, name);
      const relPath = rel ? `${rel}/${name}` : name;
      if (fs.statSync(abs).isDirectory()) walk(abs, relPath);
      else out.push({ path: `${prefix}/${relPath}`, abs });
    }
  };
  walk(dir, '');
  return out;
}

/**
 * 打包成 bundle Buffer。files: [{ path, bytes?|abs? }]——有 bytes 用之，否则读 abs。
 */
export function packBundle(manifest, files) {
  const bodies = files.map((f) => f.bytes ?? fs.readFileSync(f.abs));
  const fileEntries = files.map((f, i) => ({ path: f.path, size: bodies[i].length }));
  const headerBuf = Buffer.from(JSON.stringify({ manifest, files: fileEntries }), 'utf8');
  const lenBuf = Buffer.alloc(4);
  lenBuf.writeUInt32BE(headerBuf.length, 0);
  return Buffer.concat([lenBuf, headerBuf, ...bodies]);
}

/** 解析 env + argv 为发布配置。 */
export function resolveConfig(env, argv) {
  const has = (f) => argv.includes(f);
  const rbIdx = argv.indexOf('--rollback');
  /** @type {boolean | string} */
  let rollback = false;
  if (rbIdx >= 0) {
    const next = argv[rbIdx + 1];
    rollback = next && !next.startsWith('--') ? next : true;
  }
  return {
    url: env.RIDGE_CLOUD_ARTIFACT_URL,
    token: env.RIDGE_ARTIFACT_TOKEN,
    build: !has('--no-build'),
    dryRun: has('--dry-run'),
    rollback,
  };
}
