import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

const source = readFileSync(new URL('./SplitContainer.svelte', import.meta.url), 'utf8');
const paneSource = readFileSync(new URL('./RidgePane.svelte', import.meta.url), 'utf8');

describe('desktop Pane Agent border contract', () => {
  it('uses transient attention only; runtime status never paints a border', () => {
    expect(source).toContain('{@const paneAttention = $agentPaneAttentionStore');
    expect(source).toContain("paneAttention === 'waiting'");
    expect(source).toContain("paneAttention === 'stopped'");
    expect(source).not.toContain("paneStatus === 'working'");
    expect(source).not.toContain("paneStatus === 'idle'");
    expect(source).toContain('data-agent-attention={paneAttention ?? \'\'}');
    expect(paneSource).toContain('clearAgentPaneAttention(workspaceId, paneId)');
    expect(paneSource).toContain('if (container?.contains(document.activeElement))');
    expect(paneSource).toContain('if (isActive) clearAgentPaneAttention(workspaceId, paneId);');
  });

  it('keeps PaneHeader Git pill at one sibling layer', () => {
    expect((source.match(/<PaneGitPill\b/g) ?? []).length).toBe(1);
    expect((source.match(/<PaneDiffPill\b/g) ?? []).length).toBe(1);
    expect((source.match(/<PaneRepoSwitcher\b/g) ?? []).length).toBe(1);
    expect(source).toMatch(
      /<PaneRepoSwitcher paneId=\{node\.id\} \/>\r?\n\s+<PaneGitPill paneId=\{node\.id\} \/>\r?\n\s+<PaneDiffPill paneId=\{node\.id\} \/>/,
    );
  });
});
