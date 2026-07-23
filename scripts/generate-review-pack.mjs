#!/usr/bin/env node
// 分支审查导读包（iteration 9 G3）：把 origin/main..HEAD 全部提交按 conventional
// type(scope) 分组，标注触及面（协议面/安全面），输出 docs/review/branch-review-guide.md，
// 降低用户合并审查成本。只做导读不做合并自动化。幂等可重跑。
import { execFileSync } from 'node:child_process';
import { writeFileSync, mkdirSync } from 'node:fs';
import { resolve } from 'node:path';

const root = resolve(import.meta.dirname, '..');
const git = (...args) => execFileSync('git', args, { cwd: root, encoding: 'utf8' }).trim();

const RANGE = 'origin/main..HEAD';
const PROTOCOL_PATHS = [
  'packages/ridge-core/src/capability.rs',
  'packages/remote/src/shared/cloud/remoteAllowlist.ts',
  'packages/remote/src/shared/transport/capabilityContract.ts',
  'docs/capability-matrix.json',
];
const SECURITY_HINTS = ['hitl', 'e2ee', 'totp', 'trust', 'suspend', 'keybinding', 'auth', 'cloudhostbridge', 'fallback', 'security'];

const count = Number(git('rev-list', '--count', RANGE));
const shas = git('rev-list', '--reverse', RANGE).split('\n').filter(Boolean);
if (shas.length !== count) {
  console.error(`self-check failed: rev-list count ${count} != enumerated ${shas.length}`);
  process.exit(1);
}

const commits = shas.map((sha) => {
  const subject = git('log', '-1', '--format=%s', sha);
  const stat = git('show', '--stat', '--format=', sha).split('\n').filter(Boolean);
  const files = git('show', '--name-only', '--format=', sha).split('\n').filter(Boolean);
  const summary = stat.at(-1) ?? '';
  const m = subject.match(/^(\w+)(?:\(([^)]+)\))?!?:\s*(.*)$/);
  const tags = [];
  if (files.some((f) => PROTOCOL_PATHS.some((p) => f === p))) tags.push('协议面');
  if (files.some((f) => SECURITY_HINTS.some((h) => f.toLowerCase().includes(h)))) tags.push('安全面');
  return {
    sha: sha.slice(0, 7),
    type: m?.[1] ?? 'other',
    scope: m?.[2] ?? '',
    title: m?.[3] ?? subject,
    files: files.length,
    summary,
    tags,
  };
});

const byType = new Map();
for (const c of commits) {
  if (!byType.has(c.type)) byType.set(c.type, []);
  byType.get(c.type).push(c);
}
const ORDER = ['feat', 'fix', 'refactor', 'test', 'docs', 'other'];
const types = [...byType.keys()].sort(
  (a, b) => (ORDER.indexOf(a) + 99) % 99 - (ORDER.indexOf(b) + 99) % 99,
);

const protocolTouches = commits.filter((c) => c.tags.includes('协议面'));
const securityTouches = commits.filter((c) => c.tags.includes('安全面'));

const lines = [
  '# 分支审查导读 — `codex/remote-git-diff-iteration-1`',
  '',
  `生成：\`node scripts/generate-review-pack.mjs\`（范围 \`${RANGE}\`，共 **${count}** 提交；手改无效，重跑刷新）。`,
  '',
  '## 审查优先级建议',
  '',
  `1. **协议面提交（${protocolTouches.length}）**：动了 allowlist/能力合同/矩阵——远端可达面变化，逐条核。`,
  `2. **安全面提交（${securityTouches.length}）**：hitl/e2ee/totp/trust/suspend 路径。`,
  '3. 其余按类型抽查；docs 类可速览。',
  '',
  '## 协议面提交清单',
  '',
  ...protocolTouches.map((c) => `- \`${c.sha}\` ${c.type}${c.scope ? `(${c.scope})` : ''}: ${c.title}`),
  '',
  ...types.flatMap((t) => {
    const list = byType.get(t);
    return [
      `## ${t}（${list.length}）`,
      '',
      '| SHA | 标题 | 文件数 | 变更量 | 标注 |',
      '| --- | --- | --- | --- | --- |',
      ...list.map(
        (c) =>
          `| \`${c.sha}\` | ${c.scope ? `**${c.scope}** ` : ''}${c.title} | ${c.files} | ${c.summary.replace(/\|/g, '/')} | ${c.tags.join(' ') || '—'} |`,
      ),
      '',
    ];
  }),
];

mkdirSync(resolve(root, 'docs', 'review'), { recursive: true });
const out = resolve(root, 'docs', 'review', 'branch-review-guide.md');
writeFileSync(out, lines.join('\n'));
console.log(`review pack: ${count} commits (${protocolTouches.length} 协议面 / ${securityTouches.length} 安全面) → ${out}`);
