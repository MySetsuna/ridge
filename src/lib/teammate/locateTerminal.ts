import { tick } from 'svelte';
import { get } from 'svelte/store';
import { focusTerminalPane } from '@ridge/remote/shared/terminal/terminalFocus';
import { activeWorkspaceId, clearAgentPaneAttention, focusPane, switchWorkspace } from '$lib/stores/paneTree';

/** Restore the original input target only while this workspace selection still owns focus. */
export async function locateTerminal(workspaceId: string, paneId: string): Promise<void> {
  if (!await switchWorkspace(workspaceId)) return;
  if (get(activeWorkspaceId) !== workspaceId) return;
  focusPane(paneId, workspaceId);
  await tick();
  if (get(activeWorkspaceId) !== workspaceId) return;
  if (focusTerminalPane(paneId)) clearAgentPaneAttention(workspaceId, paneId);
}
