import { invoke, isTauri } from '@tauri-apps/api/core';
import { get } from 'svelte/store';
import { activeWorkspaceId } from '$lib/stores/paneTree';
import { TerminalManager } from '$lib/terminal/manager';

export interface ShellInfo {
  id: string;
  label: string;
  program: string;
  args: string[];
}

let cache: ShellInfo[] | null = null;
let inflight: Promise<ShellInfo[]> | null = null;

/** 检测已装 shell，进程级缓存：只调一次后端，N 个 pane 的 header 与右键菜单共享。 */
export async function getShells(): Promise<ShellInfo[]> {
  if (cache) return cache;
  if (!isTauri()) return [];
  if (!inflight) {
    inflight = invoke<ShellInfo[]>('detect_available_shells')
      .then((s) => {
        cache = s;
        return s;
      })
      .catch((e) => {
        console.warn('detect_available_shells failed', e);
        return [];
      })
      .finally(() => {
        inflight = null;
      });
  }
  return inflight;
}

/** 原地切换某 pane 的 shell（拆 PTY → 带 args 重建）。 */
export async function changePaneShell(paneId: string, shell: ShellInfo): Promise<void> {
  if (!isTauri()) return;
  const wsId = get(activeWorkspaceId);
  if (!wsId) return;
  const manager = TerminalManager.instance();
  manager.clearScrollback(paneId);
  await invoke('change_pane_shell', { paneId, shell: shell.program, args: shell.args ?? [] });
  await invoke('activate_pane_pty', {
    workspaceId: wsId,
    paneId,
    rows: manager.rows(paneId),
    cols: manager.cols(paneId),
  });
  manager.forceFullRedraw(paneId);
}
