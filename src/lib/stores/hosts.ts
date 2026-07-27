// src/lib/stores/hosts.ts
//
// 「接入」侧边栏 tab 的状态 SSOT。承载所有外部终端与共享工作区 provider：
//   - headless：本机无头会话（复用后端 list/summon/new/terminate native 命令）
//   - remote / rdg：远端 ridge / rdg 主机（P3/P4 接入，此处先留类型与占位）
//
// 生命周期不变量：
//   工作区里关闭 foreign pane = detach（会话保活）；**真正终止**只能在此面板里做。
import { writable, get } from 'svelte/store';
import { invoke, isTauri } from '@tauri-apps/api/core';
import {
  activeWorkspaceId,
  paneTreeStore,
  syncPaneLayoutFromBackend,
  dockPane,
  setPaneCwd,
  closePane as closeLocalPane,
} from '$lib/stores/paneTree';
import type { PaneNode } from '$lib/types';
import type { AttachRegion } from '$lib/stores/dockRegionPicker';
import {
  applyPumpBatch,
  formatPumpBadge,
  initialPumpState,
  type PumpState,
} from '../../../packages/remote/src/shared/hosts/livePumpPolicy';
import {
  snapshotFromOutboundStats,
  hostsLineAlert,
} from '../../../packages/remote/src/shared/hosts/liveBackpressure';
import {
  reduceLifecycle,
  safeSubscribe,
  createSession,
  type OutboundSession,
} from '../../../packages/remote/src/shared/hosts/outboundLifecycle';
import {
  planAttachSeed,
  summarizeHistoryBadge,
  type HistoryTailSnapshot,
} from '$lib/hosts/foreignHistorySession';
import * as cloudAuth from '@ridge/remote/shared/cloud/auth';
import {
  BASE_DOMAIN,
  acceptWorkspaceShare,
  cloudHttpScheme,
  listWorkspaceShares,
  listSharedWithMe,
  revokeWorkspaceShare,
} from '@ridge/remote/shared/cloud/apiClient';
import { RemoteConnection } from '@ridge/remote';
import {
  loadHostForest,
  type HostForestResult,
  type HostTopologyLink,
} from '$lib/hosts/hostForest';
import { connectCloudHostTopologyLink } from '$lib/remote/cloud/cloudHostTopologyLink';
import {
  bindRemotePane,
  deleteRemotePane,
  unbindRemotePane,
} from '$lib/hosts/remotePaneBindings';

export type HostKind = 'headless' | 'remote' | 'rdg' | 'shared' | 'sharing';
export type HostStatus = 'connected' | 'connecting' | 'disconnected' | 'error';

/** 后端 `list_native_sessions` 回传的 native 会话摘要（与 ridge_tmux::NativeSessionInfo 对齐）。 */
export interface NativeSessionInfo {
  socket: string;
  name: string;
  creator_workspace_id?: string;
  creator_pane_id?: string;
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
  creator_workspace_id?: string;
  creator_pane_id?: string;
  /**
   * Remote/rdg: backend HostSessionMeta.id (pane id on host).
   * Headless: empty (uses socket+name).
   */
  remoteSessionId?: string;
  windows: number;
  panes: number;
  width: number;
  height: number;
  /** 是否已被某工作区领养（attached=已接入）。 */
  attached: boolean;
  /** 跨账号工作区分享元数据；普通终端会话均为空。 */
  workspaceId?: string;
  shareGrantId?: string;
  shareStatus?: 'pending' | 'active' | 'declined' | 'revoked';
  ownerUsername?: string;
  deviceName?: string;
  granteeLabel?: string;
  cwd?: string;
  isAgent?: boolean;
}

export interface HostWorkspace {
  id: string;
  name: string;
  active: boolean;
  sessions: HostSession[];
  shareGrantId?: string;
  shareStatus?: HostSession['shareStatus'];
  role?: 'operator';
}

