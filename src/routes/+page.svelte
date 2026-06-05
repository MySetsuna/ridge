<!-- src/routes/+page.svelte -->
<script lang="ts">

// Monaco Editor Worker 配置 - 必须在使用 monaco 之前配置
import editorWorker from 'monaco-editor/esm/vs/editor/editor.worker?worker';
import jsonWorker from 'monaco-editor/esm/vs/language/json/json.worker?worker';
import cssWorker from 'monaco-editor/esm/vs/language/css/css.worker?worker';
import htmlWorker from 'monaco-editor/esm/vs/language/html/html.worker?worker';
import tsWorker from 'monaco-editor/esm/vs/language/typescript/ts.worker?worker';

self.MonacoEnvironment = {
  getWorker(_: unknown, label: string) {
    if (label === 'json') {
      return new jsonWorker();
    }
    if (label === 'css' || label === 'scss' || label === 'less') {
      return new cssWorker();
    }
    if (label === 'html' || label === 'handlebars' || label === 'razor') {
      return new htmlWorker();
    }
    if (label === 'typescript' || label === 'javascript') {
      return new tsWorker();
    }
    return new editorWorker();
  }
};
  import { t, tr } from '$lib/i18n';
  import { focusActiveTerminal, ownsTabKey } from '$lib/terminal/terminalFocus';
  import SplitContainer from '$lib/components/SplitContainer.svelte';
  import SourceControl from '$lib/components/SourceControl.svelte';
  import WorkspaceTabs from '$lib/components/WorkspaceTabs.svelte';
  import Explorer from '$lib/components/Explorer.svelte';
  import FileEditor from '$lib/components/FileEditor.svelte';
  import ContextMenu from '$lib/components/ContextMenu.svelte';
  import WindDialog from '$lib/components/RidgeDialog.svelte';
  import WindToast from '$lib/components/WindToast.svelte';
  import { settingsStore, initSettingsBoot } from '$lib/stores/settings';
  import SettingsPanel from '$lib/components/SettingsPanel.svelte';
  import RemotePanel from '$lib/remote/RemotePanel.svelte';
  import { Smartphone } from 'lucide-svelte';
  // 云端登录态：侧栏头像 + 账户气泡。
  import { cloudAuth, logout as cloudLogout } from '$lib/remote/cloud/auth';
  import SearchSidebar from '$lib/components/SearchSidebar.svelte';
  import SidebarPluginRegion from '$lib/components/SidebarPluginRegion.svelte';
  import { portal } from '$lib/actions/portal';
  // Side-effect import: each built-in plugin auto-registers via its module
  // script. Must land once, at app chrome level.
  import '$lib/plugins';
  import { initThemeSystem } from '$lib/stores/themes';
  import { open as openDialog } from '@tauri-apps/plugin-dialog';
  import { setupTerminalThemeBridge } from '$lib/terminal/themeBridge';
  import {
    Terminal,
    FolderOpen,
    GitBranch,
    Layout,
    ChevronLeft,
    ChevronRight,
    ChevronDown,
    Split,
    X,
    Maximize2,
    Minimize2,
    Copy,
    Trash2,
    Plus,
    Minus,
    MoreHorizontal,
    Columns,
    Rows,
    Download,
    ArrowDown,
    ArrowUp,
    Settings,
    FileCode,
    FolderInput,
    History,
    Bookmark,
    Search,
    PanelRightOpen,
    RefreshCw,
    LogOut,
  } from 'lucide-svelte';
// 删除相关的最近工作区定义和函数
  import {
    paneTreeStore,
    workspacePaneTrees,
    workspaceSaveInfoStore,
    activePaneId,
    splitActivePane,
    splitPane,
    syncPaneLayoutFromBackend,
    refreshWorkspaces,
    workspacesList,
    activeWorkspaceId,
    createWorkspace,
    switchWorkspace,
    getAllPaneIds,
    closeWorkspace,
    reorderWorkspaces,
    renameWorkspace,
    saveCurrentWorkspace,
    loadSavedWorkspaces,
    getStartupContext,
    getRestoreSet,
    openWorkspaceFromFile,
    listSavedWorkspaceFiles,
    refreshWorkspaceSaveInfo,
    deleteWorkspaceFile, // 添加此导入
    closePane,
    paneCwdStore,
  } from '$lib/stores/paneTree';
  import { fileEditorStore } from '$lib/stores/fileEditor';
  import { initFileWatcherSync } from '$lib/stores/fileWatcherSync';
  import { getScmSelectedRepo } from '$lib/stores/scmCache';
  import {
    alertDialog,
    confirmDialog,
    promptDialog,
  } from '$lib/components/RidgeDialog.svelte';

