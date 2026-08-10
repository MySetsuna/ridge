#!/usr/bin/env node
// T3：一次命令产出生产「两条版本线」只读证据（不写任何生产状态）。
//   线 1（应用服务）：GET /api/v1/health           → ridge-cloud 服务版本 + uptime
//   线 2（Remote 产物）：GET /api/v1/remote-artifacts/status → 激活版本 + 保留 releases
//
// 用法：node scripts/check-prod-status.mjs --base-url https://<cloud-domain>
// 环境：RIDGE_ARTIFACT_TOKEN（线 2 必需；缺失时线 2 如实报「未验证」，不伪造）
// 退出码：0 = 请求全部完成（含显式「未验证」）；1 = 任一已尝试的请求失败。

export const HELP = `用法: node scripts/check-prod-status.mjs --base-url <https://cloud-domain>
环境变量: RIDGE_ARTIFACT_TOKEN  artifact 状态端点的 Bearer token（缺失 → 线 2 记「未验证」）
语义: 只读探测,零写入。缺 token/缺 --base-url 时对应线记「未验证」而非伪造结论。`;

export function parseArgs(args = []) {
  const index = args.indexOf('--base-url');
  return { help: args.includes('--help') || args.includes('-h'), baseUrl: (index >= 0 ? args[index + 1] || '' : '').replace(/\/$/, '') };
}

export async function probe(baseUrl, path, headers = {}, fetchImpl = fetch) {
  const res = await fetchImpl(`${baseUrl}${path}`, { headers });
  const body = await res.json().catch(() => null);
  if (!res.ok || body?.ok === false) {
    return { status: `失败（HTTP ${res.status}）`, detail: body?.error ?? null };
  }
  return { status: '通过', detail: body?.data ?? body };
}

export async function collectEvidence({ args = [], env = process.env, fetchImpl = fetch, now = new Date(), io = console } = {}) {
  const parsed = parseArgs(args);
  if (parsed.help) return { help: true, exitCode: 0 };
  const token = env.RIDGE_ARTIFACT_TOKEN;
  const evidence = {
    probedAt: now.toISOString(), baseUrl: parsed.baseUrl || null,
    service: { status: '未验证', detail: null }, artifacts: { status: '未验证', detail: null },
  };
  let failed = false;
  if (!parsed.baseUrl) io.error('缺 --base-url：两条线均记「未验证」。');
  else {
    try { evidence.service = await probe(parsed.baseUrl, '/api/v1/health', {}, fetchImpl); }
    catch (e) { evidence.service = { status: '失败（网络）', detail: String(e) }; failed = true; }
    if (token) {
      try { evidence.artifacts = await probe(parsed.baseUrl, '/api/v1/remote-artifacts/status', { Authorization: `Bearer ${token}` }, fetchImpl); }
      catch (e) { evidence.artifacts = { status: '失败（网络）', detail: String(e) }; failed = true; }
    } else evidence.artifacts.detail = '缺 RIDGE_ARTIFACT_TOKEN';
    if (evidence.service.status.startsWith('失败') || evidence.artifacts.status.startsWith('失败')) failed = true;
  }
  return { evidence, exitCode: failed ? 1 : 0 };
}

export async function main(args = process.argv.slice(2), options = {}) {
  const result = await collectEvidence({ args, ...options });
  if (result.help) { (options.io || console).log(HELP); return result.exitCode; }
  (options.io || console).log(JSON.stringify(result.evidence, null, 2));
  return result.exitCode;
}

if (process.argv[1] && process.argv[1].endsWith('check-prod-status.mjs')) {
  main().then((code) => process.exit(code)).catch((error) => { console.error(error); process.exit(1); });
}
