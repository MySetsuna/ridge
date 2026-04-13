// src/lib/stores/paneTree.ts
import { invoke, isTauri } from '@tauri-apps/api/core';
import { get, writable } from 'svelte/store';
import type { PaneNode } from '$lib/types';

export const paneTreeStore = writable<PaneNode>({
  type: 'leaf',
  id: 'root'
});

/** 最近一次点击/聚焦的终端窗格；分屏针对此 id。 */
export const activePaneId = writable<string>('root');

export const activeWorkspaceId = writable<string>('');

export const workspacesList = writable<{ id: string; index: number }[]>([]);

export function getAllPaneIds(node: PaneNode): string[] {
  const ids: string[] = [];
  function traverse(n: PaneNode) {
    if (n.type === 'leaf') {
      ids.push(n.id);
    } else {
      n.children?.forEach(traverse);
    }
  }
  traverse(node);
  return ids;
}

export async function syncPaneLayoutFromBackend() {
  if (!isTauri()) return;
  const layout = await invoke<PaneNode>('get_pane_layout');
  paneTreeStore.set(layout);
}

export async function refreshWorkspaces() {
  if (!isTauri()) return;
  const list = await invoke<{ id: string; index: number }[]>('list_workspaces');
  workspacesList.set(list);
  const active = await invoke<string>('get_active_workspace_id');
  activeWorkspaceId.set(active);
}

export async function createWorkspace() {
  if (!isTauri()) return;
  await invoke<string>('create_workspace');
  await refreshWorkspaces();
  await syncPaneLayoutFromBackend();
  activePaneId.set('root');
}

export async function switchWorkspace(workspaceId: string) {
  if (!isTauri()) return;
  await invoke('switch_workspace', { workspaceId });
  activeWorkspaceId.set(workspaceId);
  await syncPaneLayoutFromBackend();
  activePaneId.set('root');
}

export async function splitPane(paneId: string, direction: 'horizontal' | 'vertical') {
  if (!isTauri()) return '';
  const newId = await invoke<string>('split_pane', {
    paneId,
    direction
  });
  await syncPaneLayoutFromBackend();
  return newId;
}

/** 对当前焦点窗格分屏（若无有效 id 则回退 root）。 */
export async function splitActivePane(direction: 'horizontal' | 'vertical') {
  let id = get(activePaneId);
  const tree = get(paneTreeStore);
  const valid = getAllPaneIds(tree);
  if (!valid.includes(id)) id = 'root';
  return splitPane(id, direction);
}

export async function closePane(paneId: string) {
  if (!isTauri()) return;
  await invoke('close_pane', { paneId });
  await syncPaneLayoutFromBackend();
}

export async function toggleEditor(paneId: string, filePath?: string) {
  if (!isTauri()) return;
  await invoke('toggle_mode', {
    paneId,
    mode: {
      Editor: { file_path: filePath || null, language: 'rust' }
    }
  });
}
