import { invoke } from '@tauri-apps/api/core';
import type { PaneRef } from '@ridge/remote';

export async function startPaneSession(options: {
  pane: PaneRef;
  shell: string | null;
  grid: () => { rows: number; cols: number };
  prepareStream: () => Promise<void>;
  alive: () => boolean;
}): Promise<void> {
  const { pane, shell, grid, prepareStream, alive } = options;
  if (!alive()) return;
  await invoke('create_pane', { ...pane, shell, ...grid() });
  if (!alive()) return;
  await prepareStream();
  if (!alive()) return;
  await invoke('activate_pane_pty', { ...pane, ...grid() });
}
