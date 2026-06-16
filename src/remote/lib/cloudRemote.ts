// src/remote/lib/cloudRemote.ts
//
// Cloud control-end transport for the mobile app (design 2026-06-16-mobile-cloud).
//
// The mobile UI (MainApp / BottomTabBar / WorkspaceTree) is written against the
// LAN wsRemote flat protocol ({@link RemoteLink}). The cloud host answerer
// (cloudHostBridge.ts), however, only speaks the Tauri-invoke surface the desktop
// SPA uses — JSON-RPC over the WebRTC E2EE DataChannel, gated by the §5.4 capability
// allow-list and the §4 zero-trust TOTP. That flat protocol is actually `server.rs`
// translating the host's pane-tree/PTY model down for mobile; the cloud bridge does
// NOT do that translation.
//
// So this class RE-DERIVES server.rs's translation on the CLIENT, on top of the
// tauriShim bridge (already attached by cloudControllerBoot). Net effect: the exact
// same mobile UI runs over the secure cloud path with ZERO host changes (the host
// side is the already-shipped desktop binary that desktop-app proves works).
//
// Wire reuse (no new byte path invented — all of this already exists for desktop-app):
//   - invoke(...)                 → bridge.invoke → rpc.request (allow-list gated)
//   - register_pane_delta_channel → core.ts special-cases to bridge.subscribePane
//                                   → 'subscribe-pane' notify → host streams 0x10 pane bytes
//   - listen('pty-output-{ws}-{pane}') → bridge fans the pane bytes (decoded) here
//
// Auth/boot lives in App.svelte (cookie bootstrap → cloudControllerBoot → TOTP gate);
// this class is constructed AFTER the bridge is connected + TOTP-verified, so invoke/
// listen are live. It holds the boot handle so disconnect() tears down the WebRTC.