// ─── 打开 .ridge 入口（双下拉）───
  // 副按钮 = 已保存工作区（Bookmark 图标，列出 ~/ridge-workspaces/*.ridge）。
  // 点击 = openWorkspaceFromFile。
  let savedOpen = $state(false);
  let savedList = $state<{ name: string; path: string; mtime_secs: number }[]>([]);
  let savedBtn: HTMLButtonElement | undefined = $state();
  let savedPopupStyle = $state('');

  async function loadSavedAndToggle() {
    savedList = await listSavedWorkspaceFiles();
    if (!savedOpen && savedBtn) {
      const r = savedBtn.getBoundingClientRect();
      savedPopupStyle = `top:${r.bottom + 4}px;left:${r.left}px`;
    }
    savedOpen = !savedOpen;
  }
  async function openSaved(path: string) {
    savedOpen = false;
    try {
      await openWorkspaceFromFile(path);
    } catch (err) {
      await alertDialog({ title: tr('main.dlgOpenFailTitle'), message: String(err), danger: true });
    }
  }

  async function pickAndOpenWorkspace() {
    savedOpen = false;
    try {
      const picked = await openDialog({
        multiple: false,
        filters: [{ name: 'Ridge Workspace', extensions: ['ridge'] }],
        title: tr('main.openRidgeDialogTitle'),
      });
      if (typeof picked === 'string' && picked) {
        await openWorkspaceFromFile(picked);
      }
    } catch (err) {
      await alertDialog({ title: tr('main.dlgOpenFailTitle'), message: String(err), danger: true });
    }
  }

  import {
    hideContextMenu,
    showContextMenu,
    isResizeInProgress,
    type ContextMenuTarget,
    type ContextMenuItem,
  } from '$lib/stores/contextMenu';

  import { reportDevIssue } from '$lib/devIssue';
  import { dev } from '$app/environment';
  import { parseLayoutChange, type LayoutChange } from '$lib/teammate/layoutEvent';
  import { get } from 'svelte/store';

  import { onMount, tick } from 'svelte';
  import { invoke, isTauri } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { getCurrentWindow } from '@tauri-apps/api/window';
  // §web-remote: true only in the desktop-UI-in-browser build (vite define).
  // Used to hide surfaces that make no sense when the desktop page is itself
  // being served to a remote browser: the "remote control" sidebar entry (you
  // ARE the remote) and the native window controls (no OS window to drive).
  const webRemote = import.meta.env.RIDGE_WEB_REMOTE === true;
  import { TerminalManager } from '$lib/terminal/manager';
  import { isTuiActive, snapshotLiveSignals } from '$lib/terminal/tuiGate';

  let rootNode = $derived($paneTreeStore);
  let hasPaneLayout = $derived(getAllPaneIds(rootNode).length > 0);

  // §A.9 (2026-05-08 follow-up) — single global canvas. ALL workspaces'
  // panes render onto one `<canvas data-rg-host>` mounted ONCE at the
  // pane area's parent (outside the per-workspace `{#each}` loop).
  // Switching workspaces is then a pure DOM display flip on the
  // SplitContainer side; the canvas itself never reconfigures, the
  // pipeline never re-inits, the swap chain never clears → no black
  // flash on switch.
  //
  // Inactive workspaces' panes are skipped at render time via the 0×0
  // bbox check in manager.ts (their SplitContainer is `display:none`
  // → contained pane containers measure 0×0 → `_isContainerHidden`
  // skips them). Their kernels stay alive and keep receiving PTY
  // bytes; switching back is just "the next RAF tick passes the bbox
  // gate" — typically <16ms.
  //
  // Svelte action: bind THE canvas to the manager's global SurfaceHost
  // via `manager.attachHost(canvas)`, observe its parent for resize so
  // the swap chain stays in sync. Cleans up on app teardown only —
  // the canvas is mounted once for the app's lifetime.
  function globalHostCanvas(node: HTMLCanvasElement) {
    const manager = TerminalManager.instance();
    void manager.attachHost(node).catch((err) => {
      console.warn('[ridge] attachHost failed for global canvas', err);
    });
    const parent = node.parentElement;
    let observer: ResizeObserver | undefined;
    let pendingRaf = 0;
    let pendingDims: { wCss: number; hCss: number } | null = null;
    if (parent) {
      observer = new ResizeObserver((entries) => {
        // Capture dims from the ResizeObserver entry (no layout query).
        // Stash on outer scope so the RAF callback below uses the LATEST
        // observed dims even when several observer fires coalesce into
        // one RAF tick.
        const e = entries[entries.length - 1];
        if (e) pendingDims = { wCss: e.contentRect.width, hCss: e.contentRect.height };
        if (pendingRaf !== 0) return;
        pendingRaf = requestAnimationFrame(() => {
          pendingRaf = 0;
          if (pendingDims) {
            manager.resizeHost(pendingDims);
            pendingDims = null;
          } else {
            manager.resizeHost();
          }
        });
      });
      observer.observe(parent);
    }
    return {
      destroy() {
        if (pendingRaf !== 0) cancelAnimationFrame(pendingRaf);
        observer?.disconnect();
        manager.detachHost();
      },
    };
  }

  // §A.9 — when the active workspace changes, the SplitContainer
  // wrappers swap their `display:none/flex` state in the same Svelte
  // microtask. Pane containers in the newly-active workspace transition
  // from 0×0 → real bbox; the old workspace's go the other way. The
  // existing `_isContainerHidden` gate in the RAF loop will pick this up
  // automatically next tick, but ResizeObserver is async and the first
  // post-switch RAF can read stale rects → up to one tick of empty
  // canvas.
  //
  // To make the switch feel truly instant: the moment the store flips,
  // tell the manager which workspace just became active. It will
  // recompute viewports for that workspace's panes against the (still
  // alive) global host canvas, invalidate, and wake the RAF loop so the
  // next frame paints the right scissors. No-op when no global host
  // (Canvas2D fallback path doesn't share a surface).
  $effect(() => {
    const wsId = $activeWorkspaceId;
    if (!wsId) return;
    // tick() lets Svelte commit the display:flex/none toggle before we
    // measure containers — without it, _recomputeViewport may still
    // observe the pre-switch layout.
    void tick().then(() => {
      try {
        TerminalManager.tryInstance()?.onActiveWorkspaceChanged(wsId);
      } catch (e) {
        console.warn('[ridge] onActiveWorkspaceChanged threw', e);
      }
    });
  });

  type SidebarTab = 'git' | 'files' | 'search' | 'claude' | 'remote';
  let sidebarTab = $state<SidebarTab>('files');



  // localStorage 键名
  const SIDEBAR_WIDTH_KEY = 'ridge-sidebar-width';
  const SIDEBAR_COLLAPSED_KEY = 'ridge-sidebar-collapsed';

  // 侧边栏宽度状态（用于可拖拽调整大小）
  let sidebarWidth = $state(288); // 默认 w-72 = 288px
  // 侧边栏是否折叠
  let sidebarCollapsed = $state(false);
  let isResizingSidebar = $state(false);

  // Sidebar resize cap. Tracked as $state (not just $derived) so a
  // window-level `resize` listener can push updates — `window.innerWidth`
  // isn't reactive on its own. Cap was 40% (sidebarMaxPx) until
  // round-39 user feedback bumped it to 80% to give SCM/diff users
  // genuinely wide working space without bumping a hard wall.
  let viewportInnerWidth = $state(
    typeof window !== 'undefined' ? window.innerWidth : 1000
  );
  let sidebarMaxPx = $derived(viewportInnerWidth * 0.8);

  // 设置面板开关。Settings 按钮打开后，所有可配置项（主题、字体、搜索、扩展）
  // 都集中在 SettingsPanel 内 —— 鼠标无需在多个角落寻找各自的入口。
  let settingsPanelOpen = $state(false);

  // 账户头像气泡（云端登录态）。头像在活动栏设置按钮上方，仅登录后显示。
  // UserDto 无 avatar 字段 → 一律用 username/email 首字母占位。
  let accountOpen = $state(false);
  let accountBtn: HTMLButtonElement | undefined = $state();
  let accountPopupStyle = $state('');
  const cloudUser = $derived($cloudAuth.user);
  const cloudLoggedIn = $derived(!!$cloudAuth.userToken);
  function accountInitial(): string {
    const n = (cloudUser?.username || cloudUser?.email || '').trim();
    return n ? n.charAt(0).toUpperCase() : '?';
  }
  function toggleAccount(): void {
    if (!accountOpen && accountBtn) {
      const r = accountBtn.getBoundingClientRect();
      // 头像贴左侧活动栏 → 气泡弹到其右侧，底部与头像底对齐、向上生长。
      accountPopupStyle = `bottom:${Math.round(window.innerHeight - r.bottom)}px;left:${Math.round(r.right + 8)}px`;
    }
    accountOpen = !accountOpen;
  }
  function doCloudLogout(): void {
    cloudLogout();
    accountOpen = false;
  }

  // 从 localStorage 加载侧边栏设置
  function loadSidebarSettings() {
    if (typeof localStorage === 'undefined') return;
    const savedWidth = localStorage.getItem(SIDEBAR_WIDTH_KEY);
    const savedCollapsed = localStorage.getItem(SIDEBAR_COLLAPSED_KEY);
    if (savedWidth) {
      const parsed = parseInt(savedWidth, 10);
      if (!isNaN(parsed) && parsed > 0) {
        sidebarWidth = Math.min(parsed, sidebarMaxPx);
      }
    }
    if (savedCollapsed === 'true') {
      sidebarCollapsed = true;
    }
  }

  // 保存侧边栏设置到 localStorage
  function saveSidebarSettings() {
    if (typeof localStorage === 'undefined') return;
    localStorage.setItem(SIDEBAR_WIDTH_KEY, String(sidebarWidth));
    localStorage.setItem(SIDEBAR_COLLAPSED_KEY, String(sidebarCollapsed));
  }

  // 切换侧边栏折叠状态
function toggleSidebar() {
  if (sidebarCollapsed) {
    // 展开到默认宽度
    sidebarCollapsed = false;
    sidebarWidth = 288; // 默认 w-72
  } else {
    sidebarCollapsed = true;
  }
  saveSidebarSettings();
}

