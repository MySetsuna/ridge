import { describe, expect, it } from 'vitest';
import {
  createTeamRosterScopeGuard,
  normalizeTeamRosterWorkspaceId,
  teamRosterScopeKey,
} from './teamRosterScope';

describe('team roster workspace scope', () => {
  it('preserves the active workspace id for topology requests', () => {
    expect(normalizeTeamRosterWorkspaceId('workspace-b')).toBe('workspace-b');
  });

  it('does not manufacture a scope before a workspace is selected', () => {
    expect(normalizeTeamRosterWorkspaceId('')).toBeUndefined();
  });

  it('does not rewrite an already-qualified workspace id', () => {
    expect(normalizeTeamRosterWorkspaceId(' workspace-b ')).toBe(' workspace-b ');
  });

  it('changes scope for workspace/CWD but ignores pane title churn and order', () => {
    const a = teamRosterScopeKey(1, 'workspace-a', [
      { id: 'pane-2', cwd: 'D:/two' },
      { id: 'pane-1', cwd: 'C:/one' },
    ]);
    const reordered = teamRosterScopeKey(1, 'workspace-a', [
      { id: 'pane-1', cwd: 'C:/one' },
      { id: 'pane-2', cwd: 'D:/two' },
    ]);
    expect(reordered).toBe(a);
    expect(teamRosterScopeKey(1, 'workspace-b', [
      { id: 'pane-2', cwd: 'D:/two' },
      { id: 'pane-1', cwd: 'C:/one' },
    ])).not.toBe(a);
    expect(teamRosterScopeKey(1, 'workspace-a', [
      { id: 'pane-2', cwd: 'D:/other' },
      { id: 'pane-1', cwd: 'C:/one' },
    ])).not.toBe(a);
  });

  it('aborts the previous scope and rejects stale generations', () => {
    const guard = createTeamRosterScopeGuard();
    const first = guard.begin();
    const second = guard.begin();
    expect(first.signal.aborted).toBe(true);
    expect(guard.isCurrent(first.generation)).toBe(false);
    expect(guard.isCurrent(second.generation)).toBe(true);
    guard.invalidate();
    expect(second.signal.aborted).toBe(true);
    expect(guard.isCurrent(second.generation)).toBe(false);
  });
});
