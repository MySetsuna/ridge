#!/usr/bin/env node
// 分支审查导读包：按 conventional commit 分组，并标注协议/安全触及面。
import { execFileSync } from 'node:child_process';
import { writeFileSync, mkdirSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
export const RANGE = 'origin/main..HEAD';
export const PROTOCOL_PATHS = [
  'packages/ridge-core/src/capability.rs',
  'packages/remote/src/shared/cloud/remoteAllowlist.ts',
  'packages/remote/src/shared/transport/capabilityContract.ts',
  'docs/capability-matrix.json',
];
export const SECURITY_HINTS = ['hitl', 'e2ee', 'totp', 'trust', 'suspend', 'keybinding', 'auth', 'cloudhostbridge', 'fallback', 'security'];
const ORDER = ['feat', 'fix', 'refactor', 'test', 'docs', 'other'];

function parseSubject(subject) {
  const colon = subject.indexOf(':');
  if (colon < 1) return { type: 'other', scope: '', title: subject };
  const prefix = subject.slice(0, colon);
  const bang = prefix.endsWith('!') ? prefix.slice(0, -1) : prefix;
  const open = bang.indexOf('(');
  if (open < 1 || !bang.endsWith(')')) return { type: bang, scope: '', title: subject.slice(colon + 1).trim() };
  return { type: bang.slice(0, open), scope: bang.slice(open + 1, -1), title: subject.slice(colon + 1).trim() };
}

export function collectCommits(git, range = RANGE) {
  const count = Number(git('rev-list', '--count', range));
  const shas = git('rev-list', '--reverse', range).split('\n').filter(Boolean);
  if (shas.length !== count) throw new Error(`self-check failed: rev-list count ${count} != enumerated ${shas.length}`);
  return shas.map((sha) => {
    const subject = git('log', '-1', '--format=%s', sha);
    const stat = git('show', '--stat', '--format=', sha).split('\n').filter(Boolean);
    const files = git('show', '--name-only', '--format=', sha).split('\n').filter(Boolean);
    const match = parseSubject(subject);
    const tags = [];
    if (files.some((f) => PROTOCOL_PATHS.includes(f))) tags.push('协议面');
    if (files.some((f) => SECURITY_HINTS.some((h) => f.toLowerCase().includes(h)))) tags.push('安全面');
    return { sha: sha.slice(0, 7), type: match.type, scope: match.scope, title: match.title, files: files.length, summary: stat.at(-1) ?? '', tags };
  });
}

export function renderReviewGuide(commits, range = RANGE) {
  const byType = new Map();
  for (const commit of commits) {
    if (!byType.has(commit.type)) byType.set(commit.type, []);
    byType.get(commit.type).push(commit);
  }
  const types = [...byType.keys()].sort((a, b) => (ORDER.indexOf(a) + 99) % 99 - (ORDER.indexOf(b) + 99) % 99);
  const protocolTouches = commits.filter((c) => c.tags.includes('协议面'));
  const securityTouches = commits.filter((c) => c.tags.includes('安全面'));
  const guideLine = (commit) => {
    const scope = commit.scope ? `**${commit.scope}** ` : '';
    const summary = commit.summary.replaceAll('|', '/');
    const tags = commit.tags.join(' ') || '—';
    return `| \`${commit.sha}\` | ${scope}${commit.title} | ${commit.files} | ${summary} | ${tags} |`;
  };
  const lines = [
    '# 分支审查导读 — `codex/remote-git-diff-iteration-1`', '',
    `生成：\`node scripts/generate-review-pack.mjs\`（范围 \`${range}\`，共 **${commits.length}** 提交；手改无效，重跑刷新）。`, '',
    '## 审查优先级建议', '',
    `1. **协议面提交（${protocolTouches.length}）**：动了 allowlist/能力合同/矩阵——远端可达面变化，逐条核。`,
    `2. **安全面提交（${securityTouches.length}）**：hitl/e2ee/totp/trust/suspend 路径。`,
    '3. 其余按类型抽查；docs 类可速览。', '',
    '## 协议面提交清单', '',
    ...protocolTouches.map((c) => `- \`${c.sha}\` ${c.type}${c.scope ? `(${c.scope})` : ''}: ${c.title}`), '',
    ...types.flatMap((type) => {
      const list = byType.get(type);
      return [`## ${type}（${list.length}）`, '', '| SHA | 标题 | 文件数 | 变更量 | 标注 |', '| --- | --- | --- | --- | --- |', ...list.map(guideLine), ''];
    }),
  ];
  return lines.join('\n');
}

export function main({ rootDir = root, execFileSyncImpl = execFileSync, fsImpl = { mkdirSync, writeFileSync }, io = console } = {}) {
  const git = (...args) => execFileSyncImpl('git', args, { cwd: rootDir, encoding: 'utf8' }).toString().trim();
  const commits = collectCommits(git);
  const content = renderReviewGuide(commits);
  fsImpl.mkdirSync(resolve(rootDir, 'docs/review'), { recursive: true });
  const out = resolve(rootDir, 'docs/review/branch-review-guide.md');
  fsImpl.writeFileSync(out, content);
  io.log(`review pack: ${commits.length} commits → ${out}`);
  return { count: commits.length, out };
}

if (process.argv[1] && resolve(process.argv[1]) === resolve(fileURLToPath(import.meta.url))) {
  try { main(); } catch (error) { console.error(error.message); process.exit(1); }
}
