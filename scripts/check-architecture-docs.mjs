import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { readFileSync, existsSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('../', import.meta.url));
const names = [
  'README', '00-overview', '10-kernel', '20-rtp1-wire',
  '30-remote-lifecycle', '40-cli', '50-desktop-tauri',
  '60-data-flow', '70-runtime-epoch', '80-security', '90-performance',
];
const files = ['README.md', ...names.map(name => `docs/architecture/${name}.md`)];
const read = file => readFileSync(path.join(root, file), 'utf8');

for (const file of [...files, 'docs/architecture/notes/README.md']) {
  for (const match of read(file).matchAll(/!?\[[^\]]*\]\(([^)]+)\)/g)) {
    const target = match[1].split(/[?#]/)[0];
    if (!target || /^[a-z]+:/i.test(target)) continue;
    const resolved = path.resolve(root, path.dirname(file), decodeURIComponent(target));
    assert.ok(existsSync(resolved), `${file} links to missing ${target}`);
  }
}

const index = read('docs/architecture/README.md');
for (const spec of [
  'l2-unified-client-runtime', 'l3-cli-independent-terminal-creation',
  'l3-remote-interaction-reliability', 'l3-commune-protocol-membership',
]) assert.ok(index.includes(`../../specs/${spec}.md`), `Missing authority ${spec}`);

for (const name of names) {
  const file = `docs/architecture/${name}.md`;
  const original = execFileSync('git', ['rev-parse', `d8e64e5b:${file}`], { cwd: root, encoding: 'utf8' });
  const archive = `docs/archive/CHG-089/architecture/${name}.md`;
  // Compare canonical Git bytes while honoring checkout newline conversion.
  const archived = execFileSync('git', ['hash-object', '--path', file, archive], { cwd: root, encoding: 'utf8' });
  assert.equal(archived, original, `Changed historical source ${name}`);
}

for (const file of [
  'docs/architecture/RIDGE-RUNTIME-FOUNDATION-FINAL.md',
  ...['PROTO', 'REMOTE', 'TERM', 'PERF'].map(name => `docs/architecture/specs/L2-${name}-001.md`),
]) assert.match(read(file).split('\n')[0], /历史.*\[/, `Missing historical boundary ${file}`);

assert.doesNotMatch(index, /本目录是项目的权威架构说明|副本只是过渡锚点/);
assert.match(read('docs/architecture/40-cli.md'), /workspaceId、paneId、runtimeEpoch/);
assert.match(read('docs/architecture/40-cli.md'), /Cloud，无显式 shell\/CWD/);
assert.match(read('docs/architecture/60-data-flow.md'), /Host 全局成员/);
console.log(`Architecture docs PASS, ${files.length} current files and ${names.length} Git-identical archives`);
