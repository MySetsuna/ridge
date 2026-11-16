// One manifest in the authoritative documentation spec; this is its verifier.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';

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
const round = manifest.sourceRound;
assert.match(round.commit, /^[a-f0-9]{40}$/);
assert.match(round.changeId, /^CHG-\d+$/);
for (const [file, digest, sourcePath] of [
  [round.changeArchive, round.changeSha256, `changes/${round.changeId}.md`],
  [round.proposalArchive, round.proposalSha256, `.spectree/proposals/archive/${round.changeId}.json`],
]) {
  assert.ok(file.startsWith('.spectree/proposals/archive/') && !file.split('/').includes('..'));
  const bytes = fs.readFileSync(path.join(root, file));
  const original = execFileSync('git', ['show', `${round.commit}:${sourcePath}`], { cwd: root, windowsHide: true });
  assert.equal(createHash('sha256').update(bytes).digest('hex'), digest, `changed source-round archive ${file}`);
  assert.ok(bytes.equals(original), `invalid source-round provenance ${file}`);
  assert.ok(index.includes(file), `missing readable source round ${file}`);
}
assert.match(read(round.changeArchive), /^status:\s*COMPLETED\s*$/m);
assert.equal(JSON.parse(read(round.proposalArchive)).changeId, round.changeId);
console.log(`Proposal archive consistency PASS: ${manifest.records.length} sources and one Git-identical completed source round`);
