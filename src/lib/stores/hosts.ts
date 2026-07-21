// src/lib/stores/hosts.ts
//
// 「主机 / Hosts」侧边栏 tab 的状态 SSOT。承载所有「外部终端 provider」：
//   - headless：本机无头会话（复用后端 list/summon/new/terminate native 命令）
//   - remote / rdg：远端 ridge / rdg 主机（P3/P4 接入，此处先留类型与占位）
//
// 生命周期不变量（详见 docs/superpowers/specs/2026-06-30-...-hosts-design.md）：
//   工作区里关闭 foreign pane = detach（会话保活）；**真正终止**只能在此面板里做。
import { writable, get } from 'svelte/store';
import { invoke } from '@tauri-apps/api/core';
import {
  activeWorkspaceId,
  paneTreeStore,
  syncPaneLayoutFromBackend,
  dockPane,
} from '$lib/stores/paneTree';
import type { PaneNode } from '$lib/types';
import type { AttachRegion } from '$lib/stores/dockRegionPicker';

export type HostKind = 'headless' | 'remote' | 'rdg';
export type HostStatus = 'connected' | 'connecting' | 'disconnected' | 'error';

/** 后端 `list_native_sessions` 回传的 native 会话摘要（与 ridge_tmux::NativeSessionInfo 对齐）。 */
export interface NativeSessionInfo {
  socket: string;
  name: string;
  windows: number;
  panes: number;
  width: number;
  height: number;
  attached: boolean;
}

/** 一台主机下的一个会话（provider 真正持有的 PTY）。 */
export interface HostSession {
  /** provider 域内会话键：headless 用 (socket, name)。 */
  socket: string;
  name: string;
  windows: number;
  panes: number;
  width: number;
  height: number;
  /** 是否已被某工作区领养（attached=已接入）。 */
  attached: boolean;
}

export interface Host {
  id: string;
  kind: HostKind;
  label: string;
  status: HostStatus;
  /** 远端主机的状态说明（如「live 传输待接入」）；headless 无。 */
  detail?: string;
  sessions: HostSession[];
}

/** 后端 `host_list_snapshot` 回传的远端主机记录（crate::hosts::HostRecord，不含凭据）。 */
interface HostRecord {
  id: string;
  kind: 'remote' | 'rdg';
  label: string;
  addr: string;
  status: HostStatus;
  detail: string;
  sessions: { id: string; title: string; attached: boolean }[];
}

export const hostsStore = writable<Host[]>([]);
export const hostsLoading = writable(false);
/** 上次刷新错误（面板顶部提示用），空串=无错误。 */
export const hostsError = writable('');

const HEADLESS_HOST_ID = 'headless';

/**
 * 刷新主机/会话快照。当前聚合后端 native 会话为「本机（无头）」单一 host；
 * 远端/rdg host 在 P3/P4 由各自连接推送后合并进 hostsStore。
 */
export async function refreshHosts(): Promise<void> {
  hostsLoading.set(true);
  const next: Host[] = [];
  let err = '';
  // ① 本机（无头）：native 会话。
  try {
    const sessions = await invoke<NativeSessionInfo[]>('list_native_sessions');
    next.push({
      id: HEADLESS_HOST_ID,
      kind: 'headless',
      label: '本机（无头）',
      status: 'connected',
      sessions: sessions ?? [],
    });
  } catch (e) {
    err = e instanceof Error ? e.message : String(e);
  }
  // ② 远端 ridge / rdg 主机（桌面本地命令；web-remote 无此授权 → 忽略，仅显示 headless）。
  try {
    const recs = await invoke<HostRecord[]>('host_list_snapshot');
    for (const r of recs ?? []) {
      next.push({
        id: r.id,
        kind: r.kind,
        label: r.label,
        status: r.status,
        detail: r.detail,
        // 远端会话（live 传输里程接入前恒为空）适配到 HostSession 形状。
        sessions: (r.sessions ?? []).map((s) => ({
          socket: r.id,
          name: s.title || s.id,
          windows: 0,
          panes: 0,
          width: 0,
          height: 0,
          attached: s.attached,
        })),
      });
    }
  } catch {
    /* host_list_snapshot 不可用（如 web-remote 未授权）：仅忽略远端主机 */
  }
  hostsStore.set(next);
  hostsError.set(err);
  hostsLoading.set(false);
}

