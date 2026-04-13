// src/lib/stores/paneTree.ts
import { invoke, isTauri } from '@tauri-apps/api/core';
import { get, writable } from 'svelte/store';
import type { PaneNode } from '$lib/types';

/** 占位；首屏 hydrate 前不挂载终端。根 pane 的 id 由后端按工作区生成唯一 UUID。 */
export const paneTreeStore = writable<PaneNode>({
  type: 'leaf',
  id: ''
});

/** 最近一次点击/聚焦的终端窗格；分屏针对此 id（与 layout 中 leaf id 一致）。 */
export const activePaneId = writable<string>('');

export const activeWorkspaceId = writable<string>('');

export const workspacesList = writable<{ id: string; index: number }[]>([]);

export function getAllPaneIds(node: PaneNode): string[] {
  const ids: string[] = [];
  function traverse(n: PaneNode) {
    if (n.type === 'leaf') {
      if (n.id) ids.push(n.id);
    } else {
      n.children?.forEach(traverse);
    }
  }
  traverse(node);
  return ids;
}

/** 当前 activePaneId 若不在树内（切换工作区等），回退到第一个 leaf。 */
function reconcileActivePaneId(layout: PaneNode) {
  const ids = getAllPaneIds(layout);
  if (!ids.length) return;
  const cur = get(activePaneId);
  if (!cur || !ids.includes(cur)) activePaneId.set(ids[0]);
}

export async function syncPaneLayoutFromBackend() {
  if (!isTauri()) return;
  const layout = await invoke<PaneNode>('get_pane_layout');
  paneTreeStore.set(layout);
  reconcileActivePaneId(layout);
}

/** 列表 + 活动区 id + 分屏树一次拉齐，再连续 set，避免 {#key activeWorkspaceId} 已变而 paneTree 仍是上一工作区的竞态。 */
export async function refreshWorkspaces() {
  if (!isTauri()) return;
  const list = await invoke<{ id: string; index: number }[]>('list_workspaces');
  const active = await invoke<string>('get_active_workspace_id');
  const layout = await invoke<PaneNode>('get_pane_layout');
  workspacesList.set(list);
  paneTreeStore.set(layout);
  activeWorkspaceId.set(active);
  reconcileActivePaneId(layout);
}

export async function createWorkspace() {
  if (!isTauri()) return;
  await invoke<string>('create_workspace');
  await refreshWorkspaces();
}

export async function switchWorkspace(workspaceId: string) {
  if (!isTauri()) return;
  await invoke('switch_workspace', { workspaceId });
  const layout = await invoke<PaneNode>('get_pane_layout');
  paneTreeStore.set(layout);
  activeWorkspaceId.set(workspaceId);
  reconcileActivePaneId(layout);
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

/** 对当前焦点窗格分屏（若无有效 id 则回退第一个 leaf）。 */
export async function splitActivePane(direction: 'horizontal' | 'vertical') {
  let id = get(activePaneId);
  const tree = get(paneTreeStore);
  const valid = getAllPaneIds(tree);
  if (!valid.length) return '';
  if (!valid.includes(id)) id = valid[0];
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
