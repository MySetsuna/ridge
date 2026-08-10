#!/usr/bin/env node
// C1 半自动收口（iteration 9 G2）：从 docs/capability-matrix.json（A2 机器可读
// 事实源，有 6 条一致性测试守卫）派生 rdg 无头 host 与桌面/云 host 的语义缺口
// 清单，写 docs/audits/rdg-gap-report.md。幂等可重跑；不写任何 rdg 功能代码。
import { readFileSync, writeFileSync, mkdirSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');

// C1 逐缺口人工判定（iteration 10 G4）：判定数据与报告同源共存于本脚本，重跑即刷新。
// 「补路由候选」= ridge-core dispatch 已共享实现、待真实需求触发接线；改判需过对抗评审。
export const JUDGMENTS = {
  teammate: '刻意排除（无头环境无 Agent Center 宿主；重开需 D6 安全评审）',
  theme: '永久缺口（rdg 无 UI 宿主，无主题渲染面；不宣告即语义完备）',
  git: '补路由候选（ridge-core dispatch 已共享实现，待真实 rdg 场景需求触发接线）',
  workspace: '补路由候选（同上；接线时须过 A2 宣告纪律与合同测试）',
  invoke: '不宣告即语义完备（控制器最小方法集为空，无可路由项）',
};

export function buildGapReport(matrix) {
  const rows = Object.entries(matrix.capabilities).map(([name, c]) => ({ name, methods: c.methods, rdgHost: c.cells.rdgHost, desktop: c.cells.desktop }));
  const supported = rows.filter((r) => r.rdgHost === 'supported');
  const denied = rows.filter((r) => r.rdgHost === 'denied');
  const other = rows.filter((r) => !['supported', 'denied'].includes(r.rdgHost));
  const lines = [
  '# rdg 无头 host 语义缺口报告（C1，自动派生）',
  '',
  `生成：\`node scripts/rdg-gap-report.mjs\`（源 = \`docs/capability-matrix.json\`，由 ${matrix.guards?.length ?? 0} 条一致性测试守卫）。手改无效，重跑脚本刷新。`,
  '',
  `## rdg 已支持能力（${supported.length}）`,
  '',
  ...supported.map((r) => `- **${r.name}**：${r.methods.length} 方法（${r.methods.join(', ')}）`),
  '',
  `## rdg 缺口（denied，${denied.length}）——桌面/云 host 有而 rdg 无`,
  '',
  '| 能力 | 方法数 | 缺失语义 | 收口判定 |',
  '| --- | --- | --- | --- |',
  ...denied.map(
    (r) =>
      `| ${r.name} | ${r.methods.length} | ${r.methods.join(', ')} | ${
        JUDGMENTS[r.name] ?? '待人工判定：补路由 or 声明永久缺口'
      } |`,
  ),
  '',
  other.length ? `## 其他状态（${other.length}）` : '',
  ...other.map((r) => `- ${r.name}: ${r.rdgHost}`),
  '',
  '## 语义一致性原则（锁定决策）',
  '',
  '能力必须先协商宣告，未宣告入口**显式拒绝**而非静默分叉（跨入口合同测试守卫）。rdg 对 denied 能力的正确行为 = 不宣告 + 拒绝对应方法调用；上表「收口判定」列指导后续是否补齐路由。',
  '',
  ].filter((l) => typeof l === 'string');
  return { content: lines.join('\n'), supported: supported.length, denied: denied.length };
}

export function main({ rootDir = root, fsImpl = { readFileSync, writeFileSync, mkdirSync }, io = console } = {}) {
  const matrix = JSON.parse(fsImpl.readFileSync(resolve(rootDir, 'docs', 'capability-matrix.json'), 'utf8'));
  const report = buildGapReport(matrix);
  fsImpl.mkdirSync(resolve(rootDir, 'docs', 'audits'), { recursive: true });
  const out = resolve(rootDir, 'docs', 'audits', 'rdg-gap-report.md');
  fsImpl.writeFileSync(out, report.content);
  io.log(`rdg gap report: ${report.supported} supported / ${report.denied} denied → ${out}`);
  return report;
}

if (process.argv[1] && resolve(process.argv[1]) === resolve(fileURLToPath(import.meta.url))) main();
