<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import { t, tr } from '$lib/i18n';
  import { Folder, GitBranch, Search, Users, Keyboard } from 'lucide-svelte';
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
  import {
    getRemotePanelAvailability,
    type RemoteLink,
    type RemotePanel,
    type PaneInfo,
    type ConnectionState,
    type WorkspaceInfo,
    type ConnectionFailure,
  } from '@ridge/remote';
  import { applyThemeVars, buildKernelTheme } from './lib/theme';
  import { createWsSidebarProvider } from './lib/sidebarProvider';

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
  // 句级输入缓冲（语音听写友好）——localStorage 持久，TabBar 切换。
  const LS_SBUF_KEY = 'rg-remote-sentence-buffer';
  let sentenceBuffer = $state(((): boolean => {
    try { return localStorage.getItem(LS_SBUF_KEY) === '1'; } catch { return false; }
  })());
  $effect(() => {
    try { localStorage.setItem(LS_SBUF_KEY, sentenceBuffer ? '1' : '0'); } catch { /* quota */ }
  });
  let sidebarTab: RemotePanel | null = $state(null);
  // Optimistic until D9 hello completes; onMount immediately replaces these via
  // refreshCapabilities and subscribes to reconnect renegotiation.
  let panelAvailability = $state<Readonly<Record<RemotePanel, boolean>>>({
    files: true,
    git: true,
    search: true,
    team: false,
  });
  let canManageWorkspaces = $state(true);
  let canUseTheme = $state(true);
  // Read-only file / git-diff viewer overlay. Opened from the sidebar (tap a
  // file in the tree / a search hit → 'file'; tap a changed file in git → 'diff').
  let viewer = $state<{ kind: 'file' | 'diff'; path: string; line?: number } | null>(null);
  // Active pane's working dir — roots the sidebar at the same place ridge shows.
  let activeCwd = $state('');
  // Provider rooted at the active cwd — backs the file/diff viewer (the sidebar
  // builds its own internally). Recreated when the cwd changes.
  const sidebarProvider = $derived(createWsSidebarProvider(activeCwd));

  // iter-60 G10（导航栈语义，取代旧 §close-to-terminal 一刀切）：关闭 viewer 回
  // 「打开它的那一级」——从侧栏（文件树/git/搜索）打开的，关闭回该侧栏页；从终端
  // 直接打开的（sidebarTab 本为 null，如终端链接），关闭回终端。两代诉求兼得：
  // 旧 bug「关文件误回目录」只出现在终端来源场景，栈语义下同样成立。
  let viewerReturnTab: RemotePanel | null = null;
  function openFileViewer(path: string, line?: number) {
    if (!panelAvailability.files) return;
    viewerReturnTab = sidebarTab;
    viewer = { kind: 'file', path, line };
    sidebarTab = null; // close the sidebar so the viewer takes the screen
  }
  function openDiffViewer(path: string) {
    if (!panelAvailability.git) return;
    viewerReturnTab = sidebarTab;
    viewer = { kind: 'diff', path };
    sidebarTab = null;
  }
  function closeViewer() {
    viewer = null;
    sidebarTab = viewerReturnTab;
    viewerReturnTab = null;
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
    if (!canUseTheme) return;
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

  // §keep-alive (P4, 2026-07-25): the per-pane raw-byte cache + sessionStorage
  // mirror + replay-reconcile are RETIRED. Each pane now has its own keep-alive
  // kernel in the shared TerminalManager (held by the pane's TerminalCanvas,
  // parked — not wiped — on switch), so pane history lives in the kernel and
  // switching back shows it instantly with no wipe / no cache / no reconcile.
  // We keep only the lightweight "which pane is subscribed" pointer and a
  // workspace-tagged set of attached panes so we can DETACH (free the kernel)
  // when the host truly closes a pane / workspace.
  let subscribedPaneId: string | null = null;

  // Workspace each attached (visited) pane belongs to. Mirrors the retired
  // paneCache cross-ws prune: release only panes that vanished from THEIR OWN
  // workspace's list (mobile can only close panes in the active workspace), so a
  // cross-workspace switch-back keeps the other workspaces' kernels alive.
  const attachedPaneWs = new Map<string, string>();

  // §keep-alive resume: panes whose mirror kernel has ALREADY received its full
  // RIS resync (scrollback + mode reattach) this session and is in-sync. On a
  // switch-BACK to such a pane we resubscribe with `{ resume: true }` so the host
  // skips the RIS resync (which would wipe the surviving keep-alive kernel down to
  // the tail — the RIS-vs-keep-alive conflict). Fresh panes (not in the set) get a
  // full resync. Cleared on reconnect (a disconnect leaves every mirror gapped →
  // force a full resync); a truly-closed pane is removed when its kernel is detached.
  const replayedPanes = new Set<string>();

  // Free the kernels of truly-closed panes. Dynamic import keeps the (large)
  // manager out of the mobile entry bundle — the lazy TerminalCanvas already
  // loaded it by the time any pane exists, so this resolves instantly.
  async function detachPaneKernels(ids: string[]) {
    if (ids.length === 0) return;
    // A truly-closed pane can never resume — drop it from the resume set so a later
    // pane reusing the same id (unlikely, but ids are host-assigned) starts fresh.
    for (const id of ids) replayedPanes.delete(id);
    try {
      const { TerminalManager } = await import('@ridge/remote/shared/terminal/manager');
      const mgr = TerminalManager.tryInstance();
      if (!mgr) return;
      for (const id of ids) mgr.detach(id);
    } catch { /* manager not loaded / already torn down */ }
  }

  // A fresh `panes` list arrived for the CURRENT active workspace. Panes tagged
  // to `activeWsId` that vanished from it were truly closed → detach their
  // kernels (and drop the host-side output buffer). Other workspaces' kernels
  // are untouched (§cross-ws-prune parity).
  function pruneDeadPanes(activeWsId: string, liveIds: string[]) {
    const live = new Set(liveIds);
    const dead: string[] = [];
    for (const [id, ws2] of attachedPaneWs) {
      if (ws2 === activeWsId && !live.has(id)) { dead.push(id); attachedPaneWs.delete(id); }
    }
    // (Re)tag every live pane of this workspace so GC works even if a pane was
    // activated before activeWorkspaceId was known. detach() no-ops on panes
    // whose kernel was never actually attached, so over-tagging is harmless.
    for (const id of liveIds) attachedPaneWs.set(id, activeWsId);
    if (dead.length > 0) {
      void detachPaneKernels(dead);
      ws.pruneOutputs(new Set([...attachedPaneWs.keys()]));
    }
  }

  // A whole workspace closed (its id dropped from list-workspaces): its panes
  // can never reappear, so free their kernels here (the per-list prune above
  // never sees those panes again).
  function pruneCachesForClosedWorkspaces(liveWorkspaceIds: string[]) {
    const liveWs = new Set(liveWorkspaceIds);
    const dead: string[] = [];
    for (const [id, ws2] of attachedPaneWs) {
      if (!liveWs.has(ws2)) { dead.push(id); attachedPaneWs.delete(id); }
    }
    if (dead.length > 0) {
      void detachPaneKernels(dead);
      ws.pruneOutputs(new Set([...attachedPaneWs.keys()]));
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

  // Compact a cwd for the header sub-line: keep the last two path segments so the
  // meaningful tail (repo/dir) is visible on a narrow phone header; the full path
  // is on the title attribute for a long-press tooltip.
  function compactCwd(cwd: string): string {
    const parts = cwd.split(/[/\\]+/).filter(Boolean);
    if (parts.length <= 2) return cwd;
    return '…/' + parts.slice(-2).join('/');
  }

  function handleSidebarToggle(tab: RemotePanel) {
    if (!panelAvailability[tab]) return;
    if (sidebarTab === tab) {
      sidebarTab = null;
    } else {
      sidebarTab = tab;
    }
  }

  function selectSidebarTab(tab: RemotePanel) {
    if (panelAvailability[tab]) sidebarTab = tab;
  }

  function refreshCapabilities() {
    panelAvailability = getRemotePanelAvailability((capability) => ws.hasCapability(capability));
    canManageWorkspaces = ws.hasCapability('workspace');
    canUseTheme = ws.hasCapability('theme');
    if (sidebarTab && !panelAvailability[sidebarTab]) sidebarTab = null;
    if (viewer?.kind === 'diff' && !panelAvailability.git) viewer = null;
    if (viewer?.kind === 'file' && !panelAvailability.files) viewer = null;
  }

  // 远程端自建终端：请求 host 创建 pane，成功后刷新列表并把新 pane 设为活动项
  // （onMessage 的 'panes' 分支会在 listPanes 回包后把 activePaneId 兜底为首个，
  //  这里显式置为新 id 以确保即使有多个 pane 也聚焦到刚建的那个）。失败时把错误
  // 文案显示给用户，绝不静默吞掉。
  async function handleCreatePane() {
    if (creatingPane || !canManageWorkspaces) return;
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
    const stopCapabilities = ws.onCapabilitiesChanged(refreshCapabilities);
    refreshCapabilities();
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
      // §keep-alive (P4): feed the frame into its pane's alive kernel via the
      // active pane's TerminalCanvas. The host single-subscribes so this is the
      // active pane; a straggler for a just-unsubscribed pane is dropped (it
      // isn't the mounted TerminalCanvas). No cache, no reconcile, no wipe — the
      // host's on-subscribe replay is absorbed by the alive kernel.
      if (paneId === activePaneId) canvasRef?.feedUtf8(data);
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
      // iter-60 G9: mirror cwd into the pane list too, so the pane-switcher
      // popup shows live per-pane cwd（此前只有 active pane 的侧栏根会更新）.
      if (cwd != null && cwd.length > 0) {
        panes = panes.map((p) => (p.id === paneId ? { ...p, cwd } : p));
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
    // Reconnect resync (R7): a reconnect opens a brand-new host socket that
    // holds no pane subscription. §keep-alive (P4): the local kernel stays ALIVE
    // (no wipe) — we just re-subscribe the active pane so the host resumes
    // streaming into it, and re-claim the viewport size so the PTY isn't stuck
    // at the 80x24 default. The host's replay is absorbed by the alive kernel.
    ws.onReconnect(() => {
      const pid = activePaneId;
      // §keep-alive after reconnect: a disconnect leaves a gap in every mirror kernel,
      // so force a full RIS resync on the next visit to each pane (clear the replayed
      // set). The active pane is full-resynced now (subscribePane below, resume=false).
      replayedPanes.clear();
      if (pid) {
        ws.subscribePane(pid);
        replayedPanes.add(pid);
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
    return () => {
      stopCapabilities();
      ws.disconnect();
    };
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
        // Track for keep-alive GC: this pane's kernel is (being) attached under
        // the active workspace; released only when the host closes it.
        attachedPaneWs.set(pid, activeWorkspaceId);
      }
      // §keep-alive (P4): NO resetForSwitch / no cache pre-paint. The pane's
      // kernel stays alive across switches (its TerminalCanvas parks it, not
      // wiped), so switching back shows content instantly with zero white-screen
      // and full scrollback. We only (debounced) re-subscribe so the host
      // resumes streaming THIS pane; the host's on-subscribe replay is absorbed
      // by the alive kernel.
      // §B-debounce: 防快速切换 pane 连发多次未截流的 replay_pane_scrollback_raw（256 KiB）
      // 打爆 DataChannel 缓冲区（8 MiB BUFFERED_HIGH_WATERMARK）→ 断连。
      // 只对"最终落脚"的 pane 发 subscribePane：150ms 内若 activePaneId 已变则取消。
      if (_paneSubDebounce !== null) clearTimeout(_paneSubDebounce);
      _paneSubDebounce = setTimeout(() => {
        _paneSubDebounce = null;
        if (activePaneId === pid) {
          // §keep-alive resume: a pane we've already resynced this session keeps its
          // alive kernel → resubscribe as resume (host skips the RIS resync that would
          // wipe it). First view → full resync, then mark it replayed.
          const resume = replayedPanes.has(pid);
          ws.subscribePane(pid, { resume });
          replayedPanes.add(pid);
        }
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

  // §header-pin（虚拟键盘顶吸）: keep the header (which holds the virtual keyboard)
  // glued to the VISIBLE viewport top so the soft IME can't push it off-screen /
  // let it cover content. When a mobile browser scrolls the layout up to reveal the
  // focused input above the IME, `visualViewport.offsetTop` grows by that scroll
  // amount; translating the header DOWN by the same amount re-pins it to the visible
  // top. When the browser keeps `position:fixed` still (offsetTop stays 0) this is a
  // no-op, so it's safe on every viewport model. Bounded by the scroll distance.
  let headerShift = $state(0);
  $effect(() => {
    const vv = window.visualViewport;
    if (!vv) return;
    const update = () => { headerShift = Math.max(0, Math.round(vv.offsetTop)); };
    vv.addEventListener('resize', update);
    vv.addEventListener('scroll', update);
    update();
    return () => {
      vv.removeEventListener('resize', update);
      vv.removeEventListener('scroll', update);
    };
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
      {#if canManageWorkspaces}
        <button class="create-btn" onclick={handleCreatePane} disabled={creatingPane}>
          {creatingPane ? $t('mobile.creating') : $t('mobile.newTerminal')}
        </button>
      {/if}
      {#if createError}<p class="create-error">{createError}</p>{/if}
    </div>
  {:else if activePaneId}
    <header class="mobile-header" style="transform: translateY({headerShift}px)">
      <div class="header-row">
        <div class="header-nav">
          {#if panelAvailability.files}
            <button class="hdr-btn" class:active={sidebarTab === 'files'} onclick={() => handleSidebarToggle('files')} title={$t('mobile.filesTitle')} tabindex="-1">
              <Folder class="w-4 h-4" />
            </button>
          {/if}
          {#if panelAvailability.git}
            <button class="hdr-btn" class:active={sidebarTab === 'git'} onclick={() => handleSidebarToggle('git')} title="Git" tabindex="-1">
              <GitBranch class="w-4 h-4" />
            </button>
          {/if}
          {#if panelAvailability.search}
            <button class="hdr-btn" class:active={sidebarTab === 'search'} onclick={() => handleSidebarToggle('search')} title={$t('mobile.searchTitle')} tabindex="-1">
              <Search class="w-4 h-4" />
            </button>
          {/if}
          {#if panelAvailability.team}
            <button class="hdr-btn" class:active={sidebarTab === 'team'} onclick={() => handleSidebarToggle('team')} title="Team" tabindex="-1">
              <Users class="w-4 h-4" />
            </button>
          {/if}
        </div>
        <div class="header-breadcrumb">
          {#if activePaneId}
            <div class="breadcrumb-line">
              <span class="breadcrumb-text">{activePane?.title || $t('mobile.terminalDefault')}</span>
              <span class="status-dot" class:connected={wsState === 'connected'} class:connecting={wsState === 'connecting'}></span>
            </div>
            {#if activeCwd}
              <span class="breadcrumb-cwd" title={activeCwd}>{compactCwd(activeCwd)}</span>
            {/if}
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
      <!-- §keep-alive (P4): key on activePaneId so switching panes REMOUNTS the
           input surface (onMount attach/unpark, onDestroy park) — mirroring the
           desktop RidgePane mount/unmount → attach/park lifecycle. The pane's
           kernel survives the remount (parked), so no wipe / no white-screen. -->
      {#key activePaneId}
        <module.default
          bind:this={canvasRef}
          bind:backendName
          paneId={activePaneId}
          workspaceId={activeWorkspaceId}
          {onStdin}
          {onResize}
          onHostClipboard={(text) => ws.setHostClipboard(text)}
          onNearTop={loadOlderScrollback}
          bind:selectionMode
          {sentenceBuffer}
        />
      {/key}
    {/await}
  {/if}

  {#if sidebarTab !== null && panelAvailability[sidebarTab]}
    <div class="sidebar-overlay" onclick={() => sidebarTab = null} role="presentation"></div>
    {#await RemoteSidebar}
      <div class="sidebar-loading">{$t('mobile.loading')}</div>
    {:then module}
      <module.default
        tab={sidebarTab}
        available={panelAvailability}
        cwd={activeCwd}
        {ws}
        onClose={() => sidebarTab = null}
        onTabChange={selectSidebarTab}
        onOpenFile={openFileViewer}
        onOpenDiff={openDiffViewer}
        onSelectPane={(paneId: string) => { activePaneId = paneId; sidebarTab = null; }}
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
        onClose={closeViewer}
      />
    {/await}
  {/if}

  <BottomTabBar
    {ws}
    {backendName}
    onRefresh={handleRefresh}
    onPaste={handlePaste}
    onThemeToggle={handleThemeToggle}
    {canUseTheme}
    {canManageWorkspaces}
    bind:selectionMode
    bind:sentenceBuffer
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
  .header-breadcrumb{flex:1;display:flex;flex-direction:column;align-items:center;justify-content:center;gap:1px;min-width:0;overflow:hidden}
  .breadcrumb-line{display:flex;align-items:center;justify-content:center;gap:6px;min-width:0;max-width:100%}
  .breadcrumb-text{font-size:13px;color:var(--rg-fg-muted);white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
  .breadcrumb-cwd{max-width:100%;font-size:10px;line-height:1.2;color:var(--rg-fg-muted);opacity:.7;white-space:nowrap;overflow:hidden;text-overflow:ellipsis;font-family:ui-monospace,SFMono-Regular,Menlo,Consolas,monospace}
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
