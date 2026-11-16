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

const suites = [evidence.sharedUiAcceptance, ...(evidence.sharedPermissionAcceptance ?? [])];
const expectedSuites = new Map([
  ['L4-COMMUNE-UI-ACCEPTANCE', [['node', 'scripts/commune-global-ui-acceptance.mjs'], 6]],
  ['L4-COMMUNE-UI-MEMBERSHIP-ACCEPTANCE', [['node', 'scripts/commune-permission-ui-acceptance.mjs', '--membership'], 2]],
  ['L4-COMMUNE-UI-PERMISSION-ACCEPTANCE', [['node', 'scripts/commune-permission-ui-acceptance.mjs'], 3]],
]);
assert.deepEqual(suites.map(suite => suite?.unitId).sort(), [...expectedSuites.keys()].sort());
for (const suite of suites) {
  const [command, count] = expectedSuites.get(suite.unitId);
  assert.deepEqual(suite.command, command);
  const unit = graph.nodes.find(node => node.id === suite.unitId);
  assert.ok(unit, `Missing shared UI unit ${suite.unitId}`);
  const requirements = suite.requirements.map(record => record.nodeId).sort();
  assert.equal(requirements.length, count, `Duplicate requirement mappings ${suite.unitId}`);
  assert.equal(new Set(requirements).size, count, `Incomplete requirements ${suite.unitId}`);
  assert.deepEqual([...unit.fields.depends_on].sort(), requirements);
  assert.equal(unit.fields.acceptance_checks.length, 1);
  assert.deepEqual(unit.fields.acceptance_checks[0].command, command);
  for (const record of suite.requirements) {
    const requirement = graph.nodes.find(node => node.id === record.nodeId);
    assert.ok(requirement, `Missing UI requirement ${record.nodeId}`);
    assert.ok(requirement.testTargets.includes(command[1]), `Lost UI target ${record.nodeId}`);
    assert.ok(requirement.body.includes(suite.unitId), `Lost UI mapping ${record.nodeId}`);
    assert.ok(!(requirement.fields.acceptance_checks ?? []).some(check =>
      JSON.stringify(check.command) === JSON.stringify(command)), `Duplicate UI execution ${record.nodeId}`);
  }
}

// Batch the same Git queries; do not start two Git processes per record.
// Observation archives contain metadata only, never private local settings.
const records = [
  ...evidence.sources.map(source => ({ ...source, source: source.old, canonical: true })),
  ...(evidence.observationArchives ?? []),
];
const gitLines = args => execFileSync('git', args, {
  cwd: root, encoding: 'utf8', windowsHide: true, maxBuffer: 16 * 1024 * 1024,
}).trim().split(/\r?\n/);
const rawBlobs = gitLines(['hash-object', '--no-filters', '--', ...records.map(record => record.archive)]);
const originalBlobs = gitLines(['rev-parse', ...records.map(record => `${record.commit}:${record.source}`)]);
assert.equal(rawBlobs.length, records.length, 'Incomplete archive fingerprints');
assert.equal(originalBlobs.length, records.length, 'Incomplete source provenance');
for (const [index, record] of records.entries()) {
  // Existing Markdown archives may have checkout-converted newlines; retain
  // the original per-source Git filter fallback, never apply it to observations.
  const blob = record.canonical && rawBlobs[index] !== record.blob
    ? gitLines(['hash-object', '--path', record.source, record.archive])[0]
    : rawBlobs[index];
  assert.equal(blob, record.blob, `Changed historical source ${record.source}`);
  assert.equal(originalBlobs[index], record.blob, `Invalid provenance ${record.source}`);
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
  assert.ok(node.body.includes(`${disposition.change ?? 'CHG-090'} 映射处置`), `Missing decision ${node.id}`);
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