// 展开侧边栏（不切换折叠状态，用于 tab 切换时）
function expandSidebar() {
  if (sidebarCollapsed) {
    sidebarCollapsed = false;
    sidebarWidth = 288;
    saveSidebarSettings();
  }
}

  // 侧边栏折叠/展开时的宽度
  const COLLAPSED_WIDTH = 0;

  function onSidebarResizerMouseDown(e: MouseEvent) {
    e.preventDefault();
    e.stopPropagation();
    isResizingSidebar = true;
    const startX = e.clientX;
    const startWidth = sidebarWidth;

    function onMouseMove(ev: MouseEvent) {
      const delta = ev.clientX - startX;
      const maxWidth = sidebarMaxPx;
      const newWidth = startWidth + delta;
      // 允许拖动到0关闭侧边栏
      sidebarWidth = Math.max(0, Math.min(maxWidth, newWidth));
      // 如果宽度小于20px，自动折叠
      if (sidebarWidth < 20) {
        sidebarCollapsed = true;
        sidebarWidth = 0;
      }
    }

    function onMouseUp() {
      isResizingSidebar = false;
      window.removeEventListener('mousemove', onMouseMove);
      window.removeEventListener('mouseup', onMouseUp);
      // 保存设置
      saveSidebarSettings();
    }

    window.addEventListener('mousemove', onMouseMove);
    window.addEventListener('mouseup', onMouseUp);
  }

  // 检查指定 pane 是否处于 TUI 活跃状态（全局级别，不依赖任何组件实例）
  function isPaneTuiActive(paneId: string): boolean {
    const mgr = TerminalManager.tryInstance();
    if (!mgr) return false;
    // Live-only check (no sticky history): a global shortcut should defer to
    // the TUI only while it's genuinely active right now.
    return isTuiActive(snapshotLiveSignals(
      mgr.isAltScreen(paneId),
      mgr.isInlineTuiActive(paneId),
      mgr.isMouseReporting(paneId),
      mgr.isAppCursorKeys(paneId),
    ));
  }

  // 键盘快捷键处理
  function handleGlobalKeydown(e: KeyboardEvent) {
    // 全局禁止页面刷新（F5 / Ctrl+R / Ctrl+Shift+R / Cmd+R）
    if (e.key === 'F5' || ((e.ctrlKey || e.metaKey) && (e.key === 'r' || e.key === 'R'))) {
      e.preventDefault();
      return;
    }
    // Tab 焦点收敛：终端真正的输入目标是每个 pane 的隐藏 IME helper
    // textarea（或 direct 模式下 tabindex=-1 的容器）。当焦点漂到桌面
    // chrome（工作区标签、文件树行、工具栏按钮）或 <body> 时，裸 Tab 会
    // 沿 chrome 的焦点环游走——把用户从没想选的元素逐个“选中”。这里把它
    // 拽回当前活动终端。`defaultPrevented` 表示已聚焦的 pane 已经把 Tab
    // 编码进 shell（onContainerKeyDown 两条路径都 preventDefault），故跳过；
    // 真正的文本输入（搜索 / 重命名 / Monaco 编辑器）保留原生 Tab 行为。
    if (e.key === 'Tab' && !e.defaultPrevented && !ownsTabKey(document.activeElement)) {
      if (focusActiveTerminal()) {
        e.preventDefault();
        return;
      }
    }
    // Ctrl+B: 切换侧边栏
    if (e.ctrlKey && (e.key === 'b' || e.key === 'B')) {
      e.preventDefault();
      toggleSidebar();
      return;
    }
    // Ctrl+Shift+F: 打开搜索侧栏（VS Code 对齐）。展开侧栏（若折叠）+ 切 tab。
    if (e.ctrlKey && e.shiftKey && (e.key === 'f' || e.key === 'F')) {
      e.preventDefault();
      sidebarTab = 'search';
      if (sidebarCollapsed) {
        sidebarCollapsed = false;
        saveSidebarSettings();
      }
      return;
    }
    // Ctrl+A: 全选当前文本输入框的所有文本 (只在输入框/textarea上生效)
    if (e.ctrlKey && (e.key === 'a' || e.key === 'A')) {
      const target = e.target as HTMLElement | null;
      // 当焦点在 TUI 活跃的终端 pane 上时，Ctrl+A 应由 TUI 处理
      // （如 vim/less 的 Ctrl+A 快捷键），不在此拦截。
      if (target?.closest?.('[data-rg-pane-id]')) {
        const paneEl = target.closest('[data-rg-pane-id]') as HTMLElement;
        const paneId = paneEl.dataset.rgPaneId;
        if (paneId && isPaneTuiActive(paneId)) return;
      }
      if (
        target &&
        (target.tagName === 'INPUT' ||
          target.tagName === 'TEXTAREA' ||
          target.isContentEditable)
      ) {
        // 让浏览器默认行为处理全选
        return;
      }
      // 如果不是文本输入元素，阻止默认行为（避免误触）
      e.preventDefault();
    }
  }



  // 窗口控制
  let isMaximized = $state(false);

  async function handleMinimize() {
    if (!isTauri()) return;
    const win = getCurrentWindow();
    await win.minimize();
  }

  async function handleMaximize() {
    if (!isTauri()) return;
    const win = getCurrentWindow();
    await win.toggleMaximize();
    isMaximized = await win.isMaximized();
  }

  async function handleClose() {
    if (!isTauri()) return;
    const win = getCurrentWindow();
    await win.close();
  }

  function openDevIssueHelp() {
    reportDevIssue({
      title: 'Ridge Dev',
      message: tr('main.dlgDevIssueMsg'),
    });
  }

  // 根据点击元素判断右键菜单目标类型
  function getContextMenuTarget(e: MouseEvent): {
    target: ContextMenuTarget;
    paneId?: string;
  } {
    const target = e.target as HTMLElement;

    // 检查是否点击在侧边栏区域
    if (target.closest('.rg-sidebar')) {
      return { target: 'sidebar' };
    }

    // 检查是否点击在工作区标签区域
    if (target.closest('.rg-workspace-tabs')) {
      return { target: 'workspace-tabs' };
    }

    // 检查是否点击在 Git 图谱区域
    if (target.closest('.rg-git-graph')) {
      return { target: 'git-graph' };
    }

    // 检查是否点击在分割条上
    if (target.closest('.splitpanes__splitter')) {
      return { target: 'splitter' };
    }

    // 检查是否点击在窗格标题栏
    if (target.closest('.rg-pane-header')) {
      const headerEl = target.closest('.rg-pane-header')!;
      const wrapper = headerEl.closest('.splitpanes__pane') as HTMLElement | null;
      const paneEl = wrapper?.querySelector('[data-rg-pane-id]') as HTMLElement | null;
      const paneId = paneEl?.getAttribute('data-rg-pane-id');
      return { target: 'pane-header', paneId: paneId ?? undefined };
    }

    // 检查是否点击在终端或编辑器内容区域
    // 注意属性名是 `data-rg-pane-id`（RidgePane.svelte 用的就是这个），早期
    // 重构前的 `data-pane-id` 现在没有任何元素设置 —— 旧选择器一直 miss，
    // 导致这里返回 target='unknown'，document-level handler 进而把它的
    // 「unknown」菜单贴在 RidgePane.onContextMenu 已显示的丰富菜单上面，
    // 用户看到的就是 RidgePane 菜单一闪而过，最后留下错误菜单。
    const paneEl =
      target.closest('.rg-pane-root') || target.closest('[data-rg-pane-id]');
    if (paneEl) {
      const paneId =
        paneEl.getAttribute('data-rg-pane-id') ||
        (paneEl as HTMLElement).dataset?.rgPaneId;
      // 判断是终端还是编辑器（通过 class 判断）。
      // RidgePane 渲染时挂 `.rg-pane-container[data-rg-pane-id]`；保留
      // `.rg-terminal-surface` 兜底以防其他外壳类名出现。Monaco 编辑器
      // 永远是 `.monaco-editor`。
      const isTerminal =
        target.closest('.rg-pane-container') || target.closest('.rg-terminal-surface');
      const isEditor = target.closest('.monaco-editor');
      if (isTerminal) {
        return { target: 'terminal', paneId };
      }
      if (isEditor) {
        return { target: 'editor', paneId };
      }
      return { target: 'pane-content', paneId };
    }

    return { target: 'unknown' };
  }

  /**   * Real handler shared across menu items + the Ctrl+W shortcut.
   * Centralises the "close *this* pane" semantic so the menu fires the
   * same code path as the keyboard shortcut.
   */
  async function closeCurrentPane(targetPaneId?: string): Promise<void> {
    const pid = targetPaneId || get(activePaneId);
    if (!pid) return;
    try {
      await closePane(pid);
    } catch (e) {
      await alertDialog({ title: tr('main.dlgCloseFailTitle'), message: String(e), danger: true });
    }
  }

  /** Close every leaf pane in the active workspace EXCEPT the given one. */
  async function closeOtherPanes(keepPaneId?: string): Promise<void> {
    const keep = keepPaneId || get(activePaneId);
    if (!keep) return;
    const ids = getAllPaneIds(rootNode).filter((id) => id !== keep);
    if (ids.length === 0) return;
    const ok = await confirmDialog({
      title: tr('main.dlgCloseOthersTitle'),
      message: tr('main.dlgCloseOthersMsg', { count: ids.length }),
      okLabel: tr('main.dlgCloseOthersOk'),
      danger: true,
    });
    if (!ok) return;
    for (const id of ids) {
      try {
        await closePane(id);
      } catch {
        /* best-effort — keep going */
      }
    }
  }

  /** 全零 UUID —— 后端在没有活动工作区的退化状态下会序列化为此值；与空串一并
   *  视作「无活动工作区」。 */
  const NIL_WORKSPACE_ID = '00000000-0000-0000-0000-000000000000';

  /** 启动兜底：保证当前一定有一个活动工作区。远程控制器（桌面 SPA）连接后若因
   *  竞态/异常仍无活动工作区，则采用 host 工作区列表里的第一个；列表为空才新建，
   *  避免用户被卡在「请先选择一个工作区」而无法操作。仅在缺失时介入，正常路径零开销。 */
  async function ensureActiveWorkspace(): Promise<void> {
    const current = get(activeWorkspaceId);
    if (current && current !== NIL_WORKSPACE_ID) return;
    try {
      const list = get(workspacesList);
      if (list.length > 0) {
        await switchWorkspace(list[0].id);
      } else {
        await createWorkspace();
      }
      // 切换/新建后重新拉取，使顶部工作区下拉与活动 id 同步。
      await refreshWorkspaces();
    } catch (e) {
      console.warn('ensureActiveWorkspace failed', e);
    }
  }

  async function renameActiveWorkspace(): Promise<void> {
    const wid = get(activeWorkspaceId);
    if (!wid) return;
    const ws = get(workspacesList).find((w) => w.id === wid);
    const newName = await promptDialog({
      title: tr('main.dlgRenameTitle'),
      message: tr('main.dlgRenameMsg'),
      defaultValue: ws?.name ?? '',
      placeholder: tr('main.dlgRenamePlaceholder'),
    });
    if (!newName?.trim()) return;
    try {
      await renameWorkspace(wid, newName.trim());
    } catch (e) {
      await alertDialog({ title: tr('main.dlgRenameFailTitle'), message: String(e), danger: true });
    }
  }

  /** Run a git command against the SCM-selected repo (or any one repo
   *  if SCM hasn't been opened yet). Surface errors in a themed alert. */
  async function runGitOnSelectedRepo(cmd: string, label: string): Promise<void> {
    if (!isTauri()) return;
    // Prefer the repo the SCM panel currently has selected (persisted in
    // scmCacheStore so it survives tab switches). Fall back to discovery
    // from paneCwdStore when SCM hasn't been opened yet.
    let repoRoot: string | null = getScmSelectedRepo() || null;
    if (!repoRoot) {
      const cwds = Array.from(new Set(Object.values(get(paneCwdStore))));
      for (const cwd of cwds) {
        try {
          const r = await invoke<string | null>('find_git_repo_root', { path: cwd });
          if (r) {
            repoRoot = r;
            break;
          }
        } catch {
          /* try next */
        }
      }
    }
    if (!repoRoot) {
      await alertDialog({
        title: tr('main.dlgGitOpFailed', { label }),
        message: tr('main.dlgNoGitRepo'),
        danger: true,
      });
      return;
    }
    try {
      await invoke(cmd, { repoRoot });
      // Tell the SCM panel + pane pills to refresh.
      window.dispatchEvent(
        new CustomEvent('ridge:scm-focus-repo', { detail: repoRoot })
      );
    } catch (e) {
      await alertDialog({ title: tr('main.dlgGitOpFailed', { label }), message: String(e), danger: true });
    }
  }

  function copyPaneCwd(targetPaneId?: string): void {
    const pid = targetPaneId || get(activePaneId);
    if (!pid) return;
    const wid = get(activeWorkspaceId);
    const cwd = get(paneCwdStore)[`${wid}:${pid}`] ?? '';
    if (!cwd) {
      void alertDialog({ title: tr('main.dlgCopyCwdTitle'), message: tr('main.dlgCopyCwdMsg') });
      return;
    }
    navigator.clipboard?.writeText(cwd).catch(() => {
      /* swallow — alert would fire too late after store unmount */
    });
  }

  function revealPaneCwd(targetPaneId?: string): void {
    if (!isTauri()) return;
    const pid = targetPaneId || get(activePaneId);
    if (!pid) return;
    const wid = get(activeWorkspaceId);
    const cwd = get(paneCwdStore)[`${wid}:${pid}`] ?? '';
    if (!cwd) return;
    void invoke('reveal_in_file_manager', { path: cwd });
  }

  // 生成右键菜单项
  function getContextMenuItems(
    target: ContextMenuTarget,
    paneId?: string
  ): ContextMenuItem[] {
    const items: ContextMenuItem[] = [];

    switch (target) {
      case 'terminal':
      case 'editor':
      case 'pane-content':
        items.push(
          {
            id: 'split-h',
            label: tr('main.ctxSplitH'),
            icon: Columns,
            shortcut: 'Ctrl+Shift+H',
            action: () => splitActivePane('horizontal'),
          },
          {
            id: 'split-v',
            label: tr('main.ctxSplitV'),
            icon: Rows,
            shortcut: 'Ctrl+Shift+V',
            action: () => splitActivePane('vertical'),
          },
          { divider: true, id: 'divider-1' },
          {
            id: 'close',
            label: tr('main.ctxClosePane'),
            icon: X,
            shortcut: 'Ctrl+W',
            action: () => void closeCurrentPane(paneId),
          },
          {
            id: 'close-others',
            label: tr('main.ctxCloseOthers'),
            icon: Trash2,
            action: () => void closeOtherPanes(paneId),
          },
          { divider: true, id: 'divider-2' },
          {
            id: 'focus',
            label: tr('main.ctxFocusPane'),
            icon: Maximize2,
            action: () => activePaneId.set(paneId || ''),
          },
          { divider: true, id: 'divider-3' },
          {
            id: 'copy-cwd',
            label: tr('main.ctxCopyCwd'),
            icon: Copy,
            action: () => copyPaneCwd(paneId),
          },
          {
            id: 'reveal',
            label: tr('main.ctxRevealCwd'),
            icon: FolderOpen,
            action: () => revealPaneCwd(paneId),
          }
        );
        break;

      case 'splitter':
        items.push(
          {
            id: 'split-h',
            label: tr('main.ctxSplitH'),
            icon: Columns,
            action: () => splitActivePane('horizontal'),
          },
          {
            id: 'split-v',
            label: tr('main.ctxSplitV'),
            icon: Rows,
            action: () => splitActivePane('vertical'),
          }
          // NB: "均分窗格" 待后端 split_pane reset-ratios 命令支持后再启用。
        );
        break;

      case 'sidebar':
        items.push(
          {
            id: 'files',
            label: tr('main.ctxFiles'),
            icon: FolderOpen,
            action: () => {
              sidebarTab = 'files';
              sidebarCollapsed = false;
              saveSidebarSettings();
            },
          },
          {
            id: 'search',
            label: tr('main.ctxSearch'),
            icon: Search,
            action: () => {
              sidebarTab = 'search';
              sidebarCollapsed = false;
              saveSidebarSettings();
            },
          },
          {
            id: 'git',
            label: tr('main.ctxGit'),
            icon: GitBranch,
            action: () => {
              sidebarTab = 'git';
              sidebarCollapsed = false;
              saveSidebarSettings();
            },
          }
        );
        break;

      case 'workspace-tabs':
        items.push(
          {
            id: 'new-ws',
            label: tr('main.ctxNewWorkspace'),
            icon: Plus,
            action: () => createWorkspace(),
          },
          {
            id: 'rename',
            label: tr('main.ctxRenameWorkspace'),
            icon: MoreHorizontal,
            action: () => void renameActiveWorkspace(),
          },
          { divider: true, id: 'divider-1' },
          // 保存工作区入口在 Explorer 头部已有，菜单里不重复（避免双入口）。
          {
            id: 'close-ws',
            label: tr('main.ctxCloseWorkspace'),
            icon: X,
            // activeWorkspaceId is a store; read the current id at invocation time.
            action: () => closeWorkspace(get(activeWorkspaceId)),
          }
        );
        break;

      case 'git-graph':
        items.push(
          {
            id: 'open-scm',
            label: tr('main.ctxOpenScm'),
            icon: GitBranch,
            action: () => {
              sidebarTab = 'git';
              sidebarCollapsed = false;
              saveSidebarSettings();
            },
          },
          { divider: true, id: 'divider-1' },
          {
            id: 'fetch',
            label: 'Fetch',
            icon: Download,
            action: () => void runGitOnSelectedRepo('git_fetch', 'Fetch'),
          },
          {
            id: 'pull',
            label: 'Pull',
            icon: ArrowDown,
            action: () => void runGitOnSelectedRepo('git_pull', 'Pull'),
          },
          {
            id: 'push',
            label: 'Push',
            icon: ArrowUp,
            action: () => void runGitOnSelectedRepo('git_push', 'Push'),
          },
          {
            id: 'sync',
            label: 'Sync',
            icon: RefreshCw,
            action: () => void runGitOnSelectedRepo('git_sync', 'Sync'),
          }
        );
        break;

      case 'pane-header':
        items.push(
          {
            id: 'split-h',
            label: tr('main.ctxSplitH'),
            icon: Columns,
            action: () => { if (paneId) void splitPane(paneId, 'horizontal'); },
          },
          {
            id: 'split-v',
            label: tr('main.ctxSplitV'),
            icon: Rows,
            action: () => { if (paneId) void splitPane(paneId, 'vertical'); },
          },
          { divider: true, id: 'divider-1' },
          {
            id: 'copy-cwd',
            label: tr('main.ctxCopyCwd'),
            icon: Copy,
            action: () => copyPaneCwd(paneId),
          },
          {
            id: 'reveal',
            label: tr('main.ctxReveal'),
            icon: FolderOpen,
            action: () => revealPaneCwd(paneId),
          },
          { divider: true, id: 'divider-2' },
          {
            id: 'close',
            label: tr('main.ctxCloseOnlyPane'),
            icon: X,
            action: () => void closeCurrentPane(paneId),
          }
        );
        break;

      default:
        items.push(
          {
            id: 'new-ws',
            label: tr('main.ctxNewWorkspace'),
            icon: Plus,
            action: () => createWorkspace(),
          }
        );
    }

    return items;
  }

  // 处理右键菜单事件
  function handleContextMenu(e: MouseEvent) {
    // T9：项目全局禁用系统默认右键菜单 —— 无论 target 是哪个，都先 preventDefault。
    // Monaco 自己的 contextmenu listener 早于本 document-level handler 跑，并已经
    // 弹出它自己的菜单（Go to Definition / Rename Symbol 等），prevent 不影响它。
    // 终端 / 编辑器 / 任何空白区域系统菜单都不会再出现。
    e.preventDefault();

    // resize 过程中不显示自定义菜单
    if (isResizeInProgress()) return;

    const { target, paneId } = getContextMenuTarget(e);

    // Monaco 已经显示了它自己的菜单 —— Ridge 不再叠加一层稀疏菜单。
    if (target === 'editor') return;

    // 终端窗格的右键菜单由 RidgePane.svelte::onContextMenu 拥有 ——
    // 它包含真正终端语义的项目（复制 / 粘贴 / 全选 / 清空），并在 paneId
    // 上下文里调 manager.* API。document-level handler 这里只做兜底，
    // 不能再把它的「分割 / 关闭 / cwd」泛化菜单贴在 terminal 头上覆盖
    // 掉 RidgePane 已经显示的丰富菜单（先 bubble 到 RidgePane，再 bubble
    // 到 document，后者后跑会覆盖前者）。同样的「让最贴近 target 的
    // handler 拥有其菜单」模式见上面 editor。
    if (target === 'terminal') return;

    const items = getContextMenuItems(target, paneId);
    showContextMenu(e.clientX, e.clientY, items, target, paneId);
  }

  // 侧栏切换 public event：任何组件（例如 pane 标题栏的 git pill）通过
  // `window.dispatchEvent(new CustomEvent('ridge:open-sidebar-tab', {detail:'git'}))`
  // 请求切 tab。把事件集中在 +page 这一层能避开跨组件 store 循环。
  function handleOpenSidebarTab(e: Event) {
    const detail = (e as CustomEvent<string>).detail;
    if (
      detail === 'files' ||
      detail === 'search' ||
      detail === 'git'
    ) {
      sidebarTab = detail;
      if (sidebarCollapsed) {
        sidebarCollapsed = false;
        saveSidebarSettings();
      }
    }
  }

  onMount(() => {
    // Sync onMount — Svelte's `onMount(async () => …)` returns a Promise
    // and the framework silently DROPS any cleanup function resolved from
    // it. With `async`, every listener / subscription registered below
    // would have leaked on every component unmount and HMR reload. We
    // keep the outer handler sync (so the returned cleanup actually runs)
    // and move the single `await initThemeSystem()` plus all dependent
    // setup into an inner `void (async () => …)()` IIFE.

    // 全局屏蔽默认右键菜单，显示自定义菜单
    document.addEventListener('contextmenu', handleContextMenu);
    window.addEventListener('ridge:open-sidebar-tab', handleOpenSidebarTab as EventListener);

    // Track viewport width so `sidebarMaxPx` (80% cap) recomputes when
    // the user resizes the window — otherwise a 2000px-wide sidebar
    // could outlive a window shrunk to 1000px.
    const onResize = () => {
      viewportInnerWidth = window.innerWidth;
      // Defensive: clamp current sidebar width if it now exceeds the
      // shrunken cap. Persist so reload doesn't restore the over-cap value.
      if (sidebarWidth > sidebarMaxPx) {
        sidebarWidth = Math.max(0, sidebarMaxPx);
        saveSidebarSettings();
      }
    };
    window.addEventListener('resize', onResize);

    loadSidebarSettings();

    // Hoisted cleanup handles so the sync `return` below can dispose them
    // even if the async IIFE hasn't yet assigned them (`?.()` guards undef).
    let unlisten: (() => void) | undefined;
    let unlistenResized: (() => void) | undefined;
    let unsubDefaultCwd: (() => void) | undefined;

    void (async () => {
      try {
      // 初始化主题系统：从后端获取主题数据
      await initThemeSystem();
      // 主题数据就绪后，把当前主题写到 CSS 变量
      initSettingsBoot();
      // CSS 变量就绪后再设置终端主题桥，确保 readRidgeTheme 读到正确值
      // 避免竞态：若 themeBridge 订阅先于 CSS 变量设置触发，
      // push() 会读到空 CSS 变量 → 终端底色展示缓存／错误颜色
      setupTerminalThemeBridge();

      // §A.9 (2026-05-08 follow-up) — single global host canvas. The
      // canvas itself is mounted by `globalHostCanvas` action on the
      // pane-area wrapper (just outside the workspace `{#each}` loop);
      // see the markup section below. No per-workspace canvas, no
      // per-workspace attachHost — switching workspaces is a pure DOM
      // toggle on the SplitContainer side, the canvas/swap-chain stays
      // alive across switches.

      // 文件系统监听桥接：订阅 explorer cwd + 编辑器外部文件，并把 fs-changed
      // 事件分发到文件树和编辑器。模块内部 idempotent，重复调用是安全的。
      initFileWatcherSync();

      if (!isTauri()) return;

      // 把用户配置的默认工作目录同步到后端 AppState（启动时 + 每次设置变更）。
      // 必须在 refreshWorkspaces / 任何 create_pane 之前订阅，否则首个 pane 会用旧
      // 优先级（home）而不是用户配置。Svelte writable 的 subscribe 立即用当前值
      // 触发一次，所以无需另写初始 push 路径。
      unsubDefaultCwd = settingsStore.subscribe((s) => {
        void invoke('set_user_default_cwd', { path: s.defaultCwd || null }).catch((err) => {
          console.warn('set_user_default_cwd failed', err);
        });
      });

      await refreshWorkspaces();
      // 启动策略：
      // 1. cli 启动（终端里 `ridge`）：cwd 是用户工作目录。
      //    - cwd 顶层有 .ridge → 打开它，关默认；否则保留默认（cwd 已种入根 pane）。
      //    - 不读取 restore set，避免覆盖用户用 cwd 表达的意图。
      // 2. menu 启动（双击 / 开始菜单）：cwd 是 ridge.exe 目录，无意义。
      //    - 优先读 restore set（上次关闭时已保存的工作区）；非空 → 全部 reopen，
      //      关掉默认空工作区，切到第一个；空 → 保留默认。
      try {
        const ctx = await getStartupContext();
        const priorDefaultId = get(activeWorkspaceId);
        const cwdRidge = ctx?.wind_file_in_cwd ?? null;
        if (cwdRidge) {
          await openWorkspaceFromFile(cwdRidge);
        } else if (ctx?.kind === 'menu') {
          const restorePaths = await getRestoreSet();
          for (const p of restorePaths) {
            try {
              await openWorkspaceFromFile(p);
            } catch (e) {
              console.warn('restore workspace failed', p, e);
            }
          }
        }
        // 默认空工作区如果不再是活动项（已被覆盖打开），关掉它，避免顶部多出空 tab。
        const nowActive = get(activeWorkspaceId);
        if (priorDefaultId && priorDefaultId !== nowActive) {
          try {
            await closeWorkspace(priorDefaultId);
          } catch (e) {
            console.warn('close default workspace after auto-open failed', e);
          }
        }
      } catch (err) {
        console.warn('startup workspace resolution failed', err);
      }
      // §web-remote 默认工作区兜底：远程控制器（桌面 SPA over LAN/cloud）连接后
      // 必须始终落在一个工作区上，绝不停留在「请先选择一个工作区」。host 启动时
      // 一定持有一个全局活动工作区，但若 refreshWorkspaces 期间发生竞态/异常导致
      // activeWorkspaceId 仍为空，这里主动恢复：优先切到列表里的第一个工作区
      // （即采用 host 当前工作区），列表为空才新建一个，确保用户可立即操作。
      await ensureActiveWorkspace();
      await loadSavedWorkspaces();
      await refreshWorkspaceSaveInfo();

      // 等待首个终端面板就绪后关闭 loader
      window.addEventListener('ridge:pane-attached', () => {
        window.dispatchEvent(new CustomEvent('ridge:app-ready'));
      }, { once: true });
      // 兜底：5秒后无论如何关闭 loader，避免异常阻塞
      setTimeout(() => {
        window.dispatchEvent(new CustomEvent('ridge:app-ready'));
      }, 5000);

      // 检查初始最大化状态
      const win = getCurrentWindow();
      isMaximized = await win.isMaximized();
      unlistenResized = await win.onResized(async () => {
        isMaximized = await getCurrentWindow().isMaximized();
      });

      // Re-sync the pane layout from authoritative backend state, then (in dev)
      // flag any store/DOM pane-count drift. Shared action for every kind.
      const applyLayoutSync = async (change: LayoutChange) => {
        await syncPaneLayoutFromBackend();
        if (!dev) return;
        requestAnimationFrame(() => {
          const storeCount = getAllPaneIds(get(paneTreeStore)).length;
          const domCount = document.querySelectorAll('.rg-pane-root').length;
          if (storeCount > 0 && domCount !== storeCount) {
            reportDevIssue({
              title: 'Layout sync mismatch',
              message: `teammate-layout-changed[${change.kind}] 后 store panes=${storeCount}, mounted panes=${domCount}`,
            });
          }
        });
      };
      // Dispatch seam keyed on the envelope `kind`. Today every kind re-syncs
      // from backend state; the P1 follow-ups specialize individual branches
      // (split/reused → deterministic await-Channel-then-fit per 5b; state →
      // agent badge flip per #6) without changing this contract.
      const handleLayoutChange = async (change: LayoutChange) => {
        switch (change.kind) {
          case 'split':
          case 'reused':
          case 'detached':
          case 'removed':
          case 'state':
            await applyLayoutSync(change);
            break;
          default: {
            // Exhaustiveness guard: if a new LayoutChangeKind is added to the
            // envelope, TS errors here until this switch handles it. Unreachable
            // at runtime (parseLayoutChange degrades unknown kinds to 'state').
            const _exhaustive: never = change.kind;
            void _exhaustive;
            await applyLayoutSync(change);
          }
        }
      };
      unlisten = await listen('teammate-layout-changed', (event) => {
        void handleLayoutChange(parseLayoutChange(event.payload));
      });

      const unlistenActive = await listen<string>(
        'teammate-active-pane-changed',
        (e) => {
          const id = typeof e.payload === 'string' ? e.payload : '';
          if (!id) return;
          void (async () => {
            await syncPaneLayoutFromBackend();
            activePaneId.set(id);
          })();
        }
      );

      // Remote-initiated pane/workspace changes: refresh the desktop UI.
      const unlistenPaneTreeChanged = await listen<{ workspaceId: string }>(
        'pane-tree-changed',
        () => {
          void syncPaneLayoutFromBackend();
        }
      );
      const unlistenWorkspaceListChanged = await listen(
        'workspace-list-changed',
        () => {
          void refreshWorkspaces();
        }
      );

      const prevUnlisten = unlisten;
      unlisten = () => {
        prevUnlisten?.();
        unlistenActive();
        unlistenPaneTreeChanged();
        unlistenWorkspaceListChanged();
      };
      } catch (err) {
        console.error('[boot] init failed', err);
      }
    })();

    return () => {
      unlisten?.();
      unlistenResized?.();
      unsubDefaultCwd?.();
      document.removeEventListener('contextmenu', handleContextMenu);
      window.removeEventListener('ridge:open-sidebar-tab', handleOpenSidebarTab as EventListener);
      window.removeEventListener('resize', onResize);
    };
  });

  // sidebar 图标按钮：颜色跟随主题 accent。原来写死 violet，浅色 / 棕色 / 绿色
  // 主题下整个 rail 仍是紫色调，与配色不符。
  const actBtn =
    'relative flex h-10 w-10 items-center justify-center rounded-xl text-lg transition-all duration-200 ' +
    'text-[var(--rg-fg-muted)] hover:bg-[var(--rg-accent)]/8 hover:text-[var(--rg-fg)]';
  const actBtnOn =
    ' bg-[var(--rg-accent)]/12 text-[var(--rg-accent)] ring-1 ring-[var(--rg-accent)]/35 shadow-[0_0_20px_-4px_var(--rg-accent-glow)]';

  const toolBtn =
    'flex h-8 w-8 shrink-0 items-center justify-center rounded-lg border border-[var(--rg-border)] ' +
    'bg-[var(--rg-surface)]/90 backdrop-blur-md text-[var(--rg-fg-muted)] ' +
    'hover:border-[var(--rg-accent)]/35 hover:text-[var(--rg-accent)] hover:bg-[var(--rg-accent)]/8 transition-colors';

  // 窗口控制按钮样式（跟随系统：Windows在右侧，macOS在左侧）
  const winCtrlBtn =
    'flex h-8 w-8 items-center justify-center rounded-lg text-[var(--rg-fg-muted)] hover:bg-[var(--rg-accent)]/8 hover:text-[var(--rg-fg)] transition-colors';
