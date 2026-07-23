#!/usr/bin/env node
// T3：一次命令产出生产「两条版本线」只读证据（不写任何生产状态）。
//   线 1（应用服务）：GET /api/v1/health           → ridge-cloud 服务版本 + uptime
//   线 2（Remote 产物）：GET /api/v1/remote-artifacts/status → 激活版本 + 保留 releases
//
// 用法：node scripts/check-prod-status.mjs --base-url https://<cloud-domain>
// 环境：RIDGE_ARTIFACT_TOKEN（线 2 必需；缺失时线 2 如实报「未验证」，不伪造）
// 退出码：0 = 请求全部完成（含显式「未验证」）；1 = 任一已尝试的请求失败。

const args = process.argv.slice(2);
if (args.includes('--help') || args.includes('-h')) {
  console.log(`用法: node scripts/check-prod-status.mjs --base-url <https://cloud-domain>
环境变量: RIDGE_ARTIFACT_TOKEN  artifact 状态端点的 Bearer token（缺失 → 线 2 记「未验证」）
语义: 只读探测,零写入。缺 token/缺 --base-url 时对应线记「未验证」而非伪造结论。`);
  process.exit(0);
}
const baseUrl = (args[args.indexOf('--base-url') + 1] || '').replace(/\/$/, '');
const token = process.env.RIDGE_ARTIFACT_TOKEN;

const evidence = {
  probedAt: new Date().toISOString(),
  baseUrl: baseUrl || null,
  service: { status: '未验证', detail: null },
  artifacts: { status: '未验证', detail: null },
};

async function probe(path, headers) {
  const res = await fetch(`${baseUrl}${path}`, { headers });
  const body = await res.json().catch(() => null);
  if (!res.ok || body?.ok === false) {
    return { status: `失败（HTTP ${res.status}）`, detail: body?.error ?? null };
  }
  return { status: '通过', detail: body?.data ?? body };
}

let failed = false;
if (!baseUrl) {
  console.error('缺 --base-url：两条线均记「未验证」。');
} else {
  try {
    evidence.service = await probe('/api/v1/health');
  } catch (e) {
    evidence.service = { status: '失败（网络）', detail: String(e) };
    failed = true;
  }
  if (token) {
    try {
      evidence.artifacts = await probe('/api/v1/remote-artifacts/status', {
        Authorization: `Bearer ${token}`,
      });
    } catch (e) {
      evidence.artifacts = { status: '失败（网络）', detail: String(e) };
      failed = true;
    }
  } else {
    evidence.artifacts.detail = '缺 RIDGE_ARTIFACT_TOKEN';
  }
  if (evidence.service.status.startsWith('失败') || evidence.artifacts.status.startsWith('失败'))
    failed = true;
}

console.log(JSON.stringify(evidence, null, 2));
process.exit(failed ? 1 : 0);
