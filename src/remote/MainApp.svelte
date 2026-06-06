<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import { t, tr } from '$lib/i18n';
  import TerminalCanvas from './lib/TerminalCanvas.svelte';
  import TopBar from './TopBar.svelte';
  import BottomTabBar from './BottomTabBar.svelte';
  import RemoteSidebar from './lib/RemoteSidebar.svelte';
  import { RemoteConnection, type PaneInfo, type ConnectionState, type WorkspaceInfo } from './lib/wsRemote';
  import { applyThemeVars, buildKernelTheme } from './lib/theme';

  let { ws }: { ws: RemoteConnection } = $props();
  let panes = $state<PaneInfo[]>([]);
  let activePaneId = $state<string | null>(null);
  let wsState = $state<ConnectionState>('disconnected');
  let workspaces = $state<WorkspaceInfo[]>([]);
  let activeWorkspaceId = $state<string>('');
  let showKeyboard = $state(false);
  let sidebarTab: 'files' | 'git' | 'search' | null = $state(null);
  // Active pane's working dir — roots the sidebar at the same place ridge shows.
  let activeCwd = $state('');
  // §remote 新建终端：空状态下让远程端自行创建终端，不再依赖桌面端先开一个。
  let creatingPane = $state(false);
  let createError = $state('');

  let canvasRef: TerminalCanvas | undefined = $state();
  // Kernel palette derived from the desktop theme; applied to the canvas once it
  // mounts (the theme push usually arrives before the terminal exists).
  let kernelTheme: Record<string, string> | null = $state(null);
  let backendName = $state('Canvas2D');

  // §terminal-isolation + scrollback-cache: the local kernel is a single shared
  // instance, so switching panes MUST wipe it (resetForSwitch) — otherwise the
  // previous pane's scrollback bleeds into the new one (上滚串台). We also keep
  // each pane's raw byte stream so a switch repaints instantly from cache, and
  // mirror the active pane to sessionStorage so a reload restores instantly
  // before the host reconnects. The host re-sends ≤64KB scrollback on
  // (re)subscribe; we tail-match it against the cache to avoid double-painting.
  const PANE_BUF_CAP = 256 * 1024;
  const SS_CAP = 48 * 1024;
  const paneBuffers = new Map<string, Uint8Array>();
  let subscribedPaneId: string | null = null;
  let expectReplayPane: string | null = null;
  let ssMirrorTimer: ReturnType<typeof setTimeout> | null = null;

  function ssKey(id: string) { return `rg-remote-sb:${id}`; }
  function bytesToB64(b: Uint8Array): string {
    let s = '';
    for (let i = 0; i < b.length; i++) s += String.fromCharCode(b[i]);
    return btoa(s);
  }
  function b64ToBytes(s: string): Uint8Array {
    const bin = atob(s);
    const out = new Uint8Array(bin.length);
    for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
    return out;
  }
  function bytesEndsWith(hay: Uint8Array, tail: Uint8Array): boolean {
    if (tail.length === 0) return true;
    if (tail.length > hay.length) return false;
    const off = hay.length - tail.length;
    for (let i = 0; i < tail.length; i++) if (hay[off + i] !== tail[i]) return false;
    return true;
  }
  function appendPaneBuffer(id: string, data: Uint8Array): void {
    const prev = paneBuffers.get(id);
    let next: Uint8Array;
    if (!prev) { next = data.slice(); }
    else { next = new Uint8Array(prev.length + data.length); next.set(prev); next.set(data, prev.length); }
    if (next.length > PANE_BUF_CAP) next = next.slice(next.length - PANE_BUF_CAP);
    paneBuffers.set(id, next);
  }
  function loadPaneFromSession(id: string): Uint8Array | null {
    try { const s = sessionStorage.getItem(ssKey(id)); return s ? b64ToBytes(s) : null; }
    catch { return null; }
  }
  function scheduleSessionMirror(id: string) {
    if (ssMirrorTimer) return;
    ssMirrorTimer = setTimeout(() => {
      ssMirrorTimer = null;
      const buf = paneBuffers.get(id);
      if (!buf) return;
      try {
        const tail = buf.length > SS_CAP ? buf.subarray(buf.length - SS_CAP) : buf;
        sessionStorage.setItem(ssKey(id), bytesToB64(tail));
      } catch { /* quota exceeded / disabled — ignore */ }
    }, 600);
  }

  function applyTheme(colors: Record<string, string>) {
    applyThemeVars(colors);
    kernelTheme = buildKernelTheme(colors);
  }

  function onStdin(data: string) {
    if (activePaneId) ws.sendStdin(activePaneId, data);
  }

  // Automatic refit (ResizeObserver / visualViewport): the controller fires this
  // only when the grid actually changed (cols/rows/DPR delta), i.e. a genuine
  // viewport change that needs the host to reflow. A bare `resize` is host-side
  // bookkeeping that never touches the PTY, so the remote stayed clipped/garbled
  // until the manual refresh button. `claimPane` runs the SAME host path as that
  // button (resize real PTY + parser, broadcast `pty-resized`), giving automatic
  // 自适应全屏 reflow without the manual tap.
  function onResize(paneId: string, rows: number, cols: number, pixelWidth: number, pixelHeight: number) {
    ws.claimPane(paneId, rows, cols, pixelWidth, pixelHeight);
  }

  function handleRefresh() {
    if (activePaneId && canvasRef) {
      const d = canvasRef.getDims();
      if (d) ws.refreshPane(activePaneId, d.rows, d.cols, d.pixelWidth, d.pixelHeight);
    }
    ws.listPanes();
    refreshWorkspaces();
  }

  let _refreshTimer: ReturnType<typeof setTimeout> | null = null;
  let _refreshSeq = 0;

  function refreshActivePane() {
    if (!activePaneId || !canvasRef) return;
    const pid = activePaneId;
    const d = canvasRef.getDims();
    if (!d) return;
    // Debounce: coalesce rapid calls
    if (_refreshTimer) clearTimeout(_refreshTimer);
    _refreshTimer = setTimeout(() => {
      _refreshTimer = null;
      const cur = ws.lastRefreshSeq();
      if (cur <= _refreshSeq) return; // stale, a newer call already went through
      _refreshSeq = cur;
      ws.refreshPane(pid, d.rows, d.cols, d.pixelWidth, d.pixelHeight);
    }, 100);
  }

  async function refreshWorkspaces() {
    try {
      const data = await ws.listWorkspaces();
      workspaces = data.workspaces || [];
      const active = workspaces.find(w => w.active);
      if (active) activeWorkspaceId = active.id;
    } catch { /* ignore */ }
  }

  function handleSidebarToggle(tab: 'files' | 'git' | 'search') {
    if (sidebarTab === tab) {
      sidebarTab = null;
    } else {
      sidebarTab = tab;
    }
  }

  // 远程端自建终端：请求 host 创建 pane，成功后刷新列表并把新 pane 设为活动项
  // （onMessage 的 'panes' 分支会在 listPanes 回包后把 activePaneId 兜底为首个，
  //  这里显式置为新 id 以确保即使有多个 pane 也聚焦到刚建的那个）。失败时把错误
  // 文案显示给用户，绝不静默吞掉。
  async function handleCreatePane() {
    if (creatingPane) return;
    creatingPane = true;
    createError = '';
    try {
      const newId = await ws.createPane();
      if (newId) {
        activePaneId = newId;
        ws.listPanes();
      } else {
        createError = tr('mobile.createTerminalFailRetry');
      }
    } catch (e) {
      createError = e instanceof Error ? e.message : tr('mobile.createTerminalFail');
    } finally {
      creatingPane = false;
    }
  }

  onMount(() => {
    ws.onStateChange((s) => wsState = s);
    ws.onMessage((msg) => {
      if (msg.type === 'panes') {
        panes = msg.panes;
        const paneIds = msg.panes.map(p => p.id);
        if (!activePaneId || !paneIds.includes(activePaneId)) {
          activePaneId = msg.panes.length > 0 ? msg.panes[0].id : null;
        }
      }
      if (msg.type === 'workspaces') {
        workspaces = msg.workspaces;
        const active = workspaces.find(w => w.active);
        if (active) activeWorkspaceId = active.id;
      }
      if (msg.type === 'switch-workspace-result') {
        if (msg.success && msg.workspaceId) {
          activeWorkspaceId = msg.workspaceId;
        }
        refreshWorkspaces();
      }
      if (msg.type === 'create-workspace-result' || msg.type === 'close-workspace-result') {
        refreshWorkspaces();
      }
      if (msg.type === 'workspace-renamed') {
        workspaces = workspaces.map(w =>
          w.id === msg.workspaceId ? { ...w, name: msg.name } : w
        );
      }
    });
    ws.onRawBytes((paneId, data) => {
      // The host streams only the subscribed (active) pane; ignore stragglers.
      if (paneId !== activePaneId) return;
      if (expectReplayPane === paneId) {
        // First chunk after (re)subscribe = the host's on-subscribe scrollback
        // replay. If our cache already ends with it we pre-painted it on switch
        // → drop the redundant replay. Otherwise the pane changed while we were
        // away (or the cache was empty/short) → wipe + repaint authoritatively.
        expectReplayPane = null;
        const cached = paneBuffers.get(paneId);
        if (cached && bytesEndsWith(cached, data)) return;
        canvasRef?.resetForSwitch();
        canvasRef?.feedUtf8(data);
        paneBuffers.set(paneId, data.length > PANE_BUF_CAP ? data.slice(data.length - PANE_BUF_CAP) : data.slice());
        scheduleSessionMirror(paneId);
        return;
      }
      // Live output.
      appendPaneBuffer(paneId, data);
      canvasRef?.feedUtf8(data);
      scheduleSessionMirror(paneId);
    });
    ws.onMetadata((paneId, title, cwd) => {
      if (paneId === activePaneId) {
        // Title drives the document/tab title directly.
        if (title != null && title.length > 0) document.title = title;
        // cwd roots the sidebar (file tree / git / search) at the pane's dir.
        if (cwd != null && cwd.length > 0) activeCwd = cwd;
      }
    });
    ws.onPtyResize((paneId, rows, cols) => {
      if (paneId === activePaneId) {
        canvasRef?.resizeKernel(rows, cols);
      }
    });
    // Theme: apply the snapshot pushed at connect (cached, since it usually
    // arrives before this listener), then follow any later pushes.
    const t0 = ws.lastTheme();
    if (t0) applyTheme(t0.colors);
    ws.onTheme((colors) => applyTheme(colors));
    // Reconnect resync: a reconnect opens a brand-new host socket that holds no
    // pane subscription, and the local kernel still shows the stale pre-drop
    // screen. Reset it (RIS) so the host's scrollback replay + full repaint paint
    // a correct, current view instead of appending under stale content, then
    // re-establish the subscription, workspace state, and viewport size claim.
    ws.onReconnect(() => {
      // Reconnect opens a fresh host socket with no pane subscription; the local
      // kernel still shows the stale pre-drop screen. Wipe it, pre-paint the
      // cache for instant feedback, then re-subscribe — the host replays the
      // pane's scrollback, reconciled in onRawBytes (expectReplayPane).
      canvasRef?.resetForSwitch();
      const pid = activePaneId;
      if (pid) {
        const cached = paneBuffers.get(pid);
        if (cached && cached.length > 0) canvasRef?.feedUtf8(cached);
        expectReplayPane = pid;
        ws.subscribePane(pid);
      }
      ws.listPanes();
      refreshWorkspaces();
      refreshActivePane();
    });
    ws.listPanes();
    refreshWorkspaces();
    return () => { ws.disconnect(); };
  });

  // Pane switch: isolate the kernel + (re)subscribe. Reacts to activePaneId only;
  // canvas ops run untracked so the canvas's async mount doesn't re-trigger a
  // re-subscribe (which would double the host scrollback replay).
  $effect(() => {
    const pid = activePaneId;
    if (!pid) { subscribedPaneId = null; return; } // null gap → force re-subscribe next
    untrack(() => {
      if (pid === subscribedPaneId) return;
      subscribedPaneId = pid;
      // §isolation: wipe the kernel so the previous pane can't bleed into this one.
      canvasRef?.resetForSwitch();
      // Instant pre-paint from cache (in-memory; else sessionStorage on reload).
      let cached = paneBuffers.get(pid);
      if (!cached) {
        const restored = loadPaneFromSession(pid);
        if (restored) { paneBuffers.set(pid, restored); cached = restored; }
      }
      if (cached && cached.length > 0) canvasRef?.feedUtf8(cached);
      // The host replays this pane's scrollback on subscribe — reconcile it
      // against the cache in onRawBytes to avoid double-painting.
      expectReplayPane = pid;
      ws.subscribePane(pid);
    });
  });

  $effect(() => {
    if (activePaneId && canvasRef) {
      refreshActivePane();
    }
  });

  // Apply the kernel palette once the canvas exists (theme can arrive earlier).
  $effect(() => {
    if (canvasRef && kernelTheme) canvasRef.applyTheme(kernelTheme);
  });

  // Seed the sidebar root from the active pane's cwd (pty-meta refines it live).
  $effect(() => {
    const p = panes.find((pp) => pp.id === activePaneId);
    if (p?.cwd) activeCwd = p.cwd;
  });
