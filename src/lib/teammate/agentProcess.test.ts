import { beforeEach, describe, expect, it } from 'vitest';
import { detectAgentName, nextAgentSync, resetAgentSyncForTests } from './agentProcess';

describe('agent process detection', () => {
  beforeEach(resetAgentSyncForTests);

  it('recognizes title and process signals', () => {
    expect(detectAgentName('PowerShell', 'Claude Code')).toBe('claude');
    expect(detectAgentName('codex.exe')).toBe('codex');
    expect(detectAgentName('cargo')).toBeNull();
  });

  it('registers, periodically reconciles, then releases', () => {
    expect(nextAgentSync('ws:pane', 'claude', 1)).toEqual({
      kind: 'register',
      agentId: 'claude',
    });
    expect(nextAgentSync('ws:pane', 'claude', 2)).toEqual({ kind: 'none' });
    expect(nextAgentSync('ws:pane', 'claude', 6001)).toEqual({
      kind: 'register',
      agentId: 'claude',
    });
    expect(nextAgentSync('ws:pane', null, 6002)).toEqual({ kind: 'release' });
  });
});
