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
        sessions: (r.sessions ?? []).map((s) => ({
          socket: r.id,
          name: s.title || s.id,
          remoteSessionId: s.id,
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
  const paneId = await invoke<string>('attach_host_session', {
    hostId,
    sessionId,
    workspaceId: wid ?? null,
  });
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
  const targets = hosts.filter((h) => h.kind !== 'headless' && h.status === 'connected');
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