export interface Host {
  id: string;
  kind: HostKind;
  label: string;
  status: HostStatus;
  /** 远端主机的状态说明（如「live 传输待接入」）；headless 无。 */
  detail?: string;
  sessions: HostSession[];
  workspaces: HostWorkspace[];
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

export interface RegisteredHostLink {
  hostId: string;
  kind: 'remote' | 'rdg';
  label: string;
  detail?: string;
  deviceName?: string;
  link: HostTopologyLink;
  manualDisconnected?: boolean;
}

const registeredHostLinks = new Map<string, RegisteredHostLink>();
const topologyByHost = new Map<string, HostForestResult>();
const topologyInFlight = new Map<string, Promise<HostForestResult>>();

export function registerHostTopologyLink(source: RegisteredHostLink): () => void {
  const previous = registeredHostLinks.get(source.hostId);
  if (previous && previous.link !== source.link) void previous.link.disconnect();
  registeredHostLinks.set(source.hostId, source);
  return () => {
    if (registeredHostLinks.get(source.hostId)?.link !== source.link) return;
    registeredHostLinks.delete(source.hostId);
    topologyByHost.delete(source.hostId);
  };
}

export function hasHostTopologyLink(hostId: string): boolean {
  return registeredHostLinks.has(hostId);
}

export function hostShareDeviceName(hostId: string): string | undefined {
  return registeredHostLinks.get(hostId)?.deviceName;
}

export async function refreshHostTopology(hostId: string): Promise<HostForestResult | null> {
  const source = registeredHostLinks.get(hostId);
  if (!source) return null;
  if (source.manualDisconnected || source.link.state() === 'disconnected') {
    return topologyByHost.get(hostId) ?? null;
  }
  const current = topologyInFlight.get(hostId);
  if (current) return current;
  const pending = loadHostForest([{ hostId, link: source.link }])
    .then(async ([result]) => {
      topologyByHost.set(hostId, result);
      if (isTauri() && !result.error) {
        await invoke('register_frontend_host', {
          hostId,
          kind: source.kind,
          label: source.label,
          sessions: result.workspaces.flatMap((workspace) =>
            workspace.panes.map((pane) => ({ id: pane.id, title: pane.title }))),
        });
      }
      return result;
    })
    .finally(() => topologyInFlight.delete(hostId));
  topologyInFlight.set(hostId, pending);
  return pending;
}

export async function closeHostPane(hostId: string, paneId: string): Promise<void> {
  const source = registeredHostLinks.get(hostId);
  if (!source) throw new Error('该主机未提供 pane 删除能力');
  const closed = await deleteRemotePane(hostId, paneId, source.link, closeLocalPane);
  if (!closed) throw new Error('远端 pane 删除失败');
  await refreshHosts();
}

function topologyLink(hostId: string): HostTopologyLink {
  const source = registeredHostLinks.get(hostId);
  if (!source || source.manualDisconnected) throw new Error('主机未连接');
  return source.link;
}

export async function createHostWorkspace(hostId: string, name?: string): Promise<void> {
  const id = await topologyLink(hostId).createWorkspace(name);
  if (!id) throw new Error('远端工作区创建失败');
  await refreshHosts();
}

export async function openHostWorkspace(hostId: string, workspaceId: string): Promise<void> {
  if (!await topologyLink(hostId).switchWorkspace(workspaceId)) {
    throw new Error('远端工作区打开失败');
  }
  await refreshHosts();
}

export async function renameHostWorkspace(
  hostId: string,
  workspaceId: string,
  name: string,
): Promise<void> {
  const link = topologyLink(hostId);
  if (!link.renameWorkspace || !await link.renameWorkspace(workspaceId, name)) {
    throw new Error('远端工作区重命名失败');
  }
  await refreshHosts();
}

export async function saveHostWorkspace(
  hostId: string,
  workspaceId: string,
  name: string,
): Promise<void> {
  const link = topologyLink(hostId);
  if (!link.saveWorkspace || !await link.saveWorkspace(workspaceId, name)) {
    throw new Error('远端工作区保存失败');
  }
}

export async function createHostPane(hostId: string, workspaceId: string): Promise<void> {
  const link = topologyLink(hostId);
  if (!await link.switchWorkspace(workspaceId)) throw new Error('远端工作区打开失败');
  if (!await link.createPane()) throw new Error('远端 pane 创建失败');
  await refreshHosts();
}

export async function closeHostWorkspace(hostId: string, workspaceId: string): Promise<void> {
  if (!await topologyLink(hostId).closeWorkspace(workspaceId)) {
    throw new Error('远端工作区关闭失败');
  }
  await refreshHosts();
}

export async function markHostPaneAgent(
  hostId: string,
  workspaceId: string,
  paneId: string,
  on: boolean,
): Promise<void> {
  const link = topologyLink(hostId);
  const mark = link.markPaneAgent;
  if (!mark) throw new Error('该主机不支持 Agent 标记');
  await mark.call(link, workspaceId, paneId, on);
  await refreshHosts();
}

export async function changeHostPaneShell(
  hostId: string,
  workspaceId: string,
  paneId: string,
  shellId: string,
): Promise<void> {
  const link = topologyLink(hostId);
  if (!link.listShells || !link.changePaneShell) throw new Error('该主机不支持切换 shell');
  const shells = await link.listShells();
  const shell = shells.find((candidate) => candidate.id === shellId);
  if (!shell) throw new Error('所选 shell 不可用');
  await link.changePaneShell(workspaceId, paneId, shell);
  await refreshHosts();
}

export async function hostShellChoices(hostId: string): Promise<Array<{ id: string; label: string }>> {
  const link = topologyLink(hostId);
  if (!link.listShells) return [];
  return (await link.listShells()).map(({ id, label }) => ({ id, label }));
}

function paneSession(
  hostId: string,
  workspaceId: string,
  pane: HostForestResult['workspaces'][number]['panes'][number],
): HostSession {
  return {
    socket: hostId,
    name: pane.title,
    remoteSessionId: pane.id,
    workspaceId,
    windows: 0,
    panes: 1,
    width: 0,
    height: 0,
    attached: false,
    cwd: pane.cwd,
    isAgent: pane.isAgent,
  };
}

function linkedHost(
  source: RegisteredHostLink,
  topology: HostForestResult,
  previous?: Host,
): Host {
  const attached = new Set(
    previous?.sessions
      .filter((session) => session.attached)
      .map((session) => session.remoteSessionId)
      .filter((id): id is string => !!id),
  );
  const workspaces = topology.workspaces.map((workspace) => ({
    id: workspace.id,
    name: workspace.name,
    active: workspace.active,
    sessions: workspace.panes.map((pane) => ({
      ...paneSession(source.hostId, workspace.id, pane),
      attached: attached.has(pane.id),
    })),
  }));
  return {
    id: source.hostId,
    kind: source.kind,
    label: source.label,
    status: source.manualDisconnected || source.link.state() === 'disconnected'
      ? 'disconnected'
      : topology.error
      ? 'error'
      : source.link.state() === 'connected' ? 'connected' : 'connecting',
    detail: topology.error || source.detail,
    workspaces,
    sessions: workspaces.flatMap((workspace) => workspace.sessions),
  };
}

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
      workspaces: [{
        id: HEADLESS_HOST_ID,
        name: '无头会话',
        active: true,
        sessions: sessions ?? [],
      }],
    });
  } catch (e) {
    err = e instanceof Error ? e.message : String(e);
  }
  // ② 远端 ridge / rdg 主机（桌面本地命令；web-remote 无此授权 → 忽略，仅显示 headless）。
  try {
    const recs = await invoke<HostRecord[]>('host_list_snapshot');
    for (const r of recs ?? []) {
      const sessions = (r.sessions ?? []).map((s) => ({
        socket: r.id,
        name: s.title || s.id,
        remoteSessionId: s.id,
        windows: 0,
        panes: 0,
        width: 0,
        height: 0,
        attached: s.attached,
      }));
      next.push({
        id: r.id,
        kind: r.kind,
        label: r.label,
        status: r.status,
        detail: r.detail,
        sessions,
        workspaces: sessions.length > 0 ? [{
          id: `${r.id}:legacy`,
          name: '远端会话',
          active: true,
          sessions,
        }] : [],
      });
    }
  } catch {
    /* host_list_snapshot 不可用（如 web-remote 未授权）：仅忽略远端主机 */
  }
  // ③ 跨账号分享：按 owner host 聚合，每个 grant 是其下单独工作区。
  const auth = cloudAuth.snapshot();
  if (auth.userToken) {
    try {
      const { shares } = await listSharedWithMe(auth.userToken);
      const grouped = new Map<string, Host>();
      for (const share of shares) {
        if (!['pending', 'active'].includes(share.status)) continue;
        const key = `shared:${share.ownerUserId}:${share.deviceName}`;
        let host = grouped.get(key);
        if (!host) {
          host = {
            id: key,
            kind: 'shared',
            label: `${share.ownerUsername || '共享用户'} · ${share.deviceName}`,
            status: share.status === 'active' ? 'connected' : 'connecting',
            detail: '跨账号单工作区；不可二次转发主机或 Remote',
            sessions: [],
            workspaces: [],
          };
          grouped.set(key, host);
        }
        const session: HostSession = {
          socket: key,
          name: `工作区 ${share.workspaceId.slice(0, 8)}`,
          remoteSessionId: share.workspaceId,
          workspaceId: share.workspaceId,
          shareGrantId: share.id,
          shareStatus: share.status,
          ownerUsername: share.ownerUsername,
          deviceName: share.deviceName,
          windows: 1,
          panes: 0,
          width: 0,
          height: 0,
          attached: false,
        };
        host.sessions.push(session);
        host.workspaces.push({
          id: share.workspaceId,
          name: session.name,
          active: share.status === 'active',
          sessions: [],
          shareGrantId: share.id,
          shareStatus: share.status,
          role: 'operator',
        });
      }
      next.push(...grouped.values());

      const outgoing = await listWorkspaceShares(auth.userToken);
      const visible = outgoing.shares.filter((share) =>
        ['pending', 'active'].includes(share.status),
      );
      if (visible.length > 0) {
        next.push({
          id: 'sharing:outgoing',
          kind: 'sharing',
          label: '本机已分享',
          status: 'connected',
          detail: '在此撤销邀请或已生效分享',
          sessions: visible.map((share) => ({
            socket: 'sharing:outgoing',
            name: `工作区 ${share.workspaceId.slice(0, 8)}`,
            workspaceId: share.workspaceId,
            shareGrantId: share.id,
            shareStatus: share.status,
            granteeLabel: share.granteeUsername || share.granteeEmail,
            windows: 1,
            panes: 0,
            width: 0,
            height: 0,
            attached: false,
          })),
          workspaces: visible.map((share) => ({
            id: share.workspaceId,
            name: `工作区 ${share.workspaceId.slice(0, 8)}`,
            active: share.status === 'active',
            sessions: [],
            shareGrantId: share.id,
            shareStatus: share.status,
            role: 'operator',
          })),
        });
      }
    } catch {
      /* 云登录不可用时不影响本机/普通主机列表。 */
    }
  }
  await Promise.all([...registeredHostLinks.keys()].map(refreshHostTopology));
  for (const source of registeredHostLinks.values()) {
    const topology = topologyByHost.get(source.hostId);
    if (!topology) continue;
    const index = next.findIndex((host) => host.id === source.hostId);
    const projected = linkedHost(source, topology, index >= 0 ? next[index] : undefined);
    if (index >= 0) next[index] = projected;
    else next.push(projected);
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
export async function attachSession(
  socket: string,
  target: string,
  workspaceId?: string
): Promise<void> {
  const wid = workspaceId || get(activeWorkspaceId);
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
  token?: string,
  channel: 'lan' | 'public' = 'lan',
): Promise<void> {
  if (channel === 'lan') {
    const raw = addr.trim();
    const url = new URL(raw.includes('://') ? raw : `https://${raw}`);
    const host = url.hostname;
    const secure = url.protocol === 'https:';
    const port = Number(url.port || (secure ? 443 : 80));
    const code = token?.trim();
    if (!host || !Number.isInteger(port) || port < 1 || port > 65535 || !code) {
      throw new Error('LAN 主机地址或 TOTP 无效');
    }
    const link = new RemoteConnection();
    await new Promise<void>((resolve, reject) => {
      const timer = setTimeout(() => {
        off();
        link.disconnect();
        reject(new Error('LAN 主机连接超时'));
      }, 10_000);
      const off = link.onStateChange((state) => {
        if (state === 'connected') {
          clearTimeout(timer);
          off();
          resolve();
        } else if (state === 'error') {
          clearTimeout(timer);
          off();
          reject(new Error(link.lastFailure()?.message || 'LAN 主机拒绝连接'));
        }
      });
      link.connect(host, port, code, 'code', secure);
    });
    const hostId = `lan:${host}:${port}`;
    registerHostTopologyLink({
      hostId,
      kind,
      label: label.trim() || host,
      detail: `${secure ? 'wss' : 'ws'}://${host}:${port}`,
      link,
    });
    await refreshHosts();
    return;
  }
  const deviceName = addr.trim();
  const code = token?.trim();
  if (!deviceName || !code) throw new Error('公网设备名或 TOTP 无效');
  const link = await connectCloudHostTopologyLink(deviceName, code);
  const hostId = `cloud:${deviceName}`;
  registerHostTopologyLink({
    hostId,
    kind,
    label: label.trim() || deviceName,
    detail: '公网同账号 · Cloud E2EE',
    deviceName,
    link,
  });
  await refreshHosts();
}

/** 断开一台远端主机（保留登记）。 */
export async function disconnectHost(hostId: string): Promise<void> {
  const linked = registeredHostLinks.get(hostId);
  if (linked) {
    linked.link.disconnect();
    linked.manualDisconnected = true;
    await refreshHosts();
    return;
  }
  await invoke('disconnect_host', { hostId });
  await refreshHosts();
}

/** 忘记一台远端主机（移除登记）。 */
export async function forgetHost(hostId: string): Promise<void> {
  const linked = registeredHostLinks.get(hostId);
  if (linked) {
    linked.link.disconnect();
    registeredHostLinks.delete(hostId);
    topologyByHost.delete(hostId);
    await refreshHosts();
    return;
  }
  await invoke('forget_host', { hostId });
  await refreshHosts();
}

export async function acceptSharedWorkspace(grantId: string): Promise<void> {
  const token = cloudAuth.snapshot().userToken;
  if (!token) throw new Error('请先登录 Ridge Cloud');
  await acceptWorkspaceShare(token, grantId);
  await refreshHosts();
}

export async function openSharedWorkspace(session: HostSession): Promise<void> {
  if (!session.shareGrantId || !session.ownerUsername || !session.deviceName) {
    throw new Error('共享工作区信息不完整');
  }
  const url =
    `${cloudHttpScheme(BASE_DOMAIN)}://${session.deviceName}-${session.ownerUsername}.${BASE_DOMAIN}` +
    `/?share=${encodeURIComponent(session.shareGrantId)}`;
  const { openUrl } = await import('@tauri-apps/plugin-opener');
  await openUrl(url);
}

export async function revokeSharedWorkspace(grantId: string): Promise<void> {
  const token = cloudAuth.snapshot().userToken;
  if (!token) throw new Error('请先登录 Ridge Cloud');
  await revokeWorkspaceShare(token, grantId);
  await refreshHosts();
}

// ── Outbound foreign PTY (OP-WS-PTY / OP-WS-LIFE / OP-BP-GUARD) ──────────

export interface OutboundStats {
  hostId: string;
  state: string;
  subscribed: string[];
  helloOk: number;
  listOk: number;
  subscribeOk: number;
  writeOk: number;
  resizeOk: number;
  fanoutBytes: number;
  reconnectAttempts: number;
  resubscribeOk: number;
  errors: number;
  liveBufferCap?: number;
  liveBufferBytes?: number;
  liveDroppedBytes?: number;
}

/** Per-host reconnect attempt counters (desktop UI). */
export const outboundReconnectAttempts = writable<Record<string, number>>({});

/** C54 product path: client-side pump shed model keyed by hostId. */
export const outboundPumpByHost = writable<Record<string, PumpState>>({});

/** C58 product path: outbound lifecycle mirror for attached sessions. */
export const outboundLifecycleByKey = writable<Record<string, OutboundSession>>({});

/** C50 product path: history tail badges from get_foreign_history_tail. */
export const foreignHistoryByKey = writable<Record<string, HistoryTailSnapshot>>({});

/**
 * Attach a remote host session into the active workspace as a foreign pane.
 * Backend: attach_host_session (subscribe when outbound bound).
 */
export async function attachRemoteHostSession(
  hostId: string,
  sessionId: string,
): Promise<string> {
  // C50: refresh history tail + seed plan before attach (product path).
  await fetchForeignHistoryTail(hostId, sessionId);
  const seedPlan = attachSeedPlanForSession(hostId, sessionId, false);
  if (seedPlan.seedBeforeLive && seedPlan.seedBytes > 0) {
    // Backend attach_host_session already seeds parser from ForeignHistoryStore;
    // plan is retained for UI diagnostics.
  }
  const wid = get(activeWorkspaceId);
  const linked = registeredHostLinks.get(hostId);
  const paneId = await invoke<string>('attach_host_session', {
    hostId,
    sessionId,
    workspaceId: wid ?? null,
  });
  if (linked && wid) {
    const session = get(hostsStore)
      .find((host) => host.id === hostId)
      ?.sessions.find((item) => item.remoteSessionId === sessionId);
    bindRemotePane({
      localPaneId: paneId,
      hostId,
      workspaceId: session?.workspaceId || '',
      remotePaneId: sessionId,
      link: linked.link,
    });
    setPaneCwd(wid, paneId, session?.cwd);
  }
  noteLifecycleSubscribe(hostId, sessionId);
  await refreshHosts();
  await syncPaneLayoutFromBackend();
  return paneId;
}

/**
 * Detach local foreign view only (remote PTY continues).
 */
export async function detachRemoteHostSession(paneId: string): Promise<void> {
  const wid = get(activeWorkspaceId);
  // Best-effort: mark lifecycle detached for any sessions currently attached
  // on non-headless hosts (C58 product path; remote PTY not killed).
  const hosts = get(hostsStore);
  for (const h of hosts) {
    if (h.kind === 'headless') continue;
    for (const s of h.sessions) {
      if (s.attached && s.remoteSessionId) {
        noteLifecycleDetach(h.id, s.remoteSessionId);
      }
    }
  }
  await invoke('detach_host_session', {
    paneId,
    workspaceId: wid ?? null,
  });
  unbindRemotePane(paneId);
  await refreshHosts();
  await syncPaneLayoutFromBackend();
}

/** Drain outbound inbound buffers into foreign parsers (Hosts poll path). */
export async function pumpHostOutput(hostId: string): Promise<number> {
  try {
    const n = await invoke<number>('pump_host_output', { hostId });
    // C54: feed livePumpPolicy after each real pump so UI badges track shed.
    if (n > 0) {
      notePumpBatch(hostId, n);
      noteLifecycleFanout(hostId, n);
    }
    return n;
  } catch {
    return 0;
  }
}

/** Apply a pump batch into client-side backpressure model (product path). */
export function notePumpBatch(hostId: string, byteLength: number, capHint = 256 * 1024): void {
  outboundPumpByHost.update((m) => {
    const prev = m[hostId] ?? initialPumpState(capHint);
    const { state } = applyPumpBatch(prev, {
      hostId,
      sessionId: '*',
      byteLength,
    });
    // Align cap with server stats when larger
    return { ...m, [hostId]: state };
  });
}

export function pumpBadgeForHost(hostId: string): string {
  const st = get(outboundPumpByHost)[hostId];
  if (!st) return '';
  return formatPumpBadge(st);
}

export function noteLifecycleFanout(hostId: string, bytes: number): void {
  outboundLifecycleByKey.update((m) => {
    const next = { ...m };
    for (const [k, sess] of Object.entries(m)) {
      if (sess.hostId !== hostId || !sess.subscribed) continue;
      next[k] = reduceLifecycle(sess, { type: 'fanout', bytes });
    }
    return next;
  });
}

export function noteLifecycleSubscribe(hostId: string, sessionId: string): void {
  const key = `${hostId}\0${sessionId}`;
  outboundLifecycleByKey.update((m) => {
    const prev = m[key] ?? createSession(hostId, sessionId);
    let s = reduceLifecycle(prev, { type: 'hello_ok' });
    s = reduceLifecycle(s, { type: 'list_ok', sessions: [sessionId] });
    s = safeSubscribe(s, sessionId);
    return { ...m, [key]: s };
  });
}

export function noteLifecycleDetach(hostId: string, sessionId: string): void {
  const key = `${hostId}\0${sessionId}`;
  outboundLifecycleByKey.update((m) => {
    const prev = m[key];
    if (!prev) return m;
    return {
      ...m,
      [key]: reduceLifecycle(prev, { type: 'detach_view' }),
    };
  });
}

export async function fetchOutboundStats(hostId: string): Promise<OutboundStats | null> {
  try {
    const st = await invoke<OutboundStats>('get_outbound_stats', { hostId });
    // Sync client pump cap/bytes from server DTO when present.
    if (st && (st.liveBufferCap || st.liveBufferBytes || st.liveDroppedBytes)) {
      outboundPumpByHost.update((m) => {
        const cap = Number(st.liveBufferCap ?? m[hostId]?.capBytes ?? 256 * 1024);
        const prev = m[hostId] ?? initialPumpState(cap);
        return {
          ...m,
          [hostId]: {
            ...prev,
            capBytes: cap > 0 ? cap : prev.capBytes,
            bufferedBytes: Number(st.liveBufferBytes ?? prev.bufferedBytes),
            droppedBytes: Number(st.liveDroppedBytes ?? prev.droppedBytes),
            highWaterMark: Math.max(
              prev.highWaterMark,
              Number(st.liveBufferBytes ?? 0),
            ),
          },
        };
      });
    }
    // Perf (iter 50): do NOT auto-fetch get_live_backpressure here — that was
    // a second IPC on every Hosts poll. Callers that need aggregate BP invoke
    // fetchLiveBackpressure explicitly (or rely on liveDroppedBytes above).
    return st;
  } catch {
    return null;
  }
}

export interface LiveBackpressureDto {
  hostId: string;
  cap: number;
  buffered: number;
  dropped: number;
  highWater: number;
  sessions: number;
  sheddingSessions: number;
  level: string;
  totalDroppedGlobal: number;
  injects: number;
}

export const liveBackpressureByHost = writable<Record<string, LiveBackpressureDto>>({});

/** C8 product path: desktop get_live_backpressure aggregate. */
export async function fetchLiveBackpressure(
  hostId: string,
): Promise<LiveBackpressureDto | null> {
  try {
    const raw = await invoke<LiveBackpressureDto>('get_live_backpressure', { hostId });
    liveBackpressureByHost.update((m) => ({ ...m, [hostId]: raw }));
    if (raw && (raw.dropped > 0 || raw.buffered > 0)) {
      outboundPumpByHost.update((m) => {
        const prev = m[hostId] ?? initialPumpState(Number(raw.cap) || 256 * 1024);
        return {
          ...m,
          [hostId]: {
            ...prev,
            capBytes: Number(raw.cap) || prev.capBytes,
            bufferedBytes: Number(raw.buffered),
            droppedBytes: Math.max(prev.droppedBytes, Number(raw.dropped)),
            highWaterMark: Math.max(prev.highWaterMark, Number(raw.highWater)),
          },
        };
      });
    }
    return raw;
  } catch {
    return null;
  }
}

/**
 * After refresh: for each Connected remote host, pump outbound live output.
 * This is the shipped UI-side read-loop companion to pump_host_output.
 */
export async function pumpAllConnectedOutbound(): Promise<number> {
  const hosts = get(hostsStore);
  const targets = hosts.filter((h) =>
    h.kind !== 'headless'
    && h.status === 'connected'
    && !registeredHostLinks.has(h.id));
  // Perf (iter 50): pump hosts in parallel — sequential IPC stacked latency.
  const parts = await Promise.all(targets.map((h) => pumpHostOutput(h.id)));
  return parts.reduce((a, b) => a + b, 0);
}

/** C50: pull foreign history tail for attach badge (desktop-only command). */
export async function fetchForeignHistoryTail(
  hostId: string,
  sessionId: string,
): Promise<HistoryTailSnapshot | null> {
  try {
    const raw = await invoke<{
      hostId: string;
      sessionId: string;
      bytes: number;
      cap: number;
      dataB64?: string;
    }>('get_foreign_history_tail', { hostId, sessionId });
    const snap: HistoryTailSnapshot = {
      hostId: raw.hostId,
      sessionId: raw.sessionId,
      bytes: raw.bytes,
      cap: raw.cap,
      dataB64: raw.dataB64,
    };
    const key = `${hostId}\0${sessionId}`;
    foreignHistoryByKey.update((m) => ({ ...m, [key]: snap }));
    return snap;
  } catch {
    return null;
  }
}

export function historyBadgeForSession(hostId: string, sessionId: string): string {
  const snap = get(foreignHistoryByKey)[`${hostId}\0${sessionId}`];
  return summarizeHistoryBadge(snap ?? null);
}

/** Plan attach seed from last known history (product decision for UI). */
export function attachSeedPlanForSession(
  hostId: string,
  sessionId: string,
  reattach: boolean,
): ReturnType<typeof planAttachSeed> {
  const snap = get(foreignHistoryByKey)[`${hostId}\0${sessionId}`];
  return planAttachSeed({
    localTailBytes: snap?.bytes ?? 0,
    rows: 24,
    cols: 80,
    reattach,
    hostHistoryKnown: (snap?.bytes ?? 0) > 0,
  });
}

/** Operator line for a host from pump + reconnect (Hosts panel). */
export function hostOperatorAlert(
  hostId: string,
  reconnectAttempt: number,
): string | null {
  const pump = get(outboundPumpByHost)[hostId];
  const bp = pump
    ? {
        bufferedBytes: pump.bufferedBytes,
        capBytes: pump.capBytes,
        droppedBytes: pump.droppedBytes,
        highWaterMark: pump.highWaterMark,
      }
    : snapshotFromOutboundStats({});
  return hostsLineAlert({ backpressure: bp, reconnectAttempt });
}

/** Record a reconnect attempt for UI badge (pure counter; schedule in outboundReconnect). */
export function noteOutboundReconnectAttempt(hostId: string): number {
  let next = 0;
  outboundReconnectAttempts.update((m) => {
    next = (m[hostId] ?? 0) + 1;
    return { ...m, [hostId]: next };
  });
  return next;
}

export function resetOutboundReconnectAttempt(hostId: string): void {
  outboundReconnectAttempts.update((m) => {
    const copy = { ...m };
    delete copy[hostId];
    return copy;
  });
}

