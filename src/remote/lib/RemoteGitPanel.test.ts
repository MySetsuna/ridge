import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

const source = readFileSync(new URL('./RemoteGitPanel.svelte', import.meta.url), 'utf8');

describe('remote Git panel contract', () => {
  it('keeps the mobile write surface behind a provider capability gate', () => {
    expect(source).toContain('hasRemoteGitWriteCapability(provider)');
    expect(source).toContain("runAction('commit'");
    expect(source).toContain("runAction('push'");
    expect(source).toContain('actionController?.abort()');
  });

  it('offers a dedicated Changes/Graph tab and reuses the shared graph renderer', () => {
    expect(source).toContain("view = 'changes'");
    expect(source).toContain("view = 'graph'");
    expect(source).toContain("import GitGraph from '../../lib/components/GitGraph.svelte'");
    expect(source).toContain('parents: commit.parents ??');
    expect(source).toContain('const branchNames = $derived(');
    expect(source).toContain('info.branches.length > 0');
    expect(source).toContain("commit.refs?.includes('head:')");
    expect(source).toContain('Selected commit');
    expect(source).toContain('selectedHash = commit.hash');
  });
});