</script>

<svelte:window onkeydown={handleGlobalKeydown} />
<!-- Root must NOT carry `data-tauri-drag-region` — it makes Tauri intercept
     mousedown across the entire window, eating the gesture HTML5 DnD relies
     on to start a drag (round-38 bug: WorkspaceTabs reorder, pane drag,
     FileTree DnD, FileEditor tab reorder all silently broke). The OS-window
     drag region is correctly scoped to the top `<header>` (line ~1102) only,
     where there are no draggable children. -->
<div
  class="flex h-screen w-screen overflow-hidden bg-[var(--rg-bg)] text-[var(--rg-fg)] selection:bg-violet-500/25"
>
  <!-- 左侧图标导航栏 -->
  <aside
    class="w-[52px] shrink-0 flex flex-col items-center py-3 gap-1.5 border-r border-[var(--rg-border)] bg-[var(--rg-surface)]/35 backdrop-blur-2xl"
  >
    <button
      type="button"
      class="{actBtn}{sidebarTab === 'files' ? actBtnOn : ''}"
      title={$t('main.navFiles')}
      onclick={() => { sidebarTab = 'files'; expandSidebar(); }}
    >
      <FolderOpen class="h-5 w-5" />
    </button>
    <button
      type="button"
      class="{actBtn}{sidebarTab === 'search' ? actBtnOn : ''}"
      title={$t('main.navSearch')}
      onclick={() => { sidebarTab = 'search'; expandSidebar(); }}
    >
      <Search class="h-5 w-5" />
    </button>
    <button
      type="button"
      class="{actBtn}{sidebarTab === 'git' ? actBtnOn : ''}"
      title="Git Graph"
      onclick={() => { sidebarTab = 'git'; expandSidebar(); }}
    >
      <GitBranch class="h-5 w-5" />
    </button>
    <!-- Bottom-anchored extension manager — uses mt-auto so it stays at the
         rail's bottom regardless of how many tabs sit above. Click toggles
         the Claude Code extension. The icon flips between dim/Bot when off
         and accent/Bot when on, giving a single button that both tells the
         user the current state and lets them flip it without spelunking
         through nested settings. -->
    {#if !webRemote}
    <button
      type="button"
      class="{actBtn}{sidebarTab === 'remote' ? actBtnOn : ''}"
      title={$t('main.navRemote')}
      onclick={() => { sidebarTab = 'remote'; expandSidebar(); }}
    >
      <Smartphone class="h-5 w-5" />
    </button>
    {/if}
    <!-- 底部簇：登录后头像（在设置按钮上方）+ 设置按钮，整体锚定到 rail 底部。 -->
    <div class="mt-auto flex flex-col items-center gap-1.5">
      {#if !webRemote && cloudLoggedIn}
        <button
          bind:this={accountBtn}
          type="button"
          class="flex h-9 w-9 items-center justify-center rounded-full border border-[var(--rg-border)] bg-[var(--rg-accent)]/15 text-sm font-semibold text-[var(--rg-accent)] transition-all hover:ring-1 hover:ring-[var(--rg-accent)]/40 {accountOpen ? 'ring-1 ring-[var(--rg-accent)]/50' : ''}"
          title={cloudUser?.username || cloudUser?.email || $t('main.navAccount')}
          aria-label={$t('main.navAccount')}
          onclick={toggleAccount}
        >
          {accountInitial()}
        </button>
      {/if}
      <button
        type="button"
        class={actBtn}
        title={$t('main.navSettings')}
        onclick={() => (settingsPanelOpen = true)}
      >
        <Settings class="h-4 w-4" />
      </button>
    </div>
  </aside>
  <SettingsPanel open={settingsPanelOpen} onClose={() => (settingsPanelOpen = false)} />

  <!-- 侧边栏区域：wrapper 始终渲染，toggle 按钮始终可见 -->
  <div
    class="relative shrink-0 z-11"
    style="width: {sidebarCollapsed ? 0 : sidebarWidth}px; overflow: visible"
  >
      {#if !sidebarCollapsed}
      <aside
        class="h-full border-r border-[var(--rg-border)] bg-[var(--rg-surface-2)]/55 backdrop-blur-xl flex flex-col min-h-0"
      >
        <!-- Tab content container: holds the four absolutely-positioned
             tab panels (git / search / claude / files) inside a relative
             box. Anchoring `absolute inset-0` to this flex-1 box (instead
             of the whole aside) keeps the global SidebarPluginRegion
             footer below from being overlapped — the footer sits in the
             flex flow after this container, not underneath it. -->
        <div class="relative flex-1 min-h-0 rg-scroll overflow-y-auto">
        <!-- Git tab -->
        <div class="absolute inset-0 flex flex-col {sidebarTab === 'git' ? '' : 'hidden'}">
          <div
            data-tauri-drag-region
            class="px-3 h-11 items-center flex shrink-0 border-b border-[var(--rg-border)] text-xs font-semibold uppercase tracking-wider text-[var(--rg-fg-muted)]"
          >
            {$t('main.sidebarGitHeader')}
          </div>
          <div class="flex-1 min-h-0 overflow-hidden">
            <SourceControl />
          </div>
        </div>

        <!-- Search tab -->
        <div class="absolute inset-0 flex flex-col {sidebarTab === 'search' ? '' : 'hidden'}">
          <div class="flex-1 min-h-0 overflow-hidden">
            <SearchSidebar active={sidebarTab === 'search'} />
          </div>
        </div>

        <!-- Remote tab -->
        {#if !webRemote}
        <div class="absolute inset-0 flex flex-col {sidebarTab === 'remote' ? '' : 'hidden'}">
          <RemotePanel />
        </div>
        {/if}

        <!-- Files tab (default) -->
        <div class="absolute inset-0 flex flex-col {sidebarTab === 'files' ? '' : 'hidden'}">
          <div
            data-tauri-drag-region
            class="px-3 h-11 items-center flex justify-between shrink-0 border-b border-[var(--rg-border)] text-xs font-semibold uppercase tracking-wider text-[var(--rg-fg-muted)] relative"
          >
            <span>{$t('main.sidebarExplorerHeader')}</span>
          </div>
          <div class="flex-1 min-h-0 overflow-hidden">
            {#if $activeWorkspaceId}
              <Explorer workspaceId={$activeWorkspaceId} />
            {:else}
              <div
                class="p-4 text-[13px] leading-relaxed text-[var(--rg-fg-muted)]"
              >
                {$t('main.noWorkspaceSelected')}
              </div>
            {/if}
          </div>
        </div>

        </div>
        <!-- Global-scope plugin region — mounted once at the sidebar footer,
             visible across every tab. Sits OUTSIDE the absolute tab container
             above so the four tab panels can use `absolute inset-0` without
             overlaying it. -->
        <div class="shrink-0 border-t border-[var(--rg-border)]/40">
          <SidebarPluginRegion scope="global" />
        </div>

      </aside>
    {/if}
<!-- 侧边栏拖动条：始终渲染 — collapsed 时在 left-0（wrapper 宽度为0，左边界即导航栏右边缘）
     显示虚线；expanded 时在 right-0（侧边栏右边缘）作为透明可拖区域。
     wrapper 有 z-10 + overflow:visible，保证此元素即使在 collapsed 状态下也能响应点击。 -->
<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <!-- T16：与 SplitContainer 终端 splitter 视觉对齐 —— 8px 命中区透明，
       ::before 画 1px var(--rg-border) 中线；hover/drag 时 scaleX(4) 变粗
       并切到 var(--rg-accent) + accent-glow 阴影。 -->
  <div
    class="rg-sidebar-resize absolute top-0 h-full w-2 shrink-0 cursor-col-resize select-none z-30 {sidebarCollapsed ? 'left-0' : '-right-1'} {isResizingSidebar ? 'rg-sidebar-resize-active' : ''}"
    role="separator"
  aria-orientation="vertical"
  aria-label={sidebarCollapsed ? $t('main.sidebarResizeExpand') : $t('main.sidebarResizeAdjust')}
  tabindex="0"
  onmousedown={(e) => {
    if (sidebarCollapsed) {
      expandSidebar();
    } else {
      onSidebarResizerMouseDown(e);
    }
  }}
  onkeydown={(e) => {
    if (e.key === 'Enter' || e.key === ' ') {
      if (sidebarCollapsed) {
        expandSidebar();
      } else {
        toggleSidebar();
      }
      e.preventDefault();
      return;
    }
    if (sidebarCollapsed) return;
    if (e.key !== 'ArrowLeft' && e.key !== 'ArrowRight') return;
    const step = e.shiftKey ? 64 : 16;
    const delta = e.key === 'ArrowRight' ? step : -step;
    const maxWidth = sidebarMaxPx;
    sidebarWidth = Math.max(0, Math.min(maxWidth, sidebarWidth + delta));
    if (sidebarWidth < 20) {
      sidebarCollapsed = true;
      sidebarWidth = 0;
    }
    saveSidebarSettings();
    e.preventDefault();
  }}
></div>

    <!-- T16：移除显式折叠按钮 —— 用户仍可通过 Ctrl+B 快捷键 / 拖到极窄宽度
         自动折叠（onMouseMove < 20 自动 collapse）实现折叠。 -->
  </div>

  <!-- 主内容区 -->
  <div class="flex-1 flex flex-col min-w-0 min-h-0">
    <!-- 顶部标题栏 -->
    <header
      class="h-11 flex items-center gap-2 px-2 border-b border-[var(--rg-border)] bg-[var(--rg-glass)] backdrop-blur-md min-w-0"
      data-tauri-drag-region
    >
      <!-- 左侧元素组 -->
      <div class="flex items-center gap-2 flex-1" data-tauri-drag-region>
        <!-- 工作区标签区 -->
        <WorkspaceTabs
          workspaces={$workspacesList}
          activeWorkspaceId={$activeWorkspaceId}
          onSwitch={switchWorkspace}
          onClose={closeWorkspace}
          onReorder={reorderWorkspaces}
          onRename={renameWorkspace}
        >
          {#snippet actions()}
            <button
              bind:this={savedBtn}
              type="button"
              class="shrink-0 flex h-8 w-8 mr-1 items-center justify-center rounded-lg border border-[var(--rg-border)] text-[var(--rg-fg-muted)] hover:border-[var(--rg-accent)]/40 hover:text-[var(--rg-accent)] hover:bg-[var(--rg-accent)]/8 transition-colors"
              title={$t('main.savedWorkspacesBtn')}
              aria-label={$t('main.savedWorkspacesBtn')}
              onclick={() => void loadSavedAndToggle()}
            >
              <Bookmark class="h-4 w-4" />
            </button>
            {#if savedOpen}
              <!-- svelte-ignore a11y_no_static_element_interactions -->
              <div
                role="presentation"
                class="fixed inset-0 z-[9989]"
                onmousedown={() => (savedOpen = false)}
              >
                <!-- svelte-ignore a11y_interactive_supports_focus -->
                <div
                  style={savedPopupStyle}
                  class="rg-popup w-[300px] max-w-[90vw]"
                  role="menu"
                  use:portal={{ id: 'saved-workspaces' }}
                  onmousedown={(e) => e.stopPropagation()}
                >
                  <div class="flex items-center justify-between h-7 px-3 bg-[var(--rg-surface)]/60 border-b border-[var(--rg-border)]/60 text-[10px] font-semibold uppercase tracking-wider text-[var(--rg-fg-muted)]">
                    <span>{$t('main.savedWorkspacesTitle')}</span>
                    <button
                      type="button"
                      class="text-[10px] normal-case tracking-normal hover:text-[var(--rg-fg)]"
                      title={$t('main.savedWorkspacesBrowseTitle')}
                      onclick={() => { savedOpen = false; void pickAndOpenWorkspace(); }}
                    >
                      {$t('main.savedWorkspacesBrowse')}
                    </button>
                  </div>
                  <div class="max-h-[260px] overflow-y-auto">
                    {#if savedList.length === 0}
                      <div class="px-3 py-2 text-[11px] text-[var(--rg-fg-muted)]">{$t('main.savedWorkspacesEmpty')}</div>
                    {:else}
                      {#each savedList as s (s.path)}
                        <div class="group flex items-center justify-between w-full px-3 py-1.5 text-left hover:bg-[var(--rg-surface)] transition-colors normal-case tracking-normal">
                          <button
                            type="button"
                            class="flex-1 flex flex-col items-start min-w-0"
                            onclick={() => void openSaved(s.path)}
                            title={s.path}
                          >
                            <span class="text-[12px] text-[var(--rg-fg)] truncate max-w-full">{s.name}</span>
                            <span class="text-[10px] text-[var(--rg-fg-muted)] truncate max-w-full font-mono">{s.path}</span>
                          </button>
                          <button
                            type="button"
                            class="opacity-0 group-hover:opacity-100 p-1 rounded hover:bg-[var(--rg-surface)] hover:text-red-500 transition-colors"
                            title={$t('main.savedWorkspacesDelete')}
                            onclick={(e) => {
                              e.stopPropagation();
                              const info = Object.values($workspaceSaveInfoStore).find(
                                (i) => i.file_path === s.path
                              );
                              if (info) {
                                void deleteWorkspaceFile(info.workspace_id);
                              } else {
                                console.warn('Workspace file not associated with active workspace, direct deletion not implemented');
                              }
                            }}
                          >
                            <Trash2 class="h-3.5 w-3.5" />
                          </button>
                        </div>
                      {/each}
                    {/if}
                  </div>
                </div>
              </div>
            {/if}
          {/snippet}
          {#snippet trailingActions()}
            <button
              type="button"
              class="shrink-0 flex h-8 w-8 items-center justify-center rounded-lg border border-dashed border-[var(--rg-border)] text-[var(--rg-fg-muted)] hover:border-[var(--rg-accent)]/40 hover:text-[var(--rg-accent)] hover:bg-[var(--rg-accent)]/8 transition-colors"
              title={$t('main.newWorkspaceBtn')}
              onclick={() => createWorkspace()}
            >
              <span class="text-lg leading-none">+</span>
            </button>
          {/snippet}
        </WorkspaceTabs>

        <!-- 开发排障入口 -->
        {#if dev}
          <button
            type="button"
            class="rg-no-drag shrink-0 rounded-lg px-2.5 py-1.5 text-[11px] font-medium border border-red-500/30 text-red-300/90 hover:bg-red-500/10 transition-colors"
            title={$t('main.devIssueTooltip')}
            onclick={openDevIssueHelp}
          >
            Dev Issue
          </button>
        {/if}

<!-- 编辑器抽屉开关：没有打开文件时不展示 -->
        {#if $fileEditorStore.openFiles.length > 0}
        <button
          type="button"
          class="rg-no-drag {toolBtn} {$fileEditorStore.isVisible ? 'bg-[var(--rg-accent)]/15 text-[var(--rg-accent)]' : ''}"
          title={$fileEditorStore.isVisible ? $t('main.editorHide') : $t('main.editorShow')}
          onclick={() => fileEditorStore.toggleVisibility()}
        >
          <PanelRightOpen class="h-4 w-4" />
        </button>
        {/if}

        <!-- 分屏操作按钮 -->
        <div
          class="rg-no-drag flex items-center gap-1 rounded-xl backdrop-blur-md"
        >
          <button
            type="button"
            class={toolBtn}
            title={$t('main.splitHorizontal')}
            data-testid="add-pane-btn"
            onclick={() => void splitActivePane('horizontal')}
          >
            <svg
              class="h-4 w-4"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              stroke-width="2"
              aria-hidden="true"
            >
              <rect x="3" y="5" width="7" height="14" rx="1.5" />
              <rect x="14" y="5" width="7" height="14" rx="1.5" />
            </svg>
          </button>
          <button
            type="button"
            class={toolBtn}
            title={$t('main.splitVertical')}
            onclick={() => void splitActivePane('vertical')}
          >
            <svg
              class="h-4 w-4"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              stroke-width="2"
              aria-hidden="true"
            >
              <rect x="4" y="4" width="16" height="7" rx="1.5" />
              <rect x="4" y="13" width="16" height="7" rx="1.5" />
            </svg>
          </button>
        </div>

        <!-- 窗口控制按钮（右侧）：wf-no-drag 避免与标题栏拖动区域冲突 -->
      </div>
      <div class="rg-no-drag flex items-center gap-1 shrink-0" class:hidden={webRemote}>
        <button
          type="button"
          class={winCtrlBtn}
          title={$t('main.winMinimize')}
          onclick={handleMinimize}
        >
          <svg
            class="h-4 w-4"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="2"
          >
            <path d="M5 12h14" stroke-linecap="round" />
          </svg>
        </button>
        <button
          type="button"
          class={winCtrlBtn}
          title={isMaximized ? $t('main.winRestore') : $t('main.winMaximize')}
          onclick={handleMaximize}
        >
          {#if isMaximized}
            <svg
              class="h-4 w-4"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              stroke-width="2"
            >
              <rect x="5" y="9" width="10" height="10" rx="1" />
              <path d="M9 9V5h10v10h-4" />
            </svg>
          {:else}
            <svg
              class="h-4 w-4"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              stroke-width="2"
            >
              <rect x="4" y="4" width="16" height="16" rx="2" />
            </svg>
          {/if}
        </button>
        <button
          type="button"
          class="{winCtrlBtn} hover:bg-red-500/20 hover:text-red-400"
          title={$t('main.winClose')}
          onclick={handleClose}
        >
          <svg
            class="h-4 w-4"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="2"
          >
            <path d="M18 6L6 18M6 6l12 12" stroke-linecap="round" />
          </svg>
        </button>
      </div>
    </header>

    <!-- 工作区内容：flex-row 让嵌入模式的 FileEditor 作为右侧列，
         drawer/floating 模式的 FileEditor 通过 position:fixed 脱离普通流，不占用此空间。 -->
    <div
      class="relative flex-1 min-h-0 min-w-0 overflow-hidden flex flex-row bg-[var(--rg-bg-raised)]"
    >
      <div class="relative flex-1 min-w-0 min-h-0 overflow-hidden flex flex-col">
        {#if $activeWorkspaceId && hasPaneLayout}
          {#each $workspacesList as ws (ws.id)}
            {@const tree = $workspacePaneTrees.get(ws.id)}
            {#if tree}
              <div
                class="relative flex-1 min-w-0 min-h-0"
                style="display:{ws.id === $activeWorkspaceId ? 'flex' : 'none'};"
                data-rg-ws-pane-host={ws.id}
              >
                <SplitContainer workspaceId={ws.id} node={tree} />
              </div>
            {/if}
          {/each}
        {:else}
          <div style="display:none"></div>
        {/if}

        <!-- §A.9 (2026-05-08 follow-up) — single global canvas. ONE
             `<canvas data-rg-host>` lives at this always-mounted wrapper
             and serves every workspace's panes via per-pane scissors on
             the host. Switching workspaces is just a CSS `display:flex/
             none` flip on each workspace's SplitContainer wrapper above;
             the canvas / WebGPU swap chain / pipeline are never torn
             down or reconfigured, so the user sees an instant switch
             with no black flash and no atlas re-warm.

             Mounted AFTER the workspace each-loop in tree order so that
             — within the parent's stacking context — the canvas paints
             on top of the SplitContainer DOM. The per-pane scissor
             leaves splitter regions transparent on the canvas, so the
             DOM splitter strips below remain visible through those
             gaps. `pointer-events:none` lets clicks fall through the
             canvas to the SplitContainer for resize/focus interaction. -->
        <canvas
          use:globalHostCanvas
          data-rg-host
          aria-hidden="true"
          style="position:absolute; inset:0; width:100%; height:100%; pointer-events:none; z-index:0; display:block;"
        ></canvas>
      </div>
      <!-- 文件编辑器：嵌入模式时为右侧 flex 列；抽屉/悬浮模式时 position:fixed 脱离流 -->
      <FileEditor />
    </div>
  </div>

</div>

<!-- ── 顶层浮层区 ─────────────────────────────────────────────────────────
     以下组件使用 position:fixed，必须作为根 <div> 的兄弟节点而非其子节点，
     确保它们不受根容器任何 CSS transform / backdrop-filter / overflow 限制，
     始终以整个应用窗口为参照系居中或定位。 -->

<!-- 账户气泡：作为根 <div> 兄弟节点，position:fixed 以整窗为参照（不受活动栏
     backdrop-filter 影响）。点击遮罩或退出登录后关闭。 -->
{#if accountOpen && cloudLoggedIn}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    role="presentation"
    class="fixed inset-0 z-[9989]"
    onmousedown={() => (accountOpen = false)}
  >
    <!-- svelte-ignore a11y_interactive_supports_focus -->
    <div
      style={accountPopupStyle}
      class="rg-popup fixed w-[240px] max-w-[80vw] overflow-hidden"
      role="menu"
      onmousedown={(e) => e.stopPropagation()}
    >
      <div class="border-b border-[var(--rg-border)]/60 px-3 py-2.5">
        <p class="truncate text-xs font-medium text-[var(--rg-fg)]">
          {cloudUser?.username || $t('main.accountNoName')}
        </p>
        {#if cloudUser?.email}
          <p class="truncate text-[10px] text-[var(--rg-fg-muted)]">{cloudUser.email}</p>
        {/if}
      </div>
      <button
        type="button"
        class="flex w-full items-center gap-2 px-3 py-2 text-left text-xs text-[var(--rg-fg)] transition-colors hover:bg-[var(--rg-surface)] hover:text-red-400"
        onclick={doCloudLogout}
      >
        <LogOut class="h-3.5 w-3.5" /> {$t('main.accountLogout')}
      </button>
    </div>
  </div>
{/if}

<!-- alert / confirm / prompt 替代浏览器原生 dialog -->
<WindDialog />
<!-- 轻量 toast 通知 (z-10000，位于所有 modal 之上) -->
<WindToast />
<!-- 全局右键菜单 -->
<ContextMenu />

