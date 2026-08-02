import { describe, expect, it } from 'vitest';
import {
  agentCardStatus,
  agentPaneStatus,
  aggregateAgentCardStatus,
  buildAgentHistoryGroups,
  normalizeAgentIdentity,
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

  it('prioritizes approval and active states over history completion', () => {
    expect(agentCardStatus(undefined, false)).toBe('completed');
    expect(agentCardStatus({ status: 'Idle', activity: 'idle' }, false)).toBe('idle');
    expect(agentCardStatus({ status: 'Working', activity: 'working' }, false)).toBe('working');
    expect(agentCardStatus({ status: 'Idle', activity: 'idle' }, true)).toBe('waiting');
    expect(agentPaneStatus({ status: 'Suspended', activity: 'idle' }, false)).toBe('stopped');
    expect(agentPaneStatus({ status: 'Idle', activity: 'idle' }, false)).toBe('idle');
    expect(aggregateAgentCardStatus(['completed', 'working', 'waiting'])).toBe('waiting');
  });
});