import { Channel, invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import type { PaneNode } from '$lib/types';
import type { CloudControllerHandle } from '$lib/remote/cloud/cloudControllerBoot';
import type {
  RemoteLink,
  ConnectionState,
  PaneInfo,
  WorkspaceInfo,
  WsMessage,
  RawByteListener,
  MetaListener,
  PtyResizeListener,
  ThemeListener,
  ThemeSnapshot,
} from './wsRemote';

/** Backend `list_workspaces` row (subset we use). */
interface BackendWorkspace {
  id: string;
  name?: string | null;
}

/** Default PTY grid for a freshly-activated pane until the canvas claims its real size. */
const DEFAULT_ROWS = 24;
const DEFAULT_COLS = 80;

/** Flatten a host pane-tree to the mobile's flat leaf list (server.rs's downgrade). */
function flattenLeaves(node: PaneNode | null | undefined): PaneInfo[] {
  if (!node) return [];
  if (node.type === 'leaf') {
    if (!node.id) return []; // pre-hydration placeholder leaf
    return [{ id: node.id, title: node.title, cwd: node.cwd }];
  }
  return node.children.flatMap(flattenLeaves);
}

export class CloudRemoteConnection implements RemoteLink {
  private readonly handle: CloudControllerHandle;

  private _state: ConnectionState = 'connecting';
  private _activeWorkspaceId = '';
  private _refreshSeq = 0;

  private stateListeners = new Set<(s: ConnectionState) => void>();
  private reconnectListeners = new Set<() => void>();
  private messageListeners = new Set<(msg: WsMessage) => void>();
  private rawByteListeners = new Set<RawByteListener>();
  private metaListeners = new Set<MetaListener>();
  private resizeListeners = new Set<PtyResizeListener>();
  private themeListeners = new Set<ThemeListener>();

  // Per-pane `pty-output-*` unlisten handles (bounded via pruneOutputs / disconnect).
  private ptyUnlisten = new Map<string, UnlistenFn>();
  // Panes whose subscribe is in flight (so concurrent subscribe calls stay idempotent).
  private subscribing = new Set<string>();
  // pane-tree-changed unlisten (host-side layout changes → re-list panes).
  private treeUnlisten: UnlistenFn | null = null;
  // Per-pane decoders are unnecessary: the bridge already decodes bytes→string with a
  // streaming TextDecoder; we re-encode here to feed the byte-oriented mobile canvas.
  private readonly encoder = new TextEncoder();
  private _lastTheme: ThemeSnapshot | null = null;

  constructor(handle: CloudControllerHandle) {
    this.handle = handle;
  }

  /**
   * Bring the connection up: read the host's active workspace, watch host-side
   * layout changes, and flip to 'connected'. Must be awaited by App.svelte BEFORE
   * mounting MainApp, since MainApp.onMount calls listPanes()/refreshWorkspaces()
   * which depend on the active workspace id.
   */
  async init(): Promise<void> {
    try {
      this._activeWorkspaceId = await invoke<string>('get_active_workspace_id');
    } catch {
      this._activeWorkspaceId = '';
    }
    // Host pushes `pane-tree-changed` when ITS layout mutates (desktop user splits /
    // closes / a teammate agent reshapes panes). Re-list so the mobile tracks it.
    try {
      this.treeUnlisten = await listen('pane-tree-changed', () => {
        void this._refreshPanes();
      });
    } catch {
      /* event subscribe failed — non-fatal, manual refresh still works */
    }
    this.setState('connected');
  }

  /** App.svelte forwards the boot's ongoing onState (drop/error) into the UI here. */
  notifyState(s: ConnectionState): void {
    this.setState(s);
  }

  // ── state / listeners ──────────────────────────────────────────────────────
  state(): ConnectionState {
    return this._state;
  }
  private setState(s: ConnectionState): void {
    this._state = s;
    this.stateListeners.forEach((fn) => fn(s));
  }
  onStateChange(fn: (s: ConnectionState) => void): () => void {
    this.stateListeners.add(fn);
    return () => this.stateListeners.delete(fn);
  }
  onReconnect(fn: () => void): () => void {
    // The WebRTC adapter + bridge re-subscribe panes internally on a transport
    // blip; we don't surface an explicit reconnect in v1 (no resync storm). Kept
    // for interface parity so MainApp's listener registers harmlessly.
    this.reconnectListeners.add(fn);
    return () => this.reconnectListeners.delete(fn);
  }
  onMessage(fn: (msg: WsMessage) => void): () => void {
    this.messageListeners.add(fn);
    return () => this.messageListeners.delete(fn);
  }
  onRawBytes(fn: RawByteListener): () => void {
    this.rawByteListeners.add(fn);
    return () => this.rawByteListeners.delete(fn);
  }
  onMetadata(fn: MetaListener): () => void {
    this.metaListeners.add(fn);
    return () => this.metaListeners.delete(fn);
  }
  onPtyResize(fn: PtyResizeListener): () => void {
    // The cloud host doesn't push host→controller pty-resized; the mobile drives
    // its own size via resize_pane. Registered for parity; effectively unused.
    this.resizeListeners.add(fn);
    return () => this.resizeListeners.delete(fn);
  }
  onTheme(fn: ThemeListener): () => void {
    this.themeListeners.add(fn);
    return () => this.themeListeners.delete(fn);
  }
  lastTheme(): ThemeSnapshot | null {
    return this._lastTheme;
  }

  private emitMessage(msg: WsMessage): void {
    this.messageListeners.forEach((fn) => fn(msg));
  }

  // ── panes ──────────────────────────────────────────────────────────────────
  listPanes(): void {
    void this._refreshPanes();
  }

  private async _refreshPanes(): Promise<void> {
    let leaves: PaneInfo[];
    try {
      const layout = await invoke<PaneNode>('get_pane_layout');
      leaves = flattenLeaves(layout);
    } catch {
      return; // host not ready / transient — leave the UI as-is
    }
    this.emitMessage({ type: 'panes', panes: leaves });
    // No native pty-meta event over cloud: derive title/cwd from the layout leaves
    // so the breadcrumb + sidebar cwd track (refreshed again on pane-tree-changed).
    for (const p of leaves) {
      this.metaListeners.forEach((fn) => fn(p.id, p.title ?? null, p.cwd ?? null));
    }
  }

  subscribePane(paneId: string): void {
    if (!paneId || this.ptyUnlisten.has(paneId) || this.subscribing.has(paneId)) return;
    this.subscribing.add(paneId);
    void this._subscribe(paneId);
  }

  private async _subscribe(paneId: string): Promise<void> {
    try {
      // Per-pane `pty-output-{ws}-{pane}` event. The bridge keys its dispatch on the
      // trailing pane UUID only, so the ws segment is cosmetic — but we use the real
      // active ws for fidelity. Payload arrives as decoded `{data}`; re-encode to bytes.
      const unlisten = await listen<{ data: string }>(
        `pty-output-${this._activeWorkspaceId}-${paneId}`,
        (e) => {
          const bytes = this.encoder.encode(e.payload?.data ?? '');
          if (bytes.length) this.rawByteListeners.forEach((fn) => fn(paneId, bytes));
        },
      );
      // Idempotency guard: a teardown between listen() awaiting and resolving.
      if (!this.subscribing.has(paneId)) {
        unlisten();
        return;
      }
      this.ptyUnlisten.set(paneId, unlisten);
      // Tell the host to start streaming (core.ts maps this to bridge.subscribePane →
      // 'subscribe-pane' notify; the Channel arg is ignored in the browser shim).
      await invoke('register_pane_delta_channel', {
        workspaceId: this._activeWorkspaceId,
        paneId,
        channel: new Channel(),
      });
    } catch {
      /* subscribe failed — pane stays blank; a later refresh/re-subscribe retries */
    } finally {
      this.subscribing.delete(paneId);
    }
  }

  sendStdin(paneId: string, data: string): void {
    if (!paneId) return;
    void invoke('write_to_pty', { paneId, data }).catch(() => {});
  }

  refreshPane(paneId: string, rows: number, cols: number): void {
    this._resize(paneId, rows, cols);
  }
  claimPane(paneId: string, rows: number, cols: number): void {
    this._resize(paneId, rows, cols);
  }
  private _resize(paneId: string, rows: number, cols: number): void {
    if (!paneId || rows <= 0 || cols <= 0) return;
    this._refreshSeq++;
    void invoke('resize_pane', {
      workspaceId: this._activeWorkspaceId,
      paneId,
      rows,
      cols,
    }).catch(() => {});
  }
  lastRefreshSeq(): number {
    return this._refreshSeq;
  }

  async createPane(): Promise<string | null> {
    try {
      const layout = await invoke<PaneNode>('get_pane_layout');
      const leaves = flattenLeaves(layout);
      if (leaves.length > 0) {
        // Add a terminal to the current workspace by splitting the first leaf — the
        // desktop's own "new terminal" primitive. The mobile renders one pane at a
        // time, so it just shows the new pane; the host sees a split (shared reality).
        const result = await invoke<{ pane_id: string }>('split_pane', {
          paneId: leaves[0].id,
          direction: 'horizontal',
        });
        return result.pane_id || null;
      }
      // Empty workspace: spin up a fresh one and surface its first pane.
      await invoke<string>('create_workspace');
      const after = flattenLeaves(await invoke<PaneNode>('get_pane_layout'));
      return after[0]?.id ?? null;
    } catch {
      return null;
    }
  }

  async closePane(paneId: string): Promise<boolean> {
    try {
      await invoke('close_pane', { paneId });
      const unlisten = this.ptyUnlisten.get(paneId);
      if (unlisten) {
        this.ptyUnlisten.delete(paneId);
        try { unlisten(); } catch { /* already gone */ }
      }
      void this._refreshPanes();
      return true;
    } catch {
      return false;
    }
  }

  // ── workspaces ───────────────────────────────────────────────────────────────
  async listWorkspaces(): Promise<{ workspaces: WorkspaceInfo[] }> {
    try {
      const [list, activeId] = await Promise.all([
        invoke<BackendWorkspace[]>('list_workspaces'),
        invoke<string>('get_active_workspace_id').catch(() => this._activeWorkspaceId),
      ]);
      if (activeId) this._activeWorkspaceId = activeId;
      const workspaces = (list ?? []).map((w) => ({
        id: w.id,
        name: w.name ?? undefined,
        active: w.id === activeId,
      }));
      return { workspaces };
    } catch {
      return { workspaces: [] };
    }
  }

  async switchWorkspace(workspaceId: string): Promise<boolean> {
    try {
      await invoke('switch_workspace', { workspaceId });
      this._activeWorkspaceId = workspaceId;
      return true;
    } catch {
      return false;
    }
  }

  async createWorkspace(name?: string): Promise<string | null> {
    try {
      const id = await invoke<string>('create_workspace', name ? { name } : {});
      return id || null;
    } catch {
      return null;
    }
  }

  async closeWorkspace(workspaceId: string): Promise<boolean> {
    try {
      await invoke('close_workspace', { workspaceId });
      return true;
    } catch {
      return false;
    }
  }

  async listWorkspacePanes(workspaceId: string): Promise<PaneInfo[]> {
    try {
      const layout = await invoke<PaneNode>('get_pane_layout_for', { workspaceId });
      return flattenLeaves(layout);
    } catch {
      return [];
    }
  }

  // ── theme (v1 best-effort) ────────────────────────────────────────────────────
  // Full theme parity needs the active-theme id (settings) + a color-shape map from
  // the host ThemeEntry to the mobile's palette. Deferred: the mobile keeps its CSS
  // default theme over cloud. cycleTheme still drives the host so the desktop reflects
  // it; we just don't re-skin the mobile in v1.
  cycleTheme(_currentId: string): void {
    /* no-op in v1 — see note above */
  }

  // ── misc / parity stubs ────────────────────────────────────────────────────────
  setHostClipboard(_text: string): void {
    // No cloud command for writing the host's system clipboard; best-effort no-op
    // (the LAN path is itself fire-and-forget).
  }
  connect(): void {
    // Cloud boots via cloudControllerBoot in App.svelte, never via this signature.
  }
  getPaneOutput(): string[] {
    return [];
  }
  pruneOutputs(liveIds: Set<string>): void {
    // Release `pty-output` listeners for panes the host no longer reports (bounds
    // listener growth on a long-lived PWA tab — mirrors the LAN pruneOutputs intent).
    for (const [paneId, unlisten] of [...this.ptyUnlisten]) {
      if (!liveIds.has(paneId)) {
        this.ptyUnlisten.delete(paneId);
        try { unlisten(); } catch { /* already gone */ }
      }
    }
  }
  send(): void {
    // Raw wsRemote frames have no meaning on the cloud invoke bridge.
  }

  disconnect(): void {
    this.setState('disconnected');
    for (const [, unlisten] of this.ptyUnlisten) {
      try { unlisten(); } catch { /* already gone */ }
    }
    this.ptyUnlisten.clear();
    this.subscribing.clear();
    if (this.treeUnlisten) {
      try { this.treeUnlisten(); } catch { /* already gone */ }
      this.treeUnlisten = null;
    }
    // Tear down the WebRTC / E2EE session (idempotent).
    try { this.handle.disconnect(); } catch { /* already torn down */ }
  }
}
