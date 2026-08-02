import { describe, expect, it } from 'vitest';
import { normalizeTeamRosterWorkspaceId } from './teamRosterScope';

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
});
