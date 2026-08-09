import { describe, expect, it } from 'vitest';
import {
  collectPaneIds,
  filterWorkspaceResult,
  pathWithinRoots,
  planWorkspaceInvoke,
  type WorkspaceAccess,
} from './workspaceScope';
import { DESKTOP_PRIVILEGED_METHODS } from '../transport/protocolAdmission';

const access: WorkspaceAccess = {
  grantId: 'grant',
  granteeUserId: 'guest',
  ownerUserId: 'owner',
  deviceName: 'host',
  workspaceId: 'ws-shared',
  role: 'operator',
  delegable: false,
  paneIds: new Set(['pane-a']),
  roots: ['C:\\repo\\shared'],
};

describe('workspace share scope', () => {
  it('normalizes paths and rejects traversal/outside roots', () => {
    expect(pathWithinRoots('C:\\repo\\shared\\src\\a.ts', access.roots)).toBe(true);
    expect(pathWithinRoots('C:\\repo\\shared\\..\\secret.txt', access.roots)).toBe(false);
    expect(pathWithinRoots('C:\\repo\\shared-other\\a.ts', access.roots)).toBe(false);
    expect(pathWithinRoots('relative.txt', access.roots)).toBe(false);
  });

  it('collects leaf panes without treating split ids as panes', () => {
    expect(
      collectPaneIds({
        type: 'split',
        id: 'split-root',
        children: [{ type: 'leaf', id: 'pane-a' }, { paneId: 'pane-b' }],
      }),
    ).toEqual(new Set(['pane-a', 'pane-b']));
  });

  it('pins workspace and pane resources and denies second hop', () => {
    expect(planWorkspaceInvoke('get_pane_layout', {}, access)).toEqual({
      kind: 'invoke',
      method: 'get_pane_layout_for',
      params: { workspaceId: 'ws-shared' },
    });
    expect(planWorkspaceInvoke('write_to_pty', { paneId: 'pane-b', data: 'x' }, access).kind).toBe(
      'deny',
    );
    expect(planWorkspaceInvoke('read_file', { path: 'C:\\repo\\shared\\a.ts' }, access).kind).toBe(
      'invoke',
    );
    expect(planWorkspaceInvoke('connect_host', {}, access).kind).toBe('deny');
    expect(planWorkspaceInvoke('register_frontend_host', {}, access).kind).toBe('deny');
    expect(planWorkspaceInvoke('get_foreign_history_tail', {}, access).kind).toBe('deny');
    expect(planWorkspaceInvoke('create_workspace', {}, access).kind).toBe('deny');
    for (const method of DESKTOP_PRIVILEGED_METHODS) {
      expect(planWorkspaceInvoke(method, {}, access).kind, method).toBe('deny');
    }
  });

  it('adds the granted workspace to pane RPCs before host dispatch', () => {
    expect(planWorkspaceInvoke('write_to_pty', { paneId: 'pane-a', data: 'x' }, access)).toEqual({
      kind: 'invoke',
      method: 'write_to_pty',
      params: { paneId: 'pane-a', data: 'x', workspaceId: 'ws-shared' },
    });
    expect(planWorkspaceInvoke('resize_pane', { paneId: 'pane-a', rows: 24, cols: 80 }, access)).toEqual({
      kind: 'invoke',
      method: 'resize_pane',
      params: { paneId: 'pane-a', rows: 24, cols: 80, workspaceId: 'ws-shared' },
    });
    expect(planWorkspaceInvoke('write_to_pty', {
      paneId: 'pane-a',
      workspaceId: 'ws-other',
      data: 'x',
    }, access).kind).toBe('deny');
  });

  it('projects host workspace enumeration to the single grant', () => {
    expect(
      filterWorkspaceResult(
        'list_workspaces',
        [{ id: 'ws-other' }, { id: 'ws-shared', name: 'Shared' }],
        access.workspaceId,
      ),
    ).toEqual([{ id: 'ws-shared', name: 'Shared' }]);
  });

  it('pins Agent history and group writes to the granted workspace', () => {
    expect(planWorkspaceInvoke('read_agent_recent_replies', {}, access)).toEqual({
      kind: 'invoke',
      method: 'read_agent_recent_replies',
      params: { workspaceId: 'ws-shared' },
    });
    expect(planWorkspaceInvoke('set_teammate_groups', {}, access)).toEqual({
      kind: 'invoke',
      method: 'set_teammate_groups',
      params: { workspaceId: 'ws-shared' },
    });
    expect(planWorkspaceInvoke('resume_agent_session', { cwd: 'C:\\repo\\shared' }, access)).toEqual({
      kind: 'invoke',
      method: 'resume_agent_session',
      params: { cwd: 'C:\\repo\\shared', workspaceId: 'ws-shared' },
    });
  });
});
