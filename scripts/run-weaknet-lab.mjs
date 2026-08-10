#!/usr/bin/env node
// G2（iteration 7 / R1 实验室轨）：跑弱网实验室参数化扫描并校验 metrics 结构。
// 产出 artifacts/weak-net-lab/metrics.json（gitignored）。实验室确定性模型，
// 非真机结论——JSON 内含 disclaimer，禁止用于宣称双平台/生产弱网表现。
import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { systemTool } from './lib/toolPath.mjs';

const root = resolve(import.meta.dirname, '..');
try {
  execFileSync(
    systemTool('pnpm'),
    ['exec', 'vitest', 'run', 'packages/remote/src/shared/cloud/weakNetLab.test.ts'],
    { cwd: root, stdio: 'inherit', shell: process.platform === 'win32' },
  );
} catch {
  console.error('weak-net lab 场景未全绿');
  process.exit(1);
}

const path = resolve(root, 'artifacts', 'weak-net-lab', 'metrics.json');
const m = JSON.parse(readFileSync(path, 'utf8'));
const fail = (msg) => {
  console.error(`metrics 结构校验失败: ${msg}`);
  process.exit(1);
};
if (m.model !== 'deterministic-lab') fail('model 非 deterministic-lab');
if (typeof m.disclaimer !== 'string' || !m.disclaimer.includes('非真机')) fail('缺 disclaimer');
if (!Array.isArray(m.scenarios) || m.scenarios.length < 9) fail(`场景数 ${m.scenarios?.length} < 9`);
for (const s of m.scenarios) {
  if (!s.family || typeof s.params !== 'object' || typeof s.observed !== 'object')
    fail(`场景缺字段: ${JSON.stringify(s)}`);
}
console.log(`weak-net lab 全绿：${m.scenarios.length} 场景 → ${path}`);
