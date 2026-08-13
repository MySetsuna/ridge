import { describe, expect, it } from 'vitest';
import {
  AGENT_HISTORY_REFRESH_INTERVAL_MS,
  agentAttentionForTransition,
  agentAttentionPriority,
  agentCardStatus,
  agentPaneStatus,
  agentStatusLabel,
  aggregateAgentCardStatus,
  buildAgentHistoryGroups,
  latestReplyForProfile,
  normalizeAgentIdentity,
  reorderAgentGroups,
  shouldRefreshAgentHistory,
  toggleAgentGroupLeader,
} from './agentCommuneModel';

describe('agent commune view model', () => {
  it('normalizes identity without using cwd', () => {
    expect(normalizeAgentIdentity('  Claude ')).toBe('claude');
    expect(normalizeAgentIdentity('')).toBe('unknown');
  });

  it('groups sessions from different cwds into one agent card', () => {
    const groups = buildAgentHistoryGroups([
      { agent: 'Claude', sessionId: 'old', timestamp: 1, cwd: 'C:/one' },
      { agent: ' claude ', sessionId: 'new', timestamp: 3, cwd: 'D:/two' },
      { agent: 'Grok', sessionId: 'g', timestamp: 2, cwd: 'C:/one' },
    ]);
    expect(groups.map((group) => group.key)).toEqual(['claude', 'grok']);
    expect(groups[0].replies.map((reply) => reply.sessionId)).toEqual(['new', 'old']);
  });

  it('binds a member reply by exact native session and selects the latest assistant item', () => {
    const replies = [
      { agent: 'Codex', sessionId: 'session-a', timestamp: 1, text: 'old' },
      { agent: 'codex', sessionId: 'session-a', timestamp: 3, text: 'actual latest' },
      { agent: 'Codex', sessionId: 'session-b', timestamp: 9, text: 'wrong session' },
    ];
    expect(latestReplyForProfile(replies, { id: 'kernel:pane', name: 'Codex', sessionId: 'session-a' })?.text)
      .toBe('actual latest');
    expect(latestReplyForProfile(replies, { id: 'Codex', name: 'Codex' })).toBeUndefined();
  });

  it('falls back from Ridge synthetic session ids to exact agent type and cwd', () => {
    const replies = [
      { agent: 'Codex', sessionId: 'native-a', timestamp: 2, cwd: 'C:/repo', text: 'right' },
      { agent: 'Codex', sessionId: 'native-b', timestamp: 8, cwd: 'C:/other', text: 'wrong cwd' },
    ];
    expect(latestReplyForProfile(replies, {
      id: 'kernel:pane', name: 'Pane title', executable: 'codex.exe',
      sessionId: 'session:ridge-generated', cwd: 'C:\\repo',
    })?.text).toBe('right');
  });

  it('treats a process cwd trailing separator as the same native session cwd', () => {
    const replies = [
      { agent: 'Codex', sessionId: 'native-a', timestamp: 2, cwd: 'C:/code/wind', text: 'right' },
    ];
    expect(latestReplyForProfile(replies, {
      id: 'auto:codex:pane', name: 'codex', sessionId: 'session:ridge-generated', cwd: 'C:\\code\\wind\\',
    })?.text).toBe('right');
  });

  it('prioritizes approval and active states over history completion', () => {
    expect(agentCardStatus(undefined, false)).toBe('completed');
    expect(agentCardStatus({ status: 'Idle', activity: 'idle' }, false)).toBe('idle');
    expect(agentCardStatus({ status: 'Working', activity: 'working' }, false)).toBe('working');
    expect(agentCardStatus({ status: 'Working', activity: 'idle' }, false)).toBe('idle');
    expect(agentCardStatus({ status: 'Working' }, false)).toBe('working');
    expect(agentCardStatus({ status: 'Idle', activity: 'idle' }, true)).toBe('waiting');
    // Remote roster DTOs carry the same string status shape; stopped states
    // must not drift into a yellow/silent idle rail on mobile.
    expect(agentCardStatus({ status: 'Suspended', activity: 'idle' }, false)).toBe('stopped');
    expect(agentCardStatus({ status: 'Disappeared', activity: 'idle' }, false)).toBe('stopped');
    expect(agentStatusLabel('stopped')).toBe('Stopped');
    expect(agentPaneStatus({ status: 'Suspended', activity: 'idle' }, false)).toBe('stopped');
    expect(agentPaneStatus({ status: 'Idle', activity: 'idle' }, false)).toBe('idle');
    expect(aggregateAgentCardStatus(['completed', 'working', 'waiting'])).toBe('waiting');
  });

  it('emits idle attention only after working-to-idle and keeps approval priority', () => {
    expect(agentAttentionForTransition(undefined, 'idle', false, 'Idle')).toBeNull();
    expect(agentAttentionForTransition('working', 'idle', false, 'Idle')).toBe('idle');
    expect(agentAttentionForTransition('working', 'waiting', true, 'Working')).toBe('waiting');
    expect(agentAttentionForTransition('working', 'stopped', false, 'Disappeared')).toBe('stopped');
    expect(agentAttentionPriority('waiting')).toBeGreaterThan(agentAttentionPriority('idle'));
  });

  it('refreshes host-wide history on a five-minute cadence, not every roster poll', () => {
    expect(shouldRefreshAgentHistory(0, 100)).toBe(true);
    expect(shouldRefreshAgentHistory(100, 100 + AGENT_HISTORY_REFRESH_INTERVAL_MS - 1)).toBe(false);
    expect(shouldRefreshAgentHistory(100, 100 + AGENT_HISTORY_REFRESH_INTERVAL_MS)).toBe(true);
  });

  it('reorders groups immutably and keeps boundary taps no-op', () => {
    const groups = [{ id: 'a' }, { id: 'b' }, { id: 'c' }];
    expect(reorderAgentGroups(groups, 'b', -1).map((g) => g.id)).toEqual(['b', 'a', 'c']);
    expect(reorderAgentGroups(groups, 'b', 1).map((g) => g.id)).toEqual(['a', 'c', 'b']);
    expect(reorderAgentGroups(groups, 'a', -1).map((g) => g.id)).toEqual(['a', 'b', 'c']);
    expect(groups.map((g) => g.id)).toEqual(['a', 'b', 'c']);
  });

  it('toggles only a real group member as leader', () => {
    const group = { id: 'g', memberAgentIds: ['a', 'b'], leaderAgentId: 'a' };
    expect(toggleAgentGroupLeader(group, 'a').leaderAgentId).toBeUndefined();
    expect(toggleAgentGroupLeader(group, 'b').leaderAgentId).toBe('b');
    expect(toggleAgentGroupLeader(group, 'ghost')).toBe(group);
  });
});
