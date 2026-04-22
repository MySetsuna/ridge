import { writable, get } from 'svelte/store';
import type { Component } from 'svelte';
import { splitResizeUiState } from './paneTree';

export type ContextMenuTarget =
  | 'terminal'
  | 'editor'
  | 'pane-header'
  | 'splitter'
  | 'sidebar'
  | 'workspace-tabs'
  | 'git-graph'
  | 'pane-content'
  | 'unknown';

export interface ContextMenuItem {
  id: string;
  label?: string;
  icon?: Component;
  shortcut?: string;
  action?: () => void;
  disabled?: boolean;
  divider?: boolean;
  children?: ContextMenuItem[];
}

export interface ContextMenuState {
  visible: boolean;
  x: number;
  y: number;
  items: ContextMenuItem[];
  target: ContextMenuTarget;
  paneId?: string;
  workspaceId?: string;
}

function createContextMenuStore() {
  const { subscribe, set, update } = writable<ContextMenuState>({
    visible: false,
    x: 0,
    y: 0,
    items: [],
    target: 'unknown'
  });

  return {
    subscribe,
    show: (
      x: number,
      y: number,
      items: ContextMenuItem[],
      target: ContextMenuTarget = 'unknown',
      paneId?: string,
      workspaceId?: string
    ) => {
      set({
        visible: true,
        x,
        y,
        items,
        target,
        paneId,
        workspaceId
      });
    },
    hide: () => {
      update(state => ({ ...state, visible: false }));
    },
    updatePosition: (x: number, y: number) => {
      update(state => ({ ...state, x, y }));
    }
  };
}

export const contextMenu = createContextMenuStore();

export const showContextMenu = contextMenu.show;
export const hideContextMenu = contextMenu.hide;

/** 检测右键时是否正在 resize 操作中 */
export function isResizeInProgress(): boolean {
  const state = get(splitResizeUiState);
  return state.phase === 'pending' || state.phase === 'junction' || state.phase === 'drag';
}