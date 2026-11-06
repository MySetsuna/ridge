import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('../', import.meta.url));

function inspect(command) {
  const options = { cwd: root, encoding: 'utf8', windowsHide: true, maxBuffer: 64 * 1024 * 1024 };
  const output = process.platform === 'win32'
    ? execFileSync(process.env.ComSpec ?? 'cmd.exe', ['/d', '/s', '/c', `stc ${command} --json`], options)
    : execFileSync('stc', [command, '--json'], options);
  return JSON.parse(output);
}

const graph = inspect('graph');
const governance = graph.nodes.find(node => node.id === 'L3-SPEC-MAPPING-GOVERNANCE');
assert.ok(governance, 'Missing mapping governance contract');
const evidenceBlock = governance.body.match(/## 处置证据\s+```json\n([\s\S]*?)\n```/);
assert.ok(evidenceBlock, 'Missing disposition evidence');
const evidence = JSON.parse(evidenceBlock[1]);
assert.equal(evidence.sources.length, 98, 'Incomplete original path inventory');

for (const source of evidence.sources) {
  const rawBlob = execFileSync('git', ['hash-object', '--no-filters', source.archive], {
    cwd: root, encoding: 'utf8', windowsHide: true,
  }).trim();
  // Old blobs may already contain CRLF, while existing archives may be
  // checkout-converted. Accept the exact blob or Git's canonical form.
  const blob = rawBlob === source.blob ? rawBlob : execFileSync('git', [
    'hash-object', '--path', source.old, source.archive,
  ], { cwd: root, encoding: 'utf8', windowsHide: true }).trim();
  assert.equal(blob, source.blob, `Changed historical source ${source.old}`);
  const original = execFileSync('git', ['rev-parse', `${source.commit}:${source.old}`], {
    cwd: root, encoding: 'utf8', windowsHide: true,
  }).trim();
  assert.equal(original, source.blob, `Invalid provenance ${source.old}`);
}

for (const disposition of evidence.nodes) {
  const node = graph.nodes.find(candidate => candidate.id === disposition.id);
  assert.ok(node, `Lost disposition node ${disposition.id}`);
  const lifecycle = node.fields.lifecycle ?? 'ACTIVE';
  const historical = ['retire', 'archived-evidence'].includes(disposition.action);
  if (historical) {
    assert.ok(['RETIRING', 'ARCHIVED'].includes(lifecycle), `Still active ${node.id}`);
    assert.ok(node.codeTargets.length > 0, `Missing historical evidence ${node.id}`);
    assert.ok(node.codeTargets.every(target => target.startsWith('docs/archive/')), `Live target on historical record ${node.id}`);
  } else {
    assert.ok(!['RETIRING', 'ARCHIVED'].includes(lifecycle), `Retired live requirement ${node.id}`);
    assert.ok(node.codeTargets.every(target => !target.startsWith('docs/archive/')), `Archive presented as live implementation ${node.id}`);
  }
  assert.match(node.body, /CHG-090 映射处置/, `Missing decision ${node.id}`);
}

const historicalIds = new Set(evidence.nodes
  .filter(node => ['retire', 'archived-evidence'].includes(node.action))
  .map(node => node.id));
for (const node of graph.nodes) {
  if (historicalIds.has(node.parent)) {
    assert.ok(['RETIRING', 'ARCHIVED'].includes(node.fields.lifecycle), `Live child under retired parent ${node.id}`);
  }
  for (const target of [...node.codeTargets, ...node.testTargets]) {
    if (!target.includes('*')) {
      assert.ok(existsSync(new URL(`../${target}`, import.meta.url)), `Missing target ${node.id} ${target}`);
    }
  }
}

const audit = inspect('audit');
const errors = audit.findings.filter(finding => finding.severity === 'ERROR');
assert.equal(errors.length, 0, JSON.stringify(errors));
const warnings = {};
for (const finding of audit.findings) {
  if (finding.severity === 'WARNING') warnings[finding.code] = (warnings[finding.code] ?? 0) + 1;
}
console.log(JSON.stringify({ status: 'PASS', sources: evidence.sources.length, dispositions: evidence.nodes.length, remainingWarnings: warnings }));