</script>

<div class="app-root">
  <TopBar {panes} {activePaneId} {workspaces} {activeWorkspaceId} {wsState} />

  {#if panes.length === 0}
    <div class="empty">
      <p>{$t('mobile.noActiveTerminal')}</p>
      <button class="create-btn" onclick={handleCreatePane} disabled={creatingPane}>
        {creatingPane ? $t('mobile.creating') : $t('mobile.newTerminal')}
      </button>
      {#if createError}<p class="create-error">{createError}</p>{/if}
    </div>
  {:else if activePaneId}
    <TerminalCanvas
      bind:this={canvasRef}
      bind:backendName
      paneId={activePaneId ?? null}
      {onStdin}
      {onResize}
      {showKeyboard}
    />
  {/if}

  {#if sidebarTab !== null}
    <div class="sidebar-overlay" onclick={() => sidebarTab = null} role="presentation"></div>
    <RemoteSidebar tab={sidebarTab} cwd={activeCwd} onClose={() => sidebarTab = null} onTabChange={(t) => sidebarTab = t} />
  {/if}

  <BottomTabBar
    {ws}
    {sidebarTab}
    {backendName}
    onSidebarToggle={handleSidebarToggle}
    onRefresh={handleRefresh}
    bind:showKeyboard
    {panes}
    bind:activePaneId
    {workspaces}
    bind:activeWorkspaceId
    onWorkspacesChanged={refreshWorkspaces}
  />
</div>

<style>
  .app-root{position:fixed;inset:0;display:flex;flex-direction:column;background:var(--rg-bg);color:var(--rg-fg)}
  .empty{flex:1;display:flex;flex-direction:column;align-items:center;justify-content:center;color:var(--rg-fg-muted);gap:12px}
  .create-btn{padding:8px 20px;border:1px solid var(--rg-accent);border-radius:8px;background:color-mix(in srgb,var(--rg-accent) 14%,transparent);color:var(--rg-fg);font-size:14px;font-weight:600;cursor:pointer;transition:all .15s}
  .create-btn:active{background:color-mix(in srgb,var(--rg-accent) 26%,transparent)}
  .create-btn:disabled{opacity:.5;cursor:not-allowed}
  .create-error{font-size:12px;color:var(--rg-ansi-red)}
  .sidebar-overlay{position:fixed;inset:0;background:rgba(0,0,0,0.5);z-index:40;touch-action:none}
</style>