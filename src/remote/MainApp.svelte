<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import { t, tr } from '$lib/i18n';
  import { Folder, GitBranch, Search, Keyboard } from 'lucide-svelte';
  // Type-only import of the lazily-loaded TerminalCanvas, used solely to type
  // the bind:this instance ref below. Erased at build, so it does NOT defeat
  // the dynamic import / lazy-load on the next line.
  import type TerminalCanvasComponent from './lib/TerminalCanvas.svelte';
  // §lazy-load: heavy components loaded on demand to reduce initial bundle.
  // TerminalCanvas (with WASM) is only needed after auth + pane selection.
  const TerminalCanvas = import('./lib/TerminalCanvas.svelte');
  // VirtualKeyboard is only needed when user toggles it via header button.
  const VirtualKeyboard = import('./lib/VirtualKeyboard.svelte');
  // RemoteSidebar (file tree, git, search) loaded when sidebar is opened.
  const RemoteSidebar = import('./lib/RemoteSidebar.svelte');
  // FileViewer (read-only file / git-diff overlay) loaded on first open.
  const FileViewer = import('./lib/FileViewer.svelte');
  import BottomTabBar from './BottomTabBar.svelte';
  import { type RemoteLink, type PaneInfo, type ConnectionState, type WorkspaceInfo, type ConnectionFailure } from './lib/wsRemote';
  import { applyThemeVars, buildKernelTheme } from './lib/theme';
  import { createWsSidebarProvider } from './lib/sidebarProvider';
  import { PaneScrollbackCache, PANE_BUF_CAP } from './lib/paneScrollbackCache';

  let { ws }: { ws: RemoteLink } = $props();
  let panes = $state<PaneInfo[]>([]);
  let activePaneId = $state<string | null>(null);
  // The active pane object (for its title in the header breadcrumb), derived
  // from the live `panes` list by id — mirrors the panes.find(...) lookup used
  // for the active cwd below.
  let activePane = $derived(panes.find((p) => p.id === activePaneId));
  let wsState = $state<ConnectionState>('disconnected');
  // §fail-grading（任务 A 问题1）：最近一次失败分级。驱动顶部 banner 的差异化处置——
  // 'user'（账户/权限不匹配）退回登录、'parked'（设备停用）提示去控制台、'channel'
  // （信令/网络/并发）显示「通道异常」并允许重试。
  let failure = $state<ConnectionFailure | null>(null);
  let workspaces = $state<WorkspaceInfo[]>([]);
  let activeWorkspaceId = $state<string>('');
  // §selection: explicit selection mode (toggled in BottomTabBar). When on, a
  // single-finger drag selects; when off it scrolls (no accidental selection).
  let selectionMode = $state(false);
  let sidebarTab: 'files' | 'git' | 'search' | null = $state(null);
  // Read-only file / git-diff viewer overlay. Opened from the sidebar (tap a
  // file in the tree / a search hit → 'file'; tap a changed file in git → 'diff').
  let viewer = $state<{ kind: 'file' | 'diff'; path: string; line?: number } | null>(null);
  // Active pane's working dir — roots the sidebar at the same place ridge shows.
  let activeCwd = $state('');
  // Provider rooted at the active cwd — backs the file/diff viewer (the sidebar
  // builds its own internally). Recreated when the cwd changes.
  const sidebarProvider = $derived(createWsSidebarProvider(activeCwd));

  function openFileViewer(path: string, line?: number) {
    viewer = { kind: 'file', path, line };
    sidebarTab = null; // close the sidebar so the viewer takes the screen
  }
  function openDiffViewer(path: string) {
    viewer = { kind: 'diff', path };
    sidebarTab = null;
  }
  // §remote 新建终端：空状态下让远程端自行创建终端，不再依赖桌面端先开一个。
  let creatingPane = $state(false);
  let createError = $state('');

  // §B-debounce: 防快速切 pane 打爆 DataChannel 的补偿定时器（见 §replay-backpressure）。
  let _paneSubDebounce: ReturnType<typeof setTimeout> | null = null;

  let canvasRef: ReturnType<typeof TerminalCanvasComponent> | undefined = $state();
  let showKeyboard = $state(true);          // virtual keyboard visible in header
  // Kernel palette derived from the desktop theme; applied to the canvas once it
  // mounts (the theme push usually arrives before the terminal exists).
  let kernelTheme: Record<string, string> | null = $state(null);
  let backendName = $state('Canvas2D');

  // §remember-last-pane / §persist-state: remember the last active pane per
  // workspace AND the last active workspace, persisted to localStorage so a
  // refresh restores the user's exact context (工作区 + pane) instead of forcing
  // a re-selection every time. sessionStorage holds the heavy scrollback; these
  // lightweight "which ws / which pane" pointers go to localStorage so they also
  // survive a tab close, not just a reload.
  const LS_WS_KEY = 'rg-remote-active-ws';
  const LS_PANEMAP_KEY = 'rg-remote-pane-map';

  function loadPaneMap(): Map<string, string> {
    try {
      const raw = localStorage.getItem(LS_PANEMAP_KEY);
      if (!raw) return new Map();
      return new Map(Object.entries(JSON.parse(raw) as Record<string, string>));
    } catch { return new Map(); }
  }
  function persistPaneMap(): void {
    try {
      localStorage.setItem(LS_PANEMAP_KEY, JSON.stringify(Object.fromEntries(lastActivePanePerWorkspace)));
    } catch { /* quota exceeded / disabled — ignore */ }
  }
  function persistActiveWs(id: string): void {
    try { if (id) localStorage.setItem(LS_WS_KEY, id); } catch { /* ignore */ }
  }

  const lastActivePanePerWorkspace = loadPaneMap();
  // The workspace the user last viewed. Read once at init; on boot we switch the
  // host back to it (if it's on a different one) so a refresh lands on the same
  // workspace. The host then broadcasts that workspace's panes and the panes
  // handler restores the remembered pane.
  let savedActiveWs: string | null = null;
  try { savedActiveWs = localStorage.getItem(LS_WS_KEY); } catch { /* ignore */ }
  // Boot workspace-restore runs exactly once (first workspaces list after connect).
  let bootRestoreDone = false;

  // §theme-persist: a control end owns its appearance (theme isolation). Once the
  // user cycles the theme, that choice must survive a reconnect (the host re-pushes
  // its OWN active theme at every connect) AND a reload. We remember the cycled
  // {id, colors} locally; on any later host theme push we re-apply the override
  // instead of the host's theme. localStorage makes it survive a reload too.
  const LS_THEME_KEY = 'rg-remote-theme-override';
  let userTheme: { id: string; colors: Record<string, string> } | null = null;
  try {
    const raw = localStorage.getItem(LS_THEME_KEY);
    if (raw) userTheme = JSON.parse(raw) as { id: string; colors: Record<string, string> };
  } catch { /* ignore */ }
  // True between tapping the theme button and its `theme` reply arriving, so the
  // reply is adopted as the override (vs. a host-initiated connect/reconnect push).
  let pendingCycle = false;
  function persistUserTheme() {
    try {
      if (userTheme) localStorage.setItem(LS_THEME_KEY, JSON.stringify(userTheme));
      else localStorage.removeItem(LS_THEME_KEY);
    } catch { /* quota / disabled — ignore */ }
  }

  // Theme cycling: ask the host for the theme *after* the one we currently show.
  // The host computes it statelessly (no disk write / no peer clobber — see
  // wsRemote.cycleTheme) and pushes it back via the 'theme' message. We cycle from
  // the user's override id when present so cycling stays continuous after a
  // reconnect (where ws.lastTheme() would be the host's theme, not ours).
  async function handleThemeToggle() {
    if (!ws) return;
    pendingCycle = true;
    ws.cycleTheme(userTheme?.id ?? ws.lastTheme()?.id ?? '');
  }

  // Paste the CONTROL DEVICE's clipboard (this phone/browser) into the remote
  // terminal as a bracketed paste. The button onclick is the user gesture the
  // Clipboard API requires, and the LAN/cloud link is a secure context, so
  // readText() is permitted. Previously this sent `{type:'paste'}` to the host,
  // which had no handler — so the button did nothing.
  async function handlePaste() {
    if (!activePaneId || !canvasRef) return;
    try {
      const text = await navigator.clipboard.readText();
      if (text) canvasRef.pasteText(text);
    } catch { /* clipboard blocked: no permission / insecure context */ }
  }

  // §history-pull（2026-07-02）: the host no longer dumps full scrollback on
  // subscribe — it seeds ~1.5 screens and we lazily page older history as the user
  // scrolls up. TerminalCanvas fires onNearTop when the viewport nears the buffer
  // top; fetch the next older batch (cloud link only) and prepend it. Guard against
  // a pane switch mid-fetch so we never prepend one pane's history onto another.
  async function loadOlderScrollback() {
    const pid = activePaneId;
    if (!pid || !canvasRef || !ws.fetchOlderScrollback) return;
    const older = await ws.fetchOlderScrollback(pid);
    if (older && older.length > 0 && activePaneId === pid) canvasRef.prependScrollback(older);
  }

  // §terminal-isolation + scrollback-cache: the local kernel is a single shared
  // instance, so switching panes MUST wipe it (resetForSwitch) — otherwise the
  // previous pane's scrollback bleeds into the new one (上滚串台). We also keep
  // each pane's raw byte stream so a switch repaints instantly from cache, and
  // mirror the active pane to sessionStorage so a reload restores instantly
  // before the host reconnects. The host re-sends ≤64KB scrollback on
  // (re)subscribe; we tail-match it against the cache to avoid double-painting.
  // §scrollback-cache: per-pane raw byte buffers + the prune (GC) and
  // replay-reconcile DECISIONS live in the pure PaneScrollbackCache module
  // (unit-tested without a host/DOM). This shell only drives its sessionStorage
  // mirroring by the id sets the prune methods return.
  const SS_CAP = 48 * 1024;
  const paneCache = new PaneScrollbackCache(PANE_BUF_CAP);
  let subscribedPaneId: string | null = null;
  let expectReplayPane: string | null = null;
  let ssMirrorTimer: ReturnType<typeof setTimeout> | null = null;

  const SB_KEY_PREFIX = 'rg-remote-sb:';
  function ssKey(id: string) { return `${SB_KEY_PREFIX}${id}`; }
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
  function loadPaneFromSession(id: string): Uint8Array | null {
    try { const s = sessionStorage.getItem(ssKey(id)); return s ? b64ToBytes(s) : null; }
    catch { return null; }
  }
  function scheduleSessionMirror(id: string) {
    if (ssMirrorTimer) return;
    ssMirrorTimer = setTimeout(() => {
      ssMirrorTimer = null;
      const buf = paneCache.get(id);
      if (!buf) return;
      try {
        const tail = buf.length > SS_CAP ? buf.subarray(buf.length - SS_CAP) : buf;
        sessionStorage.setItem(ssKey(id), bytesToB64(tail));
      } catch { /* quota exceeded / disabled — ignore */ }
    }, 600);
  }
  function removeSessionMirror(id: string) {
    try { sessionStorage.removeItem(ssKey(id)); } catch { /* disabled — ignore */ }
  }

  // §cache-gc: a closed pane MUST release its caches. The PWA tab can live for
  // days (长期运行/长时间后台), so without this every terminal ever opened leaks
  // its scrollback into both the in-memory buffer map (≤256KB each) AND
  // sessionStorage (≤48KB each), plus the WS text buffer — eventually blowing the
  // mobile tab's memory budget / sessionStorage quota, so the page fails to
  // (re)open until the user clears site data.
  //
  // §cross-ws-prune（方案1，子方案 B）: the host's `list-panes` only returns the
  // ACTIVE workspace's panes, so the old "delete everything not in this list"
  // wrongly GC'd OTHER workspaces' caches the moment you switched workspace →
  // switching back lost scrollback. We now release ONLY panes that vanished from
  // their OWN workspace's list (truly closed — mobile can only close panes in the
  // active workspace), keeping cross-workspace caches alive. The host re-broadcasts
  // the list on every pane add/close/rename. `survivingIds` spans all workspaces so
  // ws.pruneOutputs (same "not in set → delete" semantics) doesn't over-prune either.
  function pruneDeadPanes(activeWsId: string, liveIds: string[]) {
    const { survivingIds } = paneCache.pruneCurrentWorkspace(activeWsId, liveIds);
    // Mirror the in-memory GC to sessionStorage: drop any mirror whose pane is no
    // longer cached (survivingIds is the authoritative kept set across all ws).
    const survive = new Set(survivingIds);
    try {
      const stale: string[] = [];
      for (let i = 0; i < sessionStorage.length; i++) {
        const k = sessionStorage.key(i);
        if (k && k.startsWith(SB_KEY_PREFIX) && !survive.has(k.slice(SB_KEY_PREFIX.length))) {
          stale.push(k);
        }
      }
      for (const k of stale) sessionStorage.removeItem(k);
    } catch { /* sessionStorage disabled — nothing to prune */ }
    ws.pruneOutputs(survive);
  }

  // §cross-ws-prune fallback（方案1）: when a whole workspace is closed (its id
  // drops from list-workspaces), its panes can never reappear, so release their
  // caches here — the per-list prune above never sees those panes again. Clears
  // both the in-memory buffer and its sessionStorage mirror, and keeps the WS
  // text buffers (pruneOutputs) in step with the surviving cached set.
  function pruneCachesForClosedWorkspaces(liveWorkspaceIds: string[]) {
    const removed = paneCache.pruneClosedWorkspaces(liveWorkspaceIds);
    if (removed.length > 0) {
      for (const id of removed) removeSessionMirror(id);
      ws.pruneOutputs(new Set(paneCache.liveIds()));
    }
  }

  // Defensive: the host's pane/workspace lists can briefly contain DUPLICATE ids
  // — e.g. a pane present in both `terminals` and `pending_spawns` during a spawn
  // (src-tauri/.../server.rs builds the list from both) — which makes Svelte's
  // keyed {#each (id)} throw `each_key_duplicate` and corrupt the rendered tree
  // (wrong row reused → close/switch acts on the wrong pane). Dedupe by id before
  // rendering so the UI stays correct regardless of what the host sends.
  function dedupeById<T extends { id: string }>(items: T[]): T[] {
    const seen = new Set<string>();
    return items.filter((it) => (seen.has(it.id) ? false : (seen.add(it.id), true)));
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
      workspaces = dedupeById(data.workspaces || []);
      // §cross-ws-prune fallback: drop caches of any workspace that's gone.
      pruneCachesForClosedWorkspaces(workspaces.map(w => w.id));
      const hostActive = workspaces.find(w => w.active);
      // §persist-state: on the first list after (re)connect, if the user's last
      // viewed workspace still exists but the host is on a different one, switch
      // the host back so a refresh lands on the same workspace (the host then
      // broadcasts that workspace's panes, and the panes handler restores the
      // remembered pane). Runs once; afterwards we just track the host's active.
      if (!bootRestoreDone) {
        bootRestoreDone = true;
        if (savedActiveWs && savedActiveWs !== (hostActive?.id ?? '')
            && workspaces.some(w => w.id === savedActiveWs)) {
          activeWorkspaceId = savedActiveWs;
          activePaneId = null; // force the panes handler to re-pick for the restored ws
          const ok = await ws.switchWorkspace(savedActiveWs);
          if (ok) ws.listPanes();
          // Re-read so the `active` flag reflects the switch.
          const after = dedupeById((await ws.listWorkspaces()).workspaces || []);
          workspaces = after;
          const a2 = after.find(w => w.active);
          activeWorkspaceId = a2 ? a2.id : savedActiveWs;
          return;
        }
      }
      if (hostActive) activeWorkspaceId = hostActive.id;
    } catch { /* ignore */ }
  }

  // §fail-grading 处置（任务 A 问题1）。
  // 通道异常（信令/WebRTC/网络/并发超限）→ 全量重连：reload 让 App.svelte 重新走
  // boot/gate（cloud boot 单例幂等；LAN autoReconnect 用持久化 token 重连），比在 banner
  // 里手搓一套重连状态机更稳，且复用本仓库既有「reload 即重连」模式。
  function handleRetry() {
    try { ws.disconnect(); } catch { /* already torn down */ }
    location.reload();
  }
  // 用户问题（账户/权限不匹配）或设备停用 → 退回登录态：清掉本端持久化的远控 token，
  // reload 后 App.svelte 会落到 AuthScreen（LAN 无 token→手动输码；cloud 会话失效→boot
  // 重定向到主域登录）。这就是本仓库现有的「回登录」路径（AuthScreen.fallbackToManual /
  // CloudAuthScreen 的 location.replace 同源）。
  function handleBackToLogin() {
    try { ws.disconnect(); } catch { /* already torn down */ }
    try { localStorage.removeItem('ridge_remote_token'); } catch { /* ignore */ }
    location.reload();
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
    // §realtime-status（任务 A 问题3）：先装状态监听，再同步一次真实连接态。云端进入
    // MainApp 时传输早已 'connected'，若不同步则 wsState 停在初值 'disconnected'，顶部
    // 误显示「重连中」直到下一次状态事件才纠正。装监听在先、同步在后，保证此刻起的每次
    // 连接事件都不漏。
    ws.onStateChange((s) => {
      wsState = s;
      failure = ws.lastFailure();
    });
    wsState = ws.state();
    failure = ws.lastFailure();
    ws.onMessage((msg) => {
      if (msg.type === 'panes') {
        panes = dedupeById(msg.panes);
        const paneIds = panes.map(p => p.id);
        // Release caches for panes truly closed in THIS workspace (memory/quota
        // leak); other workspaces' caches survive (§cross-ws-prune). The list
        // belongs to the active workspace; skip pruning until we know which one
        // (an empty id would mis-tag every pane).
        if (activeWorkspaceId) pruneDeadPanes(activeWorkspaceId, paneIds);
        // §persist-state pane restore: keep a still-valid current selection
        // (no "莫名奇妙切换工作区"); otherwise prefer the remembered pane for the
        // current workspace (seeded from localStorage on boot), else the first
        // pane. Re-picking when the current id went stale — e.g. right after a
        // workspace switch — is what lets a refresh land back on the remembered
        // pane instead of a dead id.
        if (activePaneId && paneIds.includes(activePaneId)) {
          // current selection still valid — leave it untouched
        } else {
          const remembered = activeWorkspaceId
            ? lastActivePanePerWorkspace.get(activeWorkspaceId)
            : undefined;
          if (remembered && paneIds.includes(remembered)) {
            activePaneId = remembered;
          } else if (paneIds.length > 0) {
            activePaneId = panes[0].id;
          } else {
            activePaneId = null;
          }
        }
      }
      if (msg.type === 'workspaces') {
        workspaces = dedupeById(msg.workspaces);
        // §cross-ws-prune fallback: a closed workspace's panes can never come
        // back — release their caches so they don't leak (per-list prune never
        // sees them again).
        pruneCachesForClosedWorkspaces(workspaces.map(w => w.id));
        const active = workspaces.find(w => w.active);
        // Once the boot restore has run, follow the host's active workspace.
        // Before that, refreshWorkspaces() owns the restore decision, so a
        // proactive push must not clobber the workspace we're about to restore.
        if (active && bootRestoreDone) activeWorkspaceId = active.id;
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
        // replay (≤64KB tail). §no-shrink（方案2）: reconcile it against the local
        // cache. 'keep' = our cache tail-matches OR is LONGER than the replay →
        // we already pre-painted it; drop the replay so the host's 64KB tail can't
        // overwrite/shrink our ≤256KB history. 'repaint' = no cache, or the pane
        // changed (cache shorter & no tail-match) → wipe + repaint authoritatively.
        expectReplayPane = null;
        const r = paneCache.reconcileReplay(paneId, data, activeWorkspaceId || undefined);
        if (r.action === 'keep') return;
        canvasRef?.resetForSwitch();
        canvasRef?.feedUtf8(r.buffer);
        scheduleSessionMirror(paneId);
        return;
      }
      // Live output.
      paneCache.append(paneId, data, activeWorkspaceId || undefined);
      canvasRef?.feedUtf8(data);
      scheduleSessionMirror(paneId);
    });
    ws.onMetadata((paneId, title, cwd) => {
      // §realtime-title: reflect the live pane title in the workspace tree (and
      // header) the instant it changes, instead of waiting for the next
      // list-panes round-trip. pty-meta only fires for the active workspace's
      // panes (host filters by active_ws_id); non-active workspaces refresh via
      // the tree's periodic poll.
      if (title != null && title.length > 0) {
        panes = panes.map((p) => (p.id === paneId ? { ...p, title } : p));
      }
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
    // arrives before this listener) — but a user override (§theme-persist) wins.
    const t0 = ws.lastTheme();
    if (userTheme) applyTheme(userTheme.colors);
    else if (t0) applyTheme(t0.colors);
    ws.onTheme((colors) => {
      if (pendingCycle) {
        // Reply to our own cycle tap → adopt it as the persisted override.
        pendingCycle = false;
        const id = ws.lastTheme()?.id ?? '';
        userTheme = id ? { id, colors } : null;
        persistUserTheme();
        applyTheme(colors);
      } else if (userTheme) {
        // Host (re)pushed its active theme at (re)connect, but the user has an
        // override → keep the override so the cycled theme survives reconnects.
        applyTheme(userTheme.colors);
      } else {
        applyTheme(colors);
      }
    });
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
        const cached = paneCache.get(pid);
        if (cached && cached.length > 0) canvasRef?.feedUtf8(cached);
        expectReplayPane = pid;
        ws.subscribePane(pid);
        // The new server socket has no knowledge of our viewport size.
        // Claim it immediately so the PTY is reflowed and the terminal
        // doesn't stay stuck at the 80x24 default.
        const d = canvasRef?.getDims();
        if (d) ws.claimPane(pid, d.rows, d.cols, d.pixelWidth, d.pixelHeight);
      }
      ws.listPanes();
      refreshWorkspaces();
      // Reset stale-guard seq to 0 so the debounced refreshActivePane
      // below can actually send — on reconnect no new claimPane has been
      // issued yet, so the guard cur <= _refreshSeq would otherwise
      // match and silently block the re-subscribe PTY resize (#B3).
      _refreshSeq = -1;
      refreshActivePane();
    });
    // §persist-state: seed the active workspace from localStorage before the
    // first panes/workspaces arrive so the panes handler can restore the
    // remembered pane immediately; refreshWorkspaces() then switches the host
    // back to this workspace if it's currently on a different one.
    if (savedActiveWs) activeWorkspaceId = savedActiveWs;
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
      // Remember this pane as the last active for the current workspace, and
      // persist it so a refresh restores the same ws + pane (§persist-state).
      if (activeWorkspaceId) {
        lastActivePanePerWorkspace.set(activeWorkspaceId, pid);
        persistPaneMap();
        persistActiveWs(activeWorkspaceId);
      }
      // §isolation: wipe the kernel so the previous pane can't bleed into this one.
      canvasRef?.resetForSwitch();
      // Instant pre-paint from cache (in-memory; else sessionStorage on reload).
      let cached = paneCache.get(pid);
      if (!cached) {
        const restored = loadPaneFromSession(pid);
        if (restored) { paneCache.set(pid, restored, activeWorkspaceId || undefined); cached = restored; }
      }
      if (cached && cached.length > 0) canvasRef?.feedUtf8(cached);
      // The host replays this pane's scrollback on subscribe — reconcile it
      // against the cache in onRawBytes to avoid double-painting.
      // §B-debounce: 防快速切换 pane 连发多次未截流的 replay_pane_scrollback_raw（256 KiB）
      // 打爆 DataChannel 缓冲区（8 MiB BUFFERED_HIGH_WATERMARK）→ 断连。
      // 只对"最终落脚"的 pane 发 subscribePane：150ms 内若 activePaneId 已变则取消。
      if (_paneSubDebounce !== null) clearTimeout(_paneSubDebounce);
      expectReplayPane = pid;
      _paneSubDebounce = setTimeout(() => {
        _paneSubDebounce = null;
        if (activePaneId === pid) ws.subscribePane(pid);
      }, 150);
    });
  });

  $effect(() => {
    if (activePaneId && canvasRef) {
      refreshActivePane();
    }
  });

  // §persist-state: save the active workspace whenever it changes (the pane map
  // is saved on pane switch above) so a refresh restores the user's context.
  $effect(() => {
    if (activeWorkspaceId) persistActiveWs(activeWorkspaceId);
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
  {#if wsState !== 'connected'}
    <!-- §断连提示 + §fail-grading（任务 A 问题1）: live link status.
         - 非 error（disconnected/connecting）: 传输在自动重连 → 「重连中」，不阻断。
         - error + user/parked: 不可重试的终态 → 标红，给「退回登录」动作（user 换凭据 /
           parked 设备停用需去控制台启用或升级），绝不再无限 pending。
         - error + channel（含无分级兜底）: 通道异常（信令/WebRTC/网络/并发超限）→ 标红，
           给「重试」动作，让用户主动全量重连而不是一直转圈。 -->
    {#if wsState === 'error'}
      <div class="conn-banner lost">
        {#if failure?.category === 'user'}
          <span class="conn-msg">{$t('mobile.connectFail')}</span>
          <button class="conn-action" onclick={handleBackToLogin}>{$t('mobile.verifyAndConnect')}</button>
        {:else if failure?.category === 'parked'}
          <span class="conn-msg">{$t('mobile.connectionLost')}</span>
          <button class="conn-action" onclick={handleBackToLogin}>{$t('mobile.refresh')}</button>
        {:else}
          <!-- channel 异常 -->
          <span class="conn-msg">{$t('mobile.connectionLost')}</span>
          <button class="conn-action" onclick={handleRetry}>{$t('mobile.refresh')}</button>
        {/if}
      </div>
    {:else}
      <div class="conn-banner">
        {$t('mobile.reconnecting')}
      </div>
    {/if}
  {/if}
  {#if panes.length === 0}
    <div class="empty">
      <p>{$t('mobile.noActiveTerminal')}</p>
      <button class="create-btn" onclick={handleCreatePane} disabled={creatingPane}>
        {creatingPane ? $t('mobile.creating') : $t('mobile.newTerminal')}
      </button>
      {#if createError}<p class="create-error">{createError}</p>{/if}
    </div>
  {:else if activePaneId}
    <header class="mobile-header">
      <div class="header-row">
        <div class="header-nav">
          <button class="hdr-btn" class:active={sidebarTab === 'files'} onclick={() => handleSidebarToggle('files')} title={$t('mobile.filesTitle')} tabindex="-1">
            <Folder class="w-4 h-4" />
          </button>
          <button class="hdr-btn" class:active={sidebarTab === 'git'} onclick={() => handleSidebarToggle('git')} title="Git" tabindex="-1">
            <GitBranch class="w-4 h-4" />
          </button>
          <button class="hdr-btn" class:active={sidebarTab === 'search'} onclick={() => handleSidebarToggle('search')} title={$t('mobile.searchTitle')} tabindex="-1">
            <Search class="w-4 h-4" />
          </button>
        </div>
        <div class="header-breadcrumb">
          {#if activePaneId}
            <span class="breadcrumb-text">{activePane?.title || $t('mobile.terminalDefault')}</span>
            <span class="status-dot" class:connected={wsState === 'connected'} class:connecting={wsState === 'connecting'}></span>
          {/if}
        </div>
        <div class="header-actions">
          <button class="hdr-btn" class:active={showKeyboard} onclick={() => showKeyboard = !showKeyboard} title={$t('mobile.virtualKeyboard')} tabindex="-1">
            <Keyboard class="w-4 h-4" />
          </button>
        </div>
      </div>
      {#if showKeyboard}
        <div class="vk-section">
          {#await VirtualKeyboard}
            <div class="vk-loading">{$t('mobile.initializingTerminal')}</div>
          {:then module}
            <module.default onKey={(k: string, c: boolean, a: boolean, s: boolean) => canvasRef?.handleVirtualKey(k, c, a, s)} />
          {/await}
        </div>
      {/if}
    </header>

    {#await TerminalCanvas}
      <div class="terminal-loading">{$t('mobile.initializingTerminal')}</div>
    {:then module}
      <module.default
        bind:this={canvasRef}
        bind:backendName
        paneId={activePaneId ?? null}
        {onStdin}
        {onResize}
        onHostClipboard={(text) => ws.setHostClipboard(text)}
        onNearTop={loadOlderScrollback}
        bind:selectionMode
      />
    {/await}
  {/if}

  {#if sidebarTab !== null}
    <div class="sidebar-overlay" onclick={() => sidebarTab = null} role="presentation"></div>
    {#await RemoteSidebar}
      <div class="sidebar-loading">{$t('mobile.loading')}</div>
    {:then module}
      <module.default
        tab={sidebarTab}
        cwd={activeCwd}
        onClose={() => sidebarTab = null}
        onTabChange={(t) => sidebarTab = t}
        onOpenFile={openFileViewer}
        onOpenDiff={openDiffViewer}
      />
    {/await}
  {/if}

  {#if viewer}
    {@const v = viewer}
    {#await FileViewer then module}
      <module.default
        provider={sidebarProvider}
        kind={v.kind}
        path={v.path}
        line={v.line}
        onClose={() => viewer = null}
      />
    {/await}
  {/if}

  <BottomTabBar
    {ws}
    {backendName}
    onRefresh={handleRefresh}
    onPaste={handlePaste}
    onThemeToggle={handleThemeToggle}
    bind:selectionMode
    {panes}
    bind:activePaneId
    {workspaces}
    bind:activeWorkspaceId
    onWorkspacesChanged={refreshWorkspaces}
  />
</div>

<style>
  .app-root{position:fixed;inset:0;display:flex;flex-direction:column;background:var(--rg-bg);color:var(--rg-fg)}
  .conn-banner{flex-shrink:0;padding:6px 12px;text-align:center;font-size:12px;font-weight:600;color:#fff;background:var(--rg-ansi-yellow,#bb8009);z-index:50;display:flex;align-items:center;justify-content:center;gap:10px}
  .conn-banner.lost{background:var(--rg-ansi-red,#cf222e)}
  .conn-msg{flex:0 1 auto}
  .conn-action{flex-shrink:0;border:1px solid rgba(255,255,255,.7);background:rgba(255,255,255,.15);color:#fff;font-size:12px;font-weight:600;border-radius:6px;padding:3px 10px;cursor:pointer}
  .conn-action:hover{background:rgba(255,255,255,.28)}
  .empty{flex:1;display:flex;flex-direction:column;align-items:center;justify-content:center;color:var(--rg-fg-muted);gap:12px}
  .create-btn{padding:8px 20px;border:1px solid var(--rg-accent);border-radius:8px;background:color-mix(in srgb,var(--rg-accent) 14%,transparent);color:var(--rg-fg);font-size:14px;font-weight:600;cursor:pointer;transition:all .15s}
  .create-btn:active{background:color-mix(in srgb,var(--rg-accent) 26%,transparent)}
  .create-btn:disabled{opacity:.5;cursor:not-allowed}
  .create-error{font-size:12px;color:var(--rg-ansi-red)}
  .sidebar-overlay{position:fixed;inset:0;background:rgba(0,0,0,0.5);z-index:40;touch-action:none}
  .mobile-header{position:sticky;top:0;display:flex;flex-direction:column;padding:env(safe-area-inset-top) 0 0 0;background:var(--rg-bg);border-bottom:1px solid color-mix(in srgb,var(--rg-fg) 12%,transparent);z-index:30;min-height:calc(44px + env(safe-area-inset-top))}
  .header-row{display:flex;align-items:center;height:44px;padding:0 8px;gap:4px}
  .header-nav{display:flex;gap:2px}
  .header-breadcrumb{flex:1;display:flex;align-items:center;justify-content:center;gap:6px;min-width:0;overflow:hidden}
  .breadcrumb-text{font-size:13px;color:var(--rg-fg-muted);white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
  .header-actions{display:flex;gap:2px}
  .hdr-btn{display:flex;align-items:center;justify-content:center;width:36px;height:36px;border:none;border-radius:8px;background:transparent;color:var(--rg-fg-muted);cursor:pointer;transition:all .15s}
  .hdr-btn:active{background:color-mix(in srgb,var(--rg-fg) 10%,transparent);color:var(--rg-fg)}
  .hdr-btn.active{color:var(--rg-accent)}
  .hdr-btn :global(svg){width:18px;height:18px}
  .vk-section{overflow:hidden;border-top:1px solid color-mix(in srgb,var(--rg-fg) 8%,transparent)}
  .status-dot{width:8px;height:8px;border-radius:50%;background:var(--rg-fg-muted);flex-shrink:0}
  .status-dot.connected{background:var(--rg-ansi-green)}
  .status-dot.connecting{background:var(--rg-ansi-yellow)}
</style>