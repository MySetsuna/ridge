// One manifest in the authoritative documentation spec; this is its verifier.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { createHash } from 'node:crypto';

const root = fileURLToPath(new URL('../', import.meta.url));
const read = file => fs.readFileSync(path.join(root, file), 'utf8');
const source = read('specs/l3-architecture-documentation.md');
const manifest = JSON.parse(source.match(/```json\r?\n([\s\S]+?)\r?\n```/)[1]);
assert.equal(manifest.version, 1);
const index = read('docs/architecture/proposals.md');
const ids = new Set(fs.readdirSync(path.join(root, 'specs')).filter(name => name.endsWith('.md'))
  .map(name => read(`specs/${name}`).match(/^id:\s*(\S+)/m)?.[1]).filter(Boolean));
const archives = new Set();
for (const record of manifest.records) {
  assert.ok(record.archive.startsWith('.spectree/proposals/archive/'));
  assert.ok(!record.archive.split('/').includes('..'));
  assert.ok(!archives.has(record.archive), `duplicate archive ${record.archive}`); archives.add(record.archive);
  let bytes = fs.readFileSync(path.join(root, record.archive));
  assert.ok(['bytes', 'lf'].includes(record.hashMode));
  if (record.hashMode === 'lf') bytes = Buffer.from(bytes.toString('utf8').replaceAll('\r\n', '\n'));
  assert.equal(createHash('sha256').update(bytes).digest('hex'), record.sha256, `changed proposal ${record.archive}`);
  const proposal = JSON.parse(bytes.toString('utf8').replace(/^\uFEFF/, ''));
  const change = read(`changes/${record.change}.md`);
  assert.match(change, /^status:\s*COMPLETED\s*$/m, `unfinished destination ${record.change}`);
  for (const node of proposal.nodes) {
    assert.ok(ids.has(node.id), `missing node ${node.id}`);
    assert.ok(change.includes(`  - ${node.id}`), `wrong destination ${record.change} for ${node.id}`);
  }
  assert.ok(index.includes(record.archive) && index.includes(record.change), `missing readable mapping ${record.source}`);
  if (record.hashMode === 'bytes') {
    const oldName = record.source.split('/').at(-1);
    assert.ok(!fs.existsSync(path.join(root, '.spectree/proposals', oldName)), `historical proposal remains active: ${oldName}`);
  }
}
console.log(`Proposal archive consistency PASS: ${manifest.records.length} sources, fingerprints and completed change mappings`);
