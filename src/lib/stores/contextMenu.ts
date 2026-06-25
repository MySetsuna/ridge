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
  | 'scm-files'
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
  /** Element that had focus before the menu opened; restored on hide. */
  previousActiveElement?: Element | null;
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
        workspaceId,
        previousActiveElement: typeof document !== 'undefined' ? document.activeElement : null,
      });
    },
    hide: () => {
      update(state => {
        // Restore focus to whichever element had it before the menu opened.
        // This prevents keyboard input from being silently lost after a
        // right-click → menu action sequence: the IME helper textarea (or
        // any other active input) gets focus back so the next keystroke
        // reaches the correct handler.
        const prev = state.previousActiveElement as HTMLElement | null;
        if (prev && prev.isConnected && typeof prev.focus === 'function') {
          // Use setTimeout to defer focus restoration past Svelte's DOM
          // removal of the menu element — without this, the browser may
          // immediately re-steal focus to <body> after our .focus() call.
          setTimeout(() => {
            // Skip restoration if a menu action already moved focus elsewhere
            // (e.g. an inline create/rename <input> opened by the clicked item).
            // Only restore when focus fell back to <body>/null — otherwise we'd
            // steal focus from that fresh input and its blur handler would cancel
            // the edit, producing the "new-file input flashes then vanishes" bug.
            const ae = document.activeElement;
            if (ae !== null && ae !== document.body) return;
            try { prev.focus(); } catch { /* element may have disconnected */ }
          }, 0);
        }
        return { ...state, visible: false, previousActiveElement: undefined };
      });
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