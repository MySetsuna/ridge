import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

const source = readFileSync(new URL('./SplitContainer.svelte', import.meta.url), 'utf8');
const paneSource = readFileSync(new URL('./RidgePane.svelte', import.meta.url), 'utf8');
const communeSource = readFileSync(new URL('../teammate/AgentCenterPanel.svelte', import.meta.url), 'utf8');
const memberSource = readFileSync(new URL('../teammate/AgentMemberRow.svelte', import.meta.url), 'utf8');

describe('desktop Pane Agent border contract', () => {
  it('scopes Git polling to the active workspace', () => {
    expect(source).toContain('workspaceId === $activeWorkspaceId');
    expect(source).toContain('trackPaneGitStatus(node.id, cwd || null)');
  });

  it('paints a high-stack status highlight without stealing pane input', () => {
    expect(source).toContain('{@const paneAttention = $agentPaneAttentionStore');
    expect(source).toContain("paneAttention === 'waiting'");
    expect(source).toContain("paneAttention === 'stopped'");
    expect(source).toContain("paneAttention === 'idle'");
    expect(source).toContain("paneStatus === 'working'");
    expect(source).toContain("paneStatus === 'idle'");
    expect(source).toContain('rg-agent-pane-highlight');
    expect(source).toContain('class="rg-agent-pane-shell relative z-[1] isolate');
    expect(source).toContain('data-agent-attention={paneAttention ?? \'\'}');
    expect(source).toContain('z-index: 80;');
    expect(paneSource).toContain('contain: strict; z-index: 1;');
    expect(paneSource).toContain('z-index: 90;');
    expect(paneSource).toContain('clearAgentPaneAttention(workspaceId, paneId)');
    expect(paneSource).toContain('if (container?.contains(document.activeElement))');
    expect(paneSource).toContain('function onImeHelperFocus()');
    expect(paneSource).not.toContain('if (isActive) clearAgentPaneAttention(workspaceId, paneId);');
  });

  it('keeps PaneHeader Git pill at one sibling layer', () => {
    expect(source.match(/<PaneGitPill\b/g) ?? []).toHaveLength(1);
    expect(source.match(/<PaneDiffPill\b/g) ?? []).toHaveLength(1);
    expect(source.match(/<PaneRepoSwitcher\b/g) ?? []).toHaveLength(1);
    expect(source).toMatch(
      /<PaneRepoSwitcher paneId=\{node\.id\} \/>\r?\n\s+<PaneGitPill paneId=\{node\.id\} \/>\r?\n\s+<PaneDiffPill paneId=\{node\.id\} \/>/,
    );
  });

  it('keeps Commune titles on the PaneHeader live stores', () => {
    expect(communeSource).toContain('$terminalTitles[paneId]');
    expect(communeSource).toContain('$paneForegroundProcessStore[paneId]');
    expect(communeSource).toContain('$paneCwdStore');
    expect(communeSource).toContain('displayTitle={livePaneTitles.get');
    expect(memberSource).toContain('data-agent-attention={attention ?? \'\'}');
    expect(memberSource).toContain("attention === 'idle'");
    expect(memberSource).toContain('onfocusin={acknowledgeAttention}');
    expect(memberSource).toContain('onpointerdown={acknowledgeAttention}');
    expect(memberSource).not.toContain('profile?.recentOutput');
  });

  it('only arms idle attention after a working-to-idle transition', () => {
    expect(communeSource).toContain('agentAttentionForTransition');
    expect(communeSource).toContain('observedAgentStatuses');
    expect(communeSource).toContain('get(agentPaneAttentionStore)[key]');
    expect(communeSource).toContain("profile.outputSeq > 0");
    expect(communeSource).not.toContain('setAgentPaneAttention(oldWorkspaceId, oldPaneId, null)');
  });
});
