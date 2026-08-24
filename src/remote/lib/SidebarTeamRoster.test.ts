import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

const source = readFileSync(new URL('./SidebarTeamRoster.svelte', import.meta.url), 'utf8');

describe('remote Agent drawer alignment contract', () => {
  it('uses one centered inline-flex baseline for labels and icons', () => {
    expect(source).toContain('.subtabs button{display:inline-flex;align-items:center;justify-content:center;gap:4px');
    expect(source).toContain('.subtabs :global(svg),.approval :global(svg),.member-head :global(svg),.act :global(svg),.msg-row :global(svg){display:block;flex:0 0 auto}');
    expect(source).toContain('.member-head{display:flex;align-items:center;gap:6px;min-height:24px');
  });

  it('keeps agent state visible as a left color rail', () => {
    expect(source).toContain("agentCardStatus,");
    expect(source).toContain("const key = agentCardStatus(m, pendingFor(m).length > 0);");
    expect(source).toContain('class:status-working={st.key === \'working\'}');
    expect(source).toContain('.member-card.status-waiting{border-left-color:var(--rg-ansi-yellow,#d29922)}');
    expect(source).toContain('.member-card.status-stopped{border-left-color:var(--rg-ansi-red,#f85149)}');
    expect(source).toContain('.member-card.status-idle{border-left-color:var(--rg-fg-muted)}');
  });

  it('maps each Agent card to its live pane CWD and truncates safely', () => {
    expect(source).toContain("m.cwd?.trim() || panes.find((pane) => pane.id === m.paneId)?.cwd?.trim() || ''");
    expect(source).toContain('panes.find((pane) => pane.id === m.paneId)?.cwd?.trim()');
    expect(source).toContain('<span class="member-cwd" title={cwd}>{cwd}</span>');
    expect(source).toContain('.member-cwd{display:block;min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap');
  });

  it('renders the live PaneHeader title while preserving stable Agent identity', () => {
    expect(source).toContain('function titleFor(m: TeammateRosterMember): string');
    expect(source).toContain("m.title?.trim() || m.name");
    expect(source).toContain('<span class="name" title={m.id}>{title}</span>');
  });

  it('refreshes live roster from transport events while history remains cached', () => {
    expect(source).toContain('function scheduleLiveRefresh(): void');
    expect(source).toContain('const offMessage = ws.onMessage');
    expect(source).toContain('const offRaw = ws.onRawBytes');
    expect(source).not.toContain('ROSTER_POLL_INTERVAL_MS');
    expect(source).not.toContain('setInterval(() => void startRefresh()');
  });

  it('raises pane attention on completion/approval edges and projects sticky card state', () => {
    expect(source).toContain('function attentionEvents(');
    expect(source).toContain('agentAttentionForTransition(previous, current, hasPending, member.status)');
    expect(source).toContain('onAttentionChange?.(attentionEvents(t.roster, p))');
    expect(source).toContain('class:agent-attention={attentionPaneIds.includes(m.paneId)}');
    expect(source).toContain('data-agent-attention={attentionPaneIds.includes(m.paneId) ? \'true\' : \'\'}');
  });

  it('fences refreshes by full Remote scope and cancels on teardown', () => {
    expect(source).toContain('createTeamRosterScopeGuard');
    expect(source).toContain('teamRosterScopeKey(remoteSessionId(ws), workspaceId, panes)');
    expect(source).toContain('const run = scopeGuard.begin();');
    expect(source).toContain('scopeGuard.invalidate();');
    expect(source).toContain('onAttentionChange?.([])');
    expect(source).toContain('ws.onReconnect');
    expect(source).toContain('fetchRemoteTeamRoster(ws, queryClient, sessionId, rosterWorkspaceId, signal)');
    expect(source).toContain('!signal.aborted && scopeGuard.isCurrent(generation)');
  });

  it('offers host-structured Agent resume and preserves recorded CWD', () => {
    expect(source).toContain('ws.resumeAgentSession(');
    expect(source).toContain('spec.cwd || reply.cwd');
    expect(source).toContain('aria-label={`Resume ${reply.agent} session ${reply.sessionId}`}');
  });

  it('deletes a group only after confirmation and persists the filtered roster', () => {
    expect(source).toContain('globalThis.confirm(`Delete Agent group "${group.name}"?`)');
    expect(source).toContain('persistGroups(groups.filter((g) => g.id !== group.id))');
    expect(source).toContain('title="Delete group"');
  });

  it('keeps group mutations ordered and exposes leader/order/color controls', () => {
    expect(source).toContain('pendingGroups: { workspaceId: string; groups: TeammateGroup[] } | null');
    expect(source).toContain('while (pendingGroups)');
    expect(source).toContain('reorderAgentGroups(groups, group.id, direction)');
    expect(source).toContain('toggleAgentGroupLeader(group, agentId)');
    expect(source).toContain('Move group up');
    expect(source).toContain('Move group down');
    expect(source).toContain('type="color"');
    expect(source).toContain('class:leader-active={g.leaderAgentId === aid}');
    expect(source).toContain('.member-actions{position:absolute');
  });

  it('sorts group cards by session identity before pane/id fallbacks', () => {
    expect(source).toContain('function sortedMemberIds(group: TeammateGroup): string[]');
    expect(source).toContain('sessionSortKey(memberOf(left), left)');
    expect(source).toContain('{#each sortedMemberIds(g) as aid (aid)}');
    expect(source).toContain('function sortedGroupCandidates(group: TeammateGroup)');
  });
});