/** 新建一个本机无头会话（仅创建、不接入）；返回会话名。 */
export async function newHeadlessSession(name?: string, cwd?: string): Promise<string> {
  const created = await invoke<string>('new_headless_session', {
    name: name?.trim() || null,
    cwd: cwd?.trim() || null,
  });
  await refreshHosts();
  return created;
}

/**
 * **真正终止**一个会话（杀进程）。这是唯一的真关闭入口。
 * 若该会话当前被领养，后端经 reader-EOF 自动把工作区视图摘除。
 */
export async function terminateSession(socket: string, target: string): Promise<void> {
  await invoke('terminate_native_session', { socket, target });
  await refreshHosts();
}

/**
 * 接入：把一个会话召唤进当前查看的工作区。P1 直接 summon（后端决定落点，通常拆分活动
 * pane）；P2 在右键/拖拽场景下走 dock 区域选择的 attach_foreign_session 精确落点。
 */
export async function attachSession(socket: string, target: string): Promise<void> {
  const wid = get(activeWorkspaceId);
  await invoke('summon_native_session', { socket, target, workspaceId: wid ?? null });
  await refreshHosts();
}

/** 在 pane 树里按 origin 会话键 `socket:gid` 找到刚领养的 foreign pane。 */
function findPaneByOriginSession(node: PaneNode, sessionId: string): string | null {
  if (node.type === 'leaf') {
    return node.origin && node.origin.session_id === sessionId ? node.id : null;
  }
  for (const child of node.children) {
    const hit = findPaneByOriginSession(child, sessionId);
    if (hit) return hit;
  }
  return null;
}

/**
 * 区域精确接入：召唤会话后，把新领养的 pane 停靠到 `targetPaneId` 的指定方向。
 * 复用既有且经测试的 summon + dock_pane 两个原语（无需新后端命令）：
 *   1. summon 把会话领养进工作区（后端决定初始落点），返回其 native global_id；
 *   2. 重新同步布局后按 origin 会话键 `socket:gid` 定位新 pane；
 *   3. dock_pane 把它移动到目标方向半区。
 */
export async function attachSessionAt(
  socket: string,
  target: string,
  targetPaneId: string,
  region: AttachRegion
): Promise<void> {
  const wid = get(activeWorkspaceId);
  const gid = await invoke<number>('summon_native_session', {
    socket,
    target,
    workspaceId: wid ?? null,
  });
  await syncPaneLayoutFromBackend();
  const newPaneId = findPaneByOriginSession(get(paneTreeStore), `${socket}:${gid}`);
  if (newPaneId && newPaneId !== targetPaneId) {
    await dockPane(newPaneId, targetPaneId, region);
  }
  await refreshHosts();
}

/**
 * 登记一台远端主机（ridge LAN / rdg）。凭据仅传给后端 live 传输里程使用，不落库。
 * P3/P4 基础层：当前仅登记 + 展示，真正出站连接与 PTY 流为下一里程。
 */
export async function connectHost(
  kind: 'remote' | 'rdg',
  label: string,
  addr: string,
  token?: string
): Promise<void> {
  await invoke('connect_host', {
    kind,
    label: label.trim() || null,
    addr: addr.trim(),
    token: token?.trim() || null,
  });
  await refreshHosts();
}

/** 断开一台远端主机（保留登记）。 */
export async function disconnectHost(hostId: string): Promise<void> {
  await invoke('disconnect_host', { hostId });
  await refreshHosts();
}

/** 忘记一台远端主机（移除登记）。 */
export async function forgetHost(hostId: string): Promise<void> {
  await invoke('forget_host', { hostId });
  await refreshHosts();
}

