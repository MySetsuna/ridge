import { describe, expect, it } from 'vitest';
import { MobileRemoteUiState } from './mobileRemoteUiState.svelte';

describe('MobileRemoteUiState workspace reconciliation', () => {
  it('preserves a confirmed pane when workspace metadata arrives after the pane snapshot', () => {
    const ui = new MobileRemoteUiState(false);
    ui.navigate('ws', 'second');
    ui.reconcileWorkspace({ id: 'ws' }, [{ id: 'first' }, { id: 'second' }]);
    expect(ui.activePaneRef()).toEqual({ workspaceId: 'ws', paneId: 'second' });
    ui.reconcileWorkspace({ id: 'ws' });
    expect(ui.activePaneRef()).toEqual({ workspaceId: 'ws', paneId: 'second' });
  });

  it('distinguishes an authoritative empty list from missing membership', () => {
    const ui = new MobileRemoteUiState(false);
    ui.navigate('ws', 'second');
    ui.reconcileWorkspace({ id: 'ws' }, []);
    expect(ui.activePaneRef()).toBeNull();
    expect(ui.activeWorkspaceId).toBe('ws');
    ui.navigate('ws', 'second');
    ui.reconcileWorkspace({ id: 'ws', panes: [] }, [{ id: 'second' }]);
    expect(ui.activePaneRef()).toBeNull();
  });

  it('selects an existing pane when the old selection was closed', () => {
    const ui = new MobileRemoteUiState(false);
    ui.navigate('ws', 'closed');
    ui.reconcileWorkspace({ id: 'ws' }, [{ id: 'first' }, { id: 'second' }]);
    expect(ui.activePaneRef()).toEqual({ workspaceId: 'ws', paneId: 'first' });
  });

  it('does not carry a pane into another workspace with unknown membership', () => {
    const ui = new MobileRemoteUiState(false);
    ui.navigate('old', 'pane');
    ui.reconcileWorkspace({ id: 'new' });
    expect(ui.activeWorkspaceId).toBe('new');
    expect(ui.activePaneRef()).toBeNull();
  });
});
