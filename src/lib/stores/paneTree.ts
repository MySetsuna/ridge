// src/lib/stores/paneTree.ts
import { invoke, isTauri } from '@tauri-apps/api/core';
import { get, writable } from 'svelte/store';
import type { PaneNode } from '$lib/types';
import { reportDevIssue } from '$lib/devIssue';

/** 占位；首屏 hydrate 前不挂载终端。根 pane 的 id 由后端按工作区生成唯一 UUID。 */
export const paneTreeStore = writable<PaneNode>({
  type: 'leaf',
  id: ''
});

/** 最近一次点击/聚焦的终端窗格；分屏针对此 id（与 layout 中 leaf id 一致）。 */
export const activePaneId = writable<string>('');

export const activeWorkspaceId = writable<string>('');

export const workspacesList = writable<{ id: string; index: number; name?: string }[]>([]);

// 工作区名称映射（用于UI显示）
export const workspaceNames = writable<Record<string, string>>({});

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
  try {
    const layout = await invoke<PaneNode>('get_pane_layout');
    paneTreeStore.set(layout);
    reconcileActivePaneId(layout);
  } catch (e) {
    console.error('syncPaneLayoutFromBackend', e);
    reportDevIssue({
      title: 'Layout sync failed',
      message: String(e),
      stack: e instanceof Error ? e.stack : undefined
    });
    throw e;
  }
}

/** 列表 + 活动区 id + 分屏树一次拉齐，再连续 set，避免 {#key activeWorkspaceId} 已变而 paneTree 仍是上一工作区的竞态。 */
export async function refreshWorkspaces() {
  if (!isTauri()) return;
  try {
    const list = await invoke<{ id: string; index: number }[]>('list_workspaces');
    const active = await invoke<string>('get_active_workspace_id');
    const layout = await invoke<PaneNode>('get_pane_layout');
    workspacesList.set(list);
    paneTreeStore.set(layout);
    activeWorkspaceId.set(active);
    reconcileActivePaneId(layout);
  } catch (e) {
    console.error('refreshWorkspaces', e);
    reportDevIssue({
      title: 'Workspace refresh failed',
      message: String(e),
      stack: e instanceof Error ? e.stack : undefined
    });
    throw e;
  }
}

export async function createWorkspace() {
  if (!isTauri()) return;
  try {
    await invoke<string>('create_workspace');
    await refreshWorkspaces();
  } catch (e) {
    console.error('createWorkspace', e);
    reportDevIssue({
      title: 'Workspace create failed',
      message: String(e),
      stack: e instanceof Error ? e.stack : undefined
    });
    throw e;
  }
}

export async function switchWorkspace(workspaceId: string) {
  if (!isTauri()) return;
  try {
    await invoke('switch_workspace', { workspaceId });
    const layout = await invoke<PaneNode>('get_pane_layout');
    paneTreeStore.set(layout);
    activeWorkspaceId.set(workspaceId);
    reconcileActivePaneId(layout);
  } catch (e) {
    console.error('switchWorkspace', workspaceId, e);
    reportDevIssue({
      title: 'Workspace switch failed',
      message: String(e),
      stack: e instanceof Error ? e.stack : undefined
    });
    throw e;
  }
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

/** 关闭工作区 */
export async function closeWorkspace(workspaceId: string) {
  if (!isTauri()) return;
  try {
    await invoke('close_workspace', { workspaceId });
    await refreshWorkspaces();
  } catch (e) {
    console.error('closeWorkspace', e);
    reportDevIssue({
      title: 'Workspace close failed',
      message: String(e),
      stack: e instanceof Error ? e.stack : undefined
    });
    throw e;
  }
}

/** 重新排序工作区 */
export async function reorderWorkspaces(fromIndex: number, toIndex: number) {
  if (!isTauri()) return;
  try {
    await invoke('reorder_workspaces', { fromIndex, toIndex });
    await refreshWorkspaces();
  } catch (e) {
    console.error('reorderWorkspaces', e);
    reportDevIssue({
      title: 'Workspace reorder failed',
      message: String(e),
      stack: e instanceof Error ? e.stack : undefined
    });
    throw e;
  }
}

/** 重命名工作区 */
export async function renameWorkspace(workspaceId: string, name: string) {
  if (!isTauri()) return;
  try {
    await invoke('rename_workspace', { workspaceId, name });
    // 更新本地名称映射
    workspaceNames.update(names => ({ ...names, [workspaceId]: name }));
    await refreshWorkspaces();
  } catch (e) {
    console.error('renameWorkspace', e);
    reportDevIssue({
      title: 'Workspace rename failed',
      message: String(e),
      stack: e instanceof Error ? e.stack : undefined
    });
    throw e;
  }
}

// ============ 历史工作区相关 ============

export interface WorkspaceHistoryItem {
  id: string;
  name: string;
  savedAt: string;
  paneCount: number;
  isPinned: boolean;
}

export const workspaceHistoryList = writable<WorkspaceHistoryItem[]>([]);

/** 获取历史工作区列表 */
export async function loadWorkspaceHistory() {
  if (!isTauri()) return;
  try {
    const history = await invoke<WorkspaceHistoryItem[]>('list_workspace_history');
    workspaceHistoryList.set(history);
  } catch (e) {
    console.error('loadWorkspaceHistory', e);
  }
}

/** 保存当前工作区到历史 */
export async function saveWorkspaceToHistory(name?: string) {
  if (!isTauri()) return;
  try {
    await invoke('save_workspace', { name: name || `工作区 ${Date.now()}` });
    await loadWorkspaceHistory();
  } catch (e) {
    console.error('saveWorkspaceToHistory', e);
    throw e;
  }
}

/** 从历史恢复工作区 */
export async function restoreWorkspaceFromHistory(historyId: string) {
  if (!isTauri()) return;
  try {
    await invoke('restore_workspace', { historyId });
    await refreshWorkspaces();
  } catch (e) {
    console.error('restoreWorkspaceFromHistory', e);
    throw e;
  }
}

/** 删除历史工作区 */
export async function deleteWorkspaceHistory(historyId: string) {
  if (!isTauri()) return;
  try {
    await invoke('delete_workspace_history', { historyId });
    await loadWorkspaceHistory();
  } catch (e) {
    console.error('deleteWorkspaceHistory', e);
    throw e;
  }
}

/** 固定/取消固定历史工作区 */
export async function togglePinWorkspaceHistory(historyId: string) {
  if (!isTauri()) return;
  try {
    await invoke('toggle_pin_workspace_history', { historyId });
    await loadWorkspaceHistory();
  } catch (e) {
    console.error('togglePinWorkspaceHistory', e);
    throw e;
  }
}

/** 重命名历史工作区 */
export async function renameWorkspaceHistory(historyId: string, name: string) {
  if (!isTauri()) return;
  try {
    await invoke('rename_workspace_history', { historyId, name });
    await loadWorkspaceHistory();
  } catch (e) {
    console.error('renameWorkspaceHistory', e);
    throw e;
  }
}
