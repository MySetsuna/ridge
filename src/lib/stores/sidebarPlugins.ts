// src/lib/stores/sidebarPlugins.ts
//
// Plugin registry for sidebar items. Plugins declare a `scope`:
//   - 'global'     — render once, regardless of workspace / pane
//   - 'workspace'  — render once per mounted workspace group
//   - 'pane'       — render once per pane (cwd column)
//
// The renderer (`SidebarPluginRegion.svelte`) walks the registry and emits
// one instance per scope unit. Plugins are Svelte components that take
// `{ workspaceId?, paneId?, cwd? }` props — all optional depending on scope.
// Keeping the API minimal lets extensions stay UI-facing without having to
// interact with Tauri plumbing up-front.

import type { Component } from 'svelte';
import { writable } from 'svelte/store';

export type SidebarPluginScope = 'global' | 'workspace' | 'pane';

export interface SidebarPluginProps {
  workspaceId?: string;
  paneId?: string;
  cwd?: string;
}

export interface SidebarPlugin {
  id: string;
  title: string;
  scope: SidebarPluginScope;
  /**
   * Svelte 5 component. Accepts SidebarPluginProps (partial depending on
   * scope). Kept as a loose `Component<any>` so plugin authors can pick
   * whatever subset of props they need.
   */
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  component: Component<any>;
  /** Sort key — lower renders earlier. Defaults to 100. */
  order?: number;
}

const _plugins = writable<SidebarPlugin[]>([]);
export const sidebarPluginStore = { subscribe: _plugins.subscribe };

export function registerSidebarPlugin(plugin: SidebarPlugin): void {
  _plugins.update((list) => {
    if (list.some((p) => p.id === plugin.id)) return list; // idempotent
    const next = [...list, plugin];
    next.sort((a, b) => (a.order ?? 100) - (b.order ?? 100));
    return next;
  });
}

export function unregisterSidebarPlugin(id: string): void {
  _plugins.update((list) => list.filter((p) => p.id !== id));
}
