import { invoke, isTauri } from '@tauri-apps/api/core';
import { get, writable } from 'svelte/store';
import { activeWorkspaceId } from '$lib/stores/paneTree';
import { TerminalManager } from '$lib/terminal/manager';

export interface ShellInfo {
  id: string;
  label: string;
  program: string;
  args: string[];
}

/**
 * §I-2 一致性：每个 pane「当前选中的 ShellInfo.id」单一真相源（paneId → shellId）。
 *
 * 由两个切 shell 入口共用的 `changePaneShell` 在切换成功后统一写入，header
 * 切换器（`PaneShellSwitcher`）据此派生显示当前 shell。解决两处不一致：
 *  - 经 pane 右键菜单切换时也会更新它（旧实现只有 header 的本地 `selectedId`
 *    会更新，右键切后 header 标签回退到 `currentShell` 的 program 匹配，WSL
 *    多发行版会错显为首个）。
 *  - 跨 `PaneShellSwitcher` 重挂载保留乐观选择（旧本地 `$state` 会被重置）。
 * 优先级高于 layout 回传的 `currentShell`(program)——program 在 WSL 多发行版
 * 同 `wsl.exe` 时不足以区分，而 `ShellInfo.id`(如 `wsl-Ubuntu`)唯一。
 */
export const paneShellSelection = writable<Record<string, string>>({});

/** pane 关闭时清掉其选择项（避免长会话里 paneId 条目无限累积）。 */
export function clearPaneShellSelection(paneId: string): void {
  paneShellSelection.update((m) => {
    if (!(paneId in m)) return m;
    const next = { ...m };
    delete next[paneId];
    return next;
  });
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
  // §I-2: 记下本 pane 选中的 shell id（单一真相源）。header 与右键菜单都经此
  // 函数切换，故两入口切后 header 标签一致显示正确的 shell（含 WSL 发行版）。
  paneShellSelection.update((m) => ({ ...m, [paneId]: shell.id }));
}
