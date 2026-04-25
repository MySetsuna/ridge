import { writable, get } from 'svelte/store';
import type { Component, ComponentType, SvelteComponent } from 'svelte';
import { splitResizeUiState } from './paneTree';

/**
 * Accept both Svelte 5 `Component` and legacy `ComponentType<SvelteComponent>`
 * so callers can pass icon components from `lucide-svelte@1.x` (which still
 * uses the class-component typing) without casting at every call site.
 * `unknown` lets the template simply render `<item.icon {size} {...}/>` —
 * Svelte's tagged-component syntax accepts both shapes at runtime.
 */
export type IconComponent =
  | Component<any, any, any>
  | ComponentType<SvelteComponent<any>>;

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
  icon?: IconComponent;
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