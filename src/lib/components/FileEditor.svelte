<script lang="ts">
  import { onMount, onDestroy, tick } from 'svelte';
  import * as monaco from 'monaco-editor';
  import {
    X,
    Save,
    Settings,
    Pin,
    PanelRightOpen,
    PanelRightClose,
    Search,
    RotateCcw,
    XCircle,
    Eye,
    Code2,
    Columns,
    AlignLeft,
    RotateCw,
    GitCompare,
    PanelRight,
  } from 'lucide-svelte';
  import {
    fileEditorStore,
    activeFile,
    clampRectToViewport,
    SIDEBAR_TAB_W,
    langFromPath,
    type EditorDisplayMode,
    type FloatingRect,
  } from '$lib/stores/fileEditor';
  import { invoke, isTauri } from '@tauri-apps/api/core';
  import MarkdownPreview from './MarkdownPreview.svelte';
  import { isMarkdownPath } from '$lib/utils/markdown';
  import { overlayScroll } from '$lib/actions/overlayScroll';
  import { settingsStore } from '$lib/stores/settings';
  import { showContextMenu, type ContextMenuItem } from '$lib/stores/contextMenu';
  import { alertDialog } from './RidgeDialog.svelte';
  import { Copy, FolderOpen } from 'lucide-svelte';

  /** 默认 monospace 栈：用户自定义 fontFamily 留空时回退到这一串。 */
  const DEFAULT_MONO =
    '"JetBrains Mono", "Cascadia Code", "SF Mono", ui-monospace, Consolas, monospace';
  const editorFontFamily = $derived(
    $settingsStore.editorFontFamily.trim() || DEFAULT_MONO
  );
  const editorFontSize = $derived($settingsStore.editorFontSize);
  // Monaco theme follows ridge theme. light ids ('sand' / 'grass') map to 'vs',
  // dark ids ('dark' / 'soil') map to 'vs-dark'. Switching theme at runtime calls
  // monaco.editor.setTheme(...) which retints both inline and diff editors.
  const monacoTheme = $derived(
    $settingsStore.theme === 'sand' || $settingsStore.theme === 'grass'
      ? 'vs'
      : 'vs-dark'
  );
  $effect(() => {
    monaco.editor.setTheme(monacoTheme);
  });

  let mountPoint: HTMLDivElement | undefined;
  let editor: monaco.editor.IStandaloneCodeEditor | null = null;
  let currentModelPath: string | null = null;
  let settingsOpen = $state(false);
  let isResizingDrawer = $state(false);
  let isDraggingFloating = $state(false);
  let isResizingFloating = $state(false);

  // ─── Diff editor state ─────────────────────────────────────────────────────
  let diffMountPoint: HTMLDivElement | undefined;
  let diffEditor: monaco.editor.IStandaloneDiffEditor | null = null;
  let diffOriginalModel: monaco.editor.ITextModel | null = null;
  let diffModifiedModel: monaco.editor.ITextModel | null = null;
  let diffLoadedTabPath: string | null = null;
  let diffLoading = $state(false);
  let diffError = $state('');
  let diffRenderSideBySide = $state(typeof window !== 'undefined' ? window.innerWidth >= 900 : true);
  let diffReqId = 0; // generation counter to cancel stale async loads
  /**
   * Monaco's current cursor line (1-based). Drives markdown preview auto-scroll
   * (VS Code "Markdown: Preview Auto-Scroll"). Updated via
   * `onDidChangeCursorPosition`; reset to null when a non-markdown tab is
   * active so the preview ignores stale positions.
   */
  let editorCursorLine = $state<number | null>(null);

  // Tab drag-and-drop reorder state
  let draggingTabIndex = $state<number | null>(null);
  let dragOverTabIndex = $state<number | null>(null);

  let editorState = $derived($fileEditorStore);
  let current = $derived($activeFile);
  let isMarkdownFile = $derived(!!current && isMarkdownPath(current.path));
  let inPreviewMode = $derived(!!current && isMarkdownFile && current.viewMode === 'preview');
  let isImageFile = $derived(!!current && current.isImage);
  let isDiffTab = $derived(!!current?.diffArgs);

  // ─── Monaco lifecycle ──────────────────────────────────────────────────────
  // Monaco is intentionally used without web workers here (same as Pane.svelte's
  // existing editor mode). Syntax highlighting works in the main thread; language
  // server features (linting, go-to-definition) are disabled, which is fine for a
  // light-weight in-place editor.

  function disposeDiffEditor(): void {
    diffEditor?.dispose();
    diffEditor = null;
    diffOriginalModel?.dispose();
    diffModifiedModel?.dispose();
    diffOriginalModel = null;
    diffModifiedModel = null;
    diffLoadedTabPath = null;
  }

  async function loadDiff(
    args: { repoRoot: string; path: string; cached: boolean; commit?: string },
    tabPath: string
  ): Promise<void> {
    const myId = ++diffReqId;
    diffLoading = true;
    diffError = '';
    disposeDiffEditor();
    try {
      if (!isTauri()) throw new Error('需要 Tauri 环境');
      const v = args.commit
        ? await invoke<{ original: string; modified: string }>(
            'git_get_file_versions_at_commit',
            { repoRoot: args.repoRoot, path: args.path, hash: args.commit }
          )
        : await invoke<{ original: string; modified: string }>('git_get_file_versions', {
            repoRoot: args.repoRoot,
            path: args.path,
            cached: args.cached,
          });
      if (myId !== diffReqId) return;
      if (!diffMountPoint) return;
      await tick();
      if (myId !== diffReqId) return;
      const lang = langFromPath(args.path);
      diffOriginalModel = monaco.editor.createModel(v.original, lang);
      diffModifiedModel = monaco.editor.createModel(v.modified, lang);
      diffEditor = monaco.editor.createDiffEditor(diffMountPoint, {
        theme: monacoTheme,
        automaticLayout: true,
        readOnly: true,
        renderOverviewRuler: false,
        minimap: { enabled: false },
        fontFamily: editorFontFamily,
        fontSize: editorFontSize,
        renderWhitespace: 'boundary',
        scrollBeyondLastLine: false,
      });
      diffEditor.updateOptions({ renderSideBySide: diffRenderSideBySide });
      diffEditor.setModel({ original: diffOriginalModel, modified: diffModifiedModel });
      diffLoadedTabPath = tabPath;
    } catch (e) {
      if (myId !== diffReqId) return;
      diffError = e instanceof Error ? e.message : String(e);
      disposeDiffEditor();
    } finally {
      if (myId === diffReqId) diffLoading = false;
    }
  }

  onDestroy(() => {
    if (editor) {
      const model = editor.getModel();
      editor.dispose();
      if (model) model.dispose();
      editor = null;
    }
    disposeDiffEditor();
  });

  // Mount editor once the DOM node is available AND the panel is visible.
  // Skip for image files and diff tabs — they use their own display layer.
  $effect(() => {
    if (!mountPoint || !editorState.isVisible) return;
    if (editor) return;
    if (current?.isImage || current?.diffArgs) return;
    const initialValue = current?.content ?? '';
    const initialLang = current?.language ?? 'plaintext';
    editor = monaco.editor.create(mountPoint, {
      value: initialValue,
      language: initialLang,
      theme: monacoTheme,
      automaticLayout: true,
      fontFamily: editorFontFamily,
      fontSize: editorFontSize,
      minimap: { enabled: false },
      scrollBeyondLastLine: false,
      tabSize: 2,
      wordWrap: 'on',
      padding: { top: 8, bottom: 8 },
    });
    currentModelPath = current?.path ?? null;
    editor.onDidChangeModelContent(() => {
      if (!editor || !currentModelPath) return;
      const value = editor.getValue();
      fileEditorStore.updateContent(currentModelPath, value);
    });
    // Track cursor line so the markdown preview can follow in preview mode.
    editor.onDidChangeCursorPosition((ev) => {
      editorCursorLine = ev.position.lineNumber;
    });
    // Ctrl+S / Cmd+S → save
    editor.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.KeyS, () => {
      void fileEditorStore.saveActive();
    });
  });

  // Swap editor model when active tab changes.
  $effect(() => {
    const c = current; // 先读 current 建立响应式订阅
    if (!editor) return;
    // Image and diff tabs don't use the regular Monaco editor.
    if (c?.isImage || c?.diffArgs) return;
    // 所有 Monaco 调用统一包 try/catch：createModel 会懒触发 language contribution 的
    // 异步加载，失败时可能抛 Event 样式的对象，直接冒到 window 就是 “Uncaught [object Event]”。
    // 这里把异常吞在 effect 内，同时尽量把 editor 恢复到可用状态（降级 plaintext）。
    try {
      if (!c) {
        const m = monaco.editor.createModel('', 'plaintext');
        const prev = editor.getModel();
        // 先更新 path，再 setModel，避免 Monaco 在 setModel 时立刻回调 onDidChangeModelContent
        // 用着旧的 currentModelPath 把新内容错写进旧文件的记录里。
        currentModelPath = null;
        editor.setModel(m);
        if (prev) prev.dispose();
        return;
      }
      if (c.path === currentModelPath) {
        if (editor.getValue() !== c.content) {
          editor.setValue(c.content);
        }
        return;
      }
      let model: monaco.editor.ITextModel;
      try {
        model = monaco.editor.createModel(c.content, c.language);
      } catch (err) {
        console.warn(
          '[FileEditor] createModel with language failed, falling back to plaintext',
          c.language,
          err
        );
        model = monaco.editor.createModel(c.content, 'plaintext');
      }
      const prev = editor.getModel();
      currentModelPath = c.path;
      editor.setModel(model);
      // Reset cursor line state on model swap; the next cursor-position event
      // on the new model will repopulate it. Without this reset the preview
      // would briefly keep scrolling using the old file's line numbers.
      editorCursorLine = editor.getPosition()?.lineNumber ?? 1;
      if (prev) prev.dispose();
      editor.focus();
    } catch (err) {
      console.error('[FileEditor] model swap failed', err);
    }
  });

  // ─── Diff editor lifecycle ────────────────────────────────────────────────
  // Load or reload when the active tab is (or changes to) a diff tab.
  // Uses diffReqId to discard stale async results on rapid switching.
  $effect(() => {
    const c = current;
    if (!c?.diffArgs) {
      // Switched away from diff tab — dispose
      if (diffLoadedTabPath !== null) disposeDiffEditor();
      return;
    }
    if (!diffMountPoint) return;
    if (c.path === diffLoadedTabPath) return; // same diff already shown
    void loadDiff(c.diffArgs, c.path);
  });

  // 字体设置变化时，让已存在的 editor / diffEditor 实时更新（无需重建）。
  // Monaco 的 updateOptions 是幂等的，重复 set 同值无副作用。
  $effect(() => {
    const opts = { fontFamily: editorFontFamily, fontSize: editorFontSize };
    editor?.updateOptions(opts);
    diffEditor?.updateOptions(opts);
  });

  // Apply renderSideBySide toggle without a full IPC reload.
  // Monaco 在 inline ↔ sideBySide 切换时，仅 updateOptions 不会重建右侧 diff
  // widget（表面上选项已改但视觉仍是旧模式）。setModel(null) → setModel(real)
  // 的 null-cycle 强制 Monaco 彻底销毁并重建内部 sub-editor，确保立即以新模式渲染。
  $effect(() => {
    if (!diffEditor) return;
    diffEditor.updateOptions({ renderSideBySide: diffRenderSideBySide });
    const orig = diffOriginalModel;
    const mod = diffModifiedModel;
    if (orig && mod) {
      diffEditor.setModel(null);
      diffEditor.setModel({ original: orig, modified: mod });
    }
    diffEditor.layout();
  });

  // When switching back to a diff tab, visibility changes from hidden→visible.
  // Because visibility:hidden doesn't affect element size, automaticLayout
  // doesn't detect the change. Force layout after the DOM settles.
  $effect(() => {
    if (isDiffTab && diffEditor) {
      void tick().then(() => diffEditor?.layout());
    }
  });

  // One-shot reveal: when a caller opened this file with a `line/column`
  // (e.g. a search result), drive Monaco to that position AFTER the model
  // swap effect above has run. We run in a separate $effect so this doesn't
  // interlock with the model-swap try/catch; if Monaco is still setting up
  // its language contributions, `reveal*` is safe to call anyway.
  $effect(() => {
    const c = current;
    if (!editor || !c) return;
    // Only consume when the active model matches; otherwise model swap will
    // fire this effect a second time when currentModelPath catches up.
    if (currentModelPath !== c.path) return;
    const r = fileEditorStore.consumePendingReveal(c.path);
    if (!r) return;
    try {
      editor.revealLineInCenter(r.line);
      editor.setPosition({ lineNumber: r.line, column: Math.max(1, r.column) });
      editor.focus();
    } catch (err) {
      console.warn('[FileEditor] reveal failed', err);
    }
  });

  // ─── Tab actions ───────────────────────────────────────────────────────────
  function activateTab(path: string) {
    fileEditorStore.setActive(path);
  }
  async function closeTab(e: MouseEvent, path: string) {
    e.stopPropagation();
    await fileEditorStore.closeFile(path);
  }

  /** Tab 上下文菜单：对标 VS Code 的关闭族 + 路径族 + 资源管理器入口。
   *  diff tab 没有真实磁盘路径，"显示在资源管理器"等动作要禁用。 */
  function onTabContextMenu(e: MouseEvent, path: string): void {
    e.preventDefault();
    e.stopPropagation();
    const file = editorState.openFiles.find((f) => f.path === path);
    if (!file) return;
    const isDiff = !!file.diffArgs;
    const idx = editorState.openFiles.findIndex((f) => f.path === path);
    const hasRight = idx >= 0 && idx < editorState.openFiles.length - 1;
    const hasOthers = editorState.openFiles.length > 1;
    const hasSaved = editorState.openFiles.some((f) => !f.isDirty && !f.diffArgs);

    const copyToClipboard = async (text: string, label: string) => {
      try {
        if (!navigator.clipboard?.writeText) throw new Error('clipboard API unavailable');
        await navigator.clipboard.writeText(text);
      } catch (err) {
        await alertDialog({ title: '复制失败', message: `${label}: ${err}`, danger: true });
      }
    };

    const items: ContextMenuItem[] = [
      {
        id: 'close',
        label: '关闭',
        shortcut: 'Ctrl+W',
        icon: X,
        action: () => void fileEditorStore.closeFile(path),
      },
      {
        id: 'close-others',
        label: '关闭其他',
        disabled: !hasOthers,
        action: () => void fileEditorStore.closeOthers(path),
      },
      {
        id: 'close-right',
        label: '关闭右侧',
        disabled: !hasRight,
        action: () => void fileEditorStore.closeToRight(path),
      },
      {
        id: 'close-saved',
        label: '关闭已保存',
        disabled: !hasSaved,
        action: () => fileEditorStore.closeSaved(),
      },
      {
        id: 'close-all',
        label: '关闭全部',
        action: () => void fileEditorStore.closeAll(),
      },
      { id: 'div1', divider: true },
      {
        id: 'copy-path',
        label: '复制路径',
        icon: Copy,
        disabled: isDiff,
        action: () => void copyToClipboard(path, '复制路径'),
      },
      {
        id: 'copy-name',
        label: '复制文件名',
        icon: Copy,
        disabled: isDiff,
        action: () => void copyToClipboard(file.name, '复制文件名'),
      },
      { id: 'div2', divider: true },
      {
        id: 'reveal',
        label: '在文件资源管理器中显示',
        icon: FolderOpen,
        disabled: isDiff || !isTauri(),
        action: () => {
          if (!isTauri()) return;
          void invoke('reveal_in_file_manager', { path }).catch(async (err) => {
            await alertDialog({ title: '打开失败', message: String(err), danger: true });
          });
        },
      },
    ];
    showContextMenu(e.clientX, e.clientY, items, 'editor');
  }

  // ─── Tab drag-reorder (HTML5 DnD, same pattern as WorkspaceTabs) ──────────
  function onTabDragStart(e: DragEvent, index: number) {
    draggingTabIndex = index;
    if (e.dataTransfer) {
      e.dataTransfer.effectAllowed = 'move';
      e.dataTransfer.setData('text/plain', String(index));
    }
  }
  function onTabDragOver(e: DragEvent, index: number) {
    e.preventDefault();
    dragOverTabIndex = index;
  }
  function onTabDragLeave() {
    dragOverTabIndex = null;
  }
  function onTabDrop(e: DragEvent, toIndex: number) {
    e.preventDefault();
    if (draggingTabIndex !== null && draggingTabIndex !== toIndex) {
      fileEditorStore.reorder(draggingTabIndex, toIndex);
    }
    draggingTabIndex = null;
    dragOverTabIndex = null;
  }
  function onTabDragEnd() {
    draggingTabIndex = null;
    dragOverTabIndex = null;
  }
  function setMode(mode: EditorDisplayMode) {
    fileEditorStore.setDisplayMode(mode);
    settingsOpen = false;
  }
  async function closeAll() {
    settingsOpen = false;
    await fileEditorStore.closeAll();
  }
  async function revertActive() {
    settingsOpen = false;
    await fileEditorStore.revertActive();
  }
  function hidePanel() {
    fileEditorStore.hide();
  }
  function triggerFind() {
    editor?.getAction('actions.find')?.run();
  }

  // ─── Drawer resize (left edge) ─────────────────────────────────────────────
  function onDrawerResizeStart(e: MouseEvent) {
    e.preventDefault();
    isResizingDrawer = true;
    const startX = e.clientX;
    const startW = editorState.drawerWidth;
    const onMove = (ev: MouseEvent) => {
      const delta = startX - ev.clientX;
      fileEditorStore.setDrawerWidth(startW + delta);
    };
    const onUp = () => {
      isResizingDrawer = false;
      window.removeEventListener('mousemove', onMove);
      window.removeEventListener('mouseup', onUp);
    };
    window.addEventListener('mousemove', onMove);
    window.addEventListener('mouseup', onUp);
  }

  // ─── Floating drag (by title bar) ──────────────────────────────────────────
  function onFloatingDragStart(e: MouseEvent) {
    if (editorState.displayMode !== 'floating') return;
    if ((e.target as HTMLElement).closest('button, select, input')) return;
    e.preventDefault();
    isDraggingFloating = true;
    const startX = e.clientX;
    const startY = e.clientY;
    const startRect = { ...editorState.floatingRect };
    const onMove = (ev: MouseEvent) => {
      const rect: FloatingRect = {
        x: startRect.x + (ev.clientX - startX),
        y: startRect.y + (ev.clientY - startY),
        w: startRect.w,
        h: startRect.h,
      };
      fileEditorStore.setFloatingRect(rect);
    };
    const onUp = () => {
      isDraggingFloating = false;
      window.removeEventListener('mousemove', onMove);
      window.removeEventListener('mouseup', onUp);
    };
    window.addEventListener('mousemove', onMove);
    window.addEventListener('mouseup', onUp);
  }

  // ─── Floating resize (8 handles) ───────────────────────────────────────────
  type HandleDir = 'n' | 's' | 'e' | 'w' | 'ne' | 'nw' | 'se' | 'sw';
  function onFloatingResizeStart(e: MouseEvent, dir: HandleDir) {
    e.preventDefault();
    e.stopPropagation();
    isResizingFloating = true;
    const startX = e.clientX;
    const startY = e.clientY;
    const startRect = { ...editorState.floatingRect };
    const onMove = (ev: MouseEvent) => {
      const dx = ev.clientX - startX;
      const dy = ev.clientY - startY;
      let { x, y, w, h } = startRect;
      if (dir.includes('e')) w = startRect.w + dx;
      if (dir.includes('s')) h = startRect.h + dy;
      if (dir.includes('w')) {
        x = startRect.x + dx;
        w = startRect.w - dx;
      }
      if (dir.includes('n')) {
        y = startRect.y + dy;
        h = startRect.h - dy;
      }
      fileEditorStore.setFloatingRect({ x, y, w, h });
    };
    const onUp = () => {
      isResizingFloating = false;
      window.removeEventListener('mousemove', onMove);
      window.removeEventListener('mouseup', onUp);
    };
    window.addEventListener('mousemove', onMove);
    window.addEventListener('mouseup', onUp);
  }

  // Reclamp floating rect on viewport resize so the panel doesn't end up off-screen.
  function onWindowResize() {
    if (editorState.displayMode !== 'floating') return;
    fileEditorStore.setFloatingRect(
      clampRectToViewport(editorState.floatingRect)
    );
  }

  /**
   * Keyboard-driven resize for the floating window. Arrow keys nudge the
   * corresponding edge by 16 px (64 px with Shift). Mirrors the mouse-drag
   * `onFloatingResizeStart` contract for a single step — no drag loop.
   * Called from each resize handle's `onkeydown` so Svelte's a11y linter is
   * happy (keyboard handler paired with mouse handler).
   */
  function onFloatingResizeKey(e: KeyboardEvent, dir: HandleDir) {
    if (editorState.displayMode !== 'floating') return;
    if (
      e.key !== 'ArrowUp' &&
      e.key !== 'ArrowDown' &&
      e.key !== 'ArrowLeft' &&
      e.key !== 'ArrowRight'
    )
      return;
    const step = e.shiftKey ? 64 : 16;
    const rect = { ...editorState.floatingRect };
    let dx = 0;
    let dy = 0;
    if (e.key === 'ArrowUp') dy = -step;
    if (e.key === 'ArrowDown') dy = step;
    if (e.key === 'ArrowLeft') dx = -step;
    if (e.key === 'ArrowRight') dx = step;
    let { x, y, w, h } = rect;
    if (dir.includes('e')) w = rect.w + dx;
    if (dir.includes('s')) h = rect.h + dy;
    if (dir.includes('w')) {
      x = rect.x + dx;
      w = rect.w - dx;
    }
    if (dir.includes('n')) {
      y = rect.y + dy;
      h = rect.h - dy;
    }
    e.preventDefault();
    fileEditorStore.setFloatingRect({ x, y, w, h });
  }
  // Monaco / AMD loader 偶发把异步加载失败以 Event 对象形式扔到 unhandledrejection，
  // 表现成 "Uncaught [object Event]"。这里接住并降级为 warn，不影响编辑器主流程。
  function onUnhandledRejection(e: PromiseRejectionEvent) {
    const reason = e.reason;
    if (
      reason instanceof Event ||
      (reason &&
        typeof reason === 'object' &&
        'type' in reason &&
        'target' in reason)
    ) {
      console.warn('[FileEditor] swallowed Event-shaped rejection', reason);
      e.preventDefault();
    }
  }
  onMount(() => {
    window.addEventListener('resize', onWindowResize);
    window.addEventListener('unhandledrejection', onUnhandledRejection);
    return () => {
      window.removeEventListener('resize', onWindowResize);
      window.removeEventListener('unhandledrejection', onUnhandledRejection);
    };
  });

  // Close settings dropdown on outside click
  function onDocClick(e: MouseEvent) {
    if (!settingsOpen) return;
    const t = e.target as HTMLElement;
    if (!t.closest('.rg-editor-settings')) settingsOpen = false;
  }
  onMount(() => {
    document.addEventListener('mousedown', onDocClick, true);
    return () => document.removeEventListener('mousedown', onDocClick, true);
  });

  // ─── Style computations ────────────────────────────────────────────────────
  const containerStyle = $derived.by(() => {
    if (!editorState.isVisible || editorState.openFiles.length === 0)
      return 'display: none;';
    if (editorState.displayMode === 'floating') {
      const r = editorState.floatingRect;
      return `position: fixed; left: ${r.x}px; top: ${r.y}px; width: ${r.w}px; height: ${r.h}px; z-index: 60;`;
    }
    if (editorState.displayMode === 'embedded') {
      // Embedded: part of the normal flex layout — no position:fixed.
      // Width driven by drawerWidth (shared with drawer mode / resizable).
      return `width: ${editorState.drawerWidth}px; flex-shrink: 0;`;
    }
    // drawer: anchored to the right, **below the 44px header bar** so the
    // titlebar + workspace tabs remain visible/interactive (用户反馈：抽屉不能遮挡顶部 header)。
    const TOP_OFFSET = 44;
    return `position: fixed; top: ${TOP_OFFSET}px; right: 0; bottom: 0; width: ${editorState.drawerWidth}px; z-index: 40;`;
  });
</script>

<div
  class="rg-file-editor flex flex-col bg-[var(--rg-surface-2)]/98 backdrop-blur-xl border border-[var(--rg-border)] shadow-2xl {editorState.displayMode === 'floating'
    ? 'rounded-lg overflow-hidden'
    : editorState.displayMode === 'drawer'
      ? 'rounded-l-lg'
      : ''}"
  style={containerStyle}
>
  <!-- ═══ Header (tabs + actions) ═══ -->
  <!-- toolbar 角色要求 tabindex 以便键盘用户 Tab 进入后用内部 Tab 遍历按钮。 -->
  <div
    class="flex items-center shrink-0 h-9 border-b border-[var(--rg-border)] bg-[var(--rg-surface)]/90 {editorState.displayMode ===
    'floating'
      ? 'cursor-grab active:cursor-grabbing'
      : ''}"
    role="toolbar"
    tabindex="-1"
    aria-label="编辑器工具栏"
    onmousedown={editorState.displayMode === 'floating'
      ? onFloatingDragStart
      : undefined}
  >
    {#if editorState.displayMode === 'drawer' || editorState.displayMode === 'embedded'}
      <button
        type="button"
        class="rg-no-drag flex h-9 w-8 shrink-0 items-center justify-center text-[var(--rg-fg-muted)] hover:bg-[var(--rg-surface)] hover:text-[var(--rg-fg)] transition-colors border-r border-[var(--rg-border)]"
        title="收起编辑器面板"
        onmousedown={(e) => e.stopPropagation()}
        onclick={hidePanel}
      >
        <PanelRightClose class="h-3.5 w-3.5" />
      </button>
    {/if}
    <!-- Tabs: pure CSS horizontal scroll, no gutter, wheel handler in action -->
    <div class="flex-1 min-w-0" use:overlayScroll={{ preset: 'horizontal-tabs' }}>
      {#each editorState.openFiles as f, i (f.path)}
        <button
          type="button"
          class="group flex items-center gap-1.5 h-9 pl-3 pr-1.5 text-[12px] shrink-0 border-r border-[var(--rg-border)] transition-colors cursor-grab active:cursor-grabbing {editorState.activePath ===
          f.path
            ? 'bg-[var(--rg-bg-raised)] text-[var(--rg-fg)]'
            : 'text-[var(--rg-fg-muted)] hover:bg-[var(--rg-surface)]/60 hover:text-[var(--rg-fg)]'}
              {draggingTabIndex === i ? 'opacity-50' : ''}
              {dragOverTabIndex === i &&
          draggingTabIndex !== null &&
          draggingTabIndex !== i
            ? 'ring-1 ring-[var(--rg-accent)]/60 ring-inset'
            : ''}"
          onclick={() => activateTab(f.path)}
          oncontextmenu={(e) => onTabContextMenu(e, f.path)}
          title={f.path}
          draggable="true"
          ondragstart={(e) => onTabDragStart(e, i)}
          ondragover={(e) => onTabDragOver(e, i)}
          ondragleave={onTabDragLeave}
          ondrop={(e) => onTabDrop(e, i)}
          ondragend={onTabDragEnd}
        >
          {#if f.diffArgs}
            <GitCompare class="h-3 w-3 shrink-0 text-[var(--rg-accent)]/70" />
          {/if}
          <span
            class="truncate max-w-[160px] {f.external === 'deleted'
              ? 'text-red-500 line-through decoration-red-500/70'
              : ''}"
            title={f.external === 'deleted' ? `${f.name} 已被外部删除` : f.name}
          >{f.name}</span>
          {#if f.external === 'deleted'}
            <span
              class="text-[10px] px-1 py-px rounded bg-red-500/15 text-red-500 leading-none"
              title="文件已被外部删除"
            >已删除</span>
          {/if}
          {#if f.isDirty}
            <span
              class="inline-block h-1.5 w-1.5 rounded-full bg-[var(--rg-accent)]"
              title="未保存"
            ></span>
          {/if}
          <span
            role="button"
            tabindex="0"
            class="flex h-4 w-4 items-center justify-center rounded text-[var(--rg-fg-muted)] hover:bg-[var(--rg-bg)]/50 hover:text-[var(--rg-fg)] {f.isDirty
              ? ''
              : 'opacity-0 group-hover:opacity-100'} transition-opacity"
            onclick={(e) => closeTab(e, f.path)}
            onkeydown={(e) =>
              (e.key === 'Enter' || e.key === ' ') &&
              closeTab(e as unknown as MouseEvent, f.path)}
            title="关闭"
          >
            <X class="h-3 w-3" />
          </span>
        </button>
      {/each}
    </div>

    <!-- Right-side actions -->
    <div
      class="flex ml-auto items-center gap-0.5 px-1 shrink-0 border-l border-[var(--rg-border)]"
    >
      {#if isDiffTab}
        <!-- Diff-specific controls: render mode toggle + reload -->
        <div class="flex items-center border border-[var(--rg-border)] rounded mr-0.5">
          <button
            type="button"
            class="flex h-6 w-7 items-center justify-center text-[var(--rg-fg-muted)] hover:bg-[var(--rg-surface)] hover:text-[var(--rg-fg)] transition-colors {diffRenderSideBySide ? 'bg-[var(--rg-accent)]/20 text-[var(--rg-accent)]' : ''}"
            title="并排 diff"
            onclick={() => (diffRenderSideBySide = true)}
          >
            <Columns class="h-3.5 w-3.5" />
          </button>
          <button
            type="button"
            class="flex h-6 w-7 items-center justify-center text-[var(--rg-fg-muted)] hover:bg-[var(--rg-surface)] hover:text-[var(--rg-fg)] transition-colors {!diffRenderSideBySide ? 'bg-[var(--rg-accent)]/20 text-[var(--rg-accent)]' : ''}"
            title="内联 diff"
            onclick={() => (diffRenderSideBySide = false)}
          >
            <AlignLeft class="h-3.5 w-3.5" />
          </button>
        </div>
        <button
          type="button"
          class="flex h-7 w-7 items-center justify-center rounded text-[var(--rg-fg-muted)] hover:bg-[var(--rg-surface)] hover:text-[var(--rg-fg)] transition-colors"
          title="重新加载 diff"
          onclick={() => { if (current?.diffArgs) void loadDiff(current.diffArgs, current.path); }}
        >
          <RotateCw class="h-3.5 w-3.5 {diffLoading ? 'animate-spin' : ''}" />
        </button>
      {:else}
        <button
          type="button"
          class="flex h-7 w-7 items-center justify-center rounded text-[var(--rg-fg-muted)] hover:bg-[var(--rg-surface)] hover:text-[var(--rg-fg)] transition-colors disabled:opacity-30 disabled:cursor-not-allowed"
          title="查找 (Ctrl+F)"
          disabled={!current}
          onclick={triggerFind}
        >
          <Search class="h-3.5 w-3.5" />
        </button>
        <button
          type="button"
          class="flex h-7 w-7 items-center justify-center rounded text-[var(--rg-fg-muted)] hover:bg-[var(--rg-surface)] hover:text-[var(--rg-fg)] transition-colors disabled:opacity-30 disabled:cursor-not-allowed"
          title="保存 (Ctrl+S)"
          disabled={!current?.isDirty}
          onclick={() => fileEditorStore.saveActive()}
        >
          <Save class="h-3.5 w-3.5" />
        </button>
      {/if}

      <div class="rg-editor-settings relative">
        <button
          type="button"
          class="flex h-7 w-7 items-center justify-center rounded text-[var(--rg-fg-muted)] hover:bg-[var(--rg-surface)] hover:text-[var(--rg-fg)] transition-colors {settingsOpen
            ? 'bg-[var(--rg-surface)] text-[var(--rg-fg)]'
            : ''}"
          title="设置"
          onclick={() => (settingsOpen = !settingsOpen)}
        >
          <Settings class="h-3.5 w-3.5" />
        </button>
        {#if settingsOpen}
          <div
            class="absolute right-0 top-9 w-56 rounded-lg bg-[var(--rg-surface-2)] border border-[var(--rg-border)] shadow-xl z-10 py-1 text-[12px]"
          >
            <div
              class="px-3 py-1 text-[10px] uppercase tracking-wider text-[var(--rg-fg-muted)]"
            >
              显示模式
            </div>
            <button
              type="button"
              class="w-full flex items-center gap-2 px-3 py-1.5 text-left hover:bg-[var(--rg-surface)] transition-colors {editorState.displayMode ===
              'embedded'
                ? 'text-[var(--rg-accent)]'
                : 'text-[var(--rg-fg)]'}"
              onclick={() => setMode('embedded')}
            >
              <PanelRight class="h-3.5 w-3.5" /> 嵌入模式
              {#if editorState.displayMode === 'embedded'}<span
                  class="ml-auto text-[10px]">✓</span
                >{/if}
            </button>
            <button
              type="button"
              class="w-full flex items-center gap-2 px-3 py-1.5 text-left hover:bg-[var(--rg-surface)] transition-colors {editorState.displayMode ===
              'drawer'
                ? 'text-[var(--rg-accent)]'
                : 'text-[var(--rg-fg)]'}"
              onclick={() => setMode('drawer')}
            >
              <PanelRightOpen class="h-3.5 w-3.5" /> 抽屉模式
              {#if editorState.displayMode === 'drawer'}<span
                  class="ml-auto text-[10px]">✓</span
                >{/if}
            </button>
            <button
              type="button"
              class="w-full flex items-center gap-2 px-3 py-1.5 text-left hover:bg-[var(--rg-surface)] transition-colors {editorState.displayMode ===
              'floating'
                ? 'text-[var(--rg-accent)]'
                : 'text-[var(--rg-fg)]'}"
              onclick={() => setMode('floating')}
            >
              <Pin class="h-3.5 w-3.5" /> 悬浮 Pin 模式
              {#if editorState.displayMode === 'floating'}<span
                  class="ml-auto text-[10px]">✓</span
                >{/if}
            </button>

            <div class="my-1 border-t border-[var(--rg-border)]"></div>
            <button
              type="button"
              class="w-full flex items-center gap-2 px-3 py-1.5 text-left hover:bg-[var(--rg-surface)] transition-colors text-[var(--rg-fg)] disabled:opacity-30 disabled:cursor-not-allowed"
              disabled={!current?.isDirty}
              onclick={() => {
                revertActive();
              }}
            >
              <RotateCcw class="h-3.5 w-3.5" /> 放弃修改（重新从磁盘加载）
            </button>
            <button
              type="button"
              class="w-full flex items-center gap-2 px-3 py-1.5 text-left hover:bg-[var(--rg-surface)] transition-colors text-[var(--rg-fg)]"
              onclick={() => closeAll()}
            >
              <XCircle class="h-3.5 w-3.5" /> 关闭全部标签
            </button>
            <button
              type="button"
              class="w-full flex items-center gap-2 px-3 py-1.5 text-left hover:bg-[var(--rg-surface)] transition-colors text-[var(--rg-fg-muted)]"
              onclick={() => {
                settingsOpen = false;
                hidePanel();
              }}
            >
              隐藏编辑器面板
            </button>
          </div>
        {/if}
      </div>
      {#if editorState.displayMode === 'floating'}
        <!-- pin 模式专属关闭按钮：drawer 模式下左侧已经有收起按钮，这里
               重复一次反而冗余；只有 floating 时为了贴合标准窗口的"关闭在
               右上角"心智模型才显示。功能与左侧收起一致——隐藏面板，
               不销毁打开的文件。 -->
        <button
          type="button"
          class="rg-no-drag flex h-7 w-7 items-center justify-center rounded text-[var(--rg-fg-muted)] hover:bg-red-500/10 hover:text-red-300 transition-colors"
          title="关闭浮动窗口（保留打开的文件）"
          onmousedown={(e) => e.stopPropagation()}
          onclick={hidePanel}
        >
          <X class="h-3.5 w-3.5" />
        </button>
      {/if}
    </div>
  </div>

  <!-- ═══ Monaco host ═══ -->
  <div class="flex-1 min-h-0 relative">
    <!-- Regular editor — hidden when showing diff or markdown preview -->
    <div
      bind:this={mountPoint}
      class="absolute inset-0"
      style={inPreviewMode || isDiffTab ? 'visibility: hidden;' : ''}
    ></div>

    <!-- Diff editor mount point — always in DOM so bind:this is stable -->
    <div
      bind:this={diffMountPoint}
      class="absolute inset-0"
      style={!isDiffTab || !!diffError ? 'visibility: hidden;' : ''}
    ></div>

    <!-- Diff loading / error overlays -->
    {#if isDiffTab && diffLoading}
      <div class="absolute top-2 right-3 text-[10px] text-[var(--rg-fg-muted)] bg-[var(--rg-surface)]/80 px-2 py-0.5 rounded pointer-events-none">
        加载中…
      </div>
    {/if}
    {#if isDiffTab && diffError}
      <div class="absolute inset-0 flex items-center justify-center p-6">
        <div class="max-w-[420px] text-center text-[12px] text-rose-300 bg-rose-500/10 border border-rose-500/30 rounded p-3 font-mono whitespace-pre-wrap">
          {diffError}
        </div>
      </div>
    {/if}

    {#if current && isMarkdownFile && inPreviewMode}
      <!-- The previous `use:overlayScroll` host was `absolute inset-0`,
             which broke wheel scrolling under overlayscrollbars: the
             synthetic viewport injected by the lib didn't get a stable
             height with absolute positioning. Switch to native
             `overflow-y-auto` + `rg-scroll` styling so the native
             scroller drives wheel events deterministically. The user
             sees a thin transparent bar matching the rest of the app
             (defined in app.css) without the overlayscrollbars layer. -->
      <div
        class="absolute inset-0 bg-[var(--rg-bg-raised)] overflow-y-auto overflow-x-hidden rg-scroll"
      >
        <MarkdownPreview
          content={current.content}
          basePath={current.path.replace(/[\\/][^\\/]+$/, '')}
          cursorLine={editorCursorLine}
          onChange={(next) =>
            fileEditorStore.updateContent(current!.path, next)}
          onRequestEdit={() =>
            fileEditorStore.setViewMode(current!.path, 'source')}
          onRevealSource={(line) => {
            // Alt-click on preview block: scroll Monaco to the source line
            // without leaving preview mode. Switch to source only if user
            // also wants editing (they can click again or use the toggle).
            if (!editor) return;
            const targetLine = Math.max(1, line + 1); // data-rg-md-src-line is 0-based
            editor.revealLineInCenter(targetLine);
            editor.setPosition({ lineNumber: targetLine, column: 1 });
          }}
        />
      </div>
    {/if}

<!-- 图片预览 -->
{#if current && isImageFile && current.imageUrl}
<div class="absolute inset-0 flex items-center justify-center bg-[var(--rg-bg-raised)] overflow-auto p-4">
  <img
    src={current.imageUrl}
    alt={current.name}
    class="max-w-full max-h-full object-contain rounded-lg shadow-lg"
  />
</div>
{/if}

    <!-- Preview ↔ Source 切换按钮：右上角浮动 pill，半透明玻璃态。
           仅对 markdown 文件渲染；悬停收到正式 accent，保证不抢主内容视觉重量。 -->
    {#if current && isMarkdownFile}
      <button
        type="button"
        class="absolute top-2.5 right-3 z-10 flex items-center gap-1.5 h-7 pl-2 pr-2.5 rounded-full text-[11px] font-medium
                 bg-[var(--rg-surface)]/60 backdrop-blur-md border border-[var(--rg-border)]
                 text-[var(--rg-fg-muted)] hover:text-[var(--rg-fg)] hover:bg-[var(--rg-surface)]/85 hover:border-[var(--rg-accent)]/40
                 transition-colors shadow-lg shadow-black/20"
        title={inPreviewMode
          ? '切换到源码编辑 (Markdown)'
          : '切换到预览 (Markdown)'}
        onclick={() =>
          fileEditorStore.setViewMode(
            current!.path,
            inPreviewMode ? 'source' : 'preview'
          )}
      >
        {#if inPreviewMode}
          <Code2 class="h-3.5 w-3.5" />
          <span>源码</span>
        {:else}
          <Eye class="h-3.5 w-3.5" />
          <span>预览</span>
        {/if}
      </button>
    {/if}
  </div>

  <!-- ═══ Status bar ═══ -->
  {#if current}
    <div
      class="shrink-0 h-6 flex items-center gap-2 px-3 text-[10px] text-[var(--rg-fg-muted)] border-t border-[var(--rg-border)] bg-[var(--rg-surface)]/70 font-mono"
    >
      <span class="truncate flex-1" title={current.path}>{current.path}</span>
      <span>{current.language}</span>
      {#if current.isDirty}
        <span class="text-[var(--rg-accent)]">● 未保存</span>
      {:else}
        <span>已保存</span>
      {/if}
    </div>
  {/if}

  <!-- ═══ Drawer / Embedded left-edge resizer ═══ -->
  {#if editorState.displayMode === 'drawer' || editorState.displayMode === 'embedded'}
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <div
      class="absolute left-0 top-0 bottom-0 w-1 cursor-col-resize hover:bg-[var(--rg-accent)]/40 transition-colors {isResizingDrawer
        ? 'bg-[var(--rg-accent)]/60'
        : ''}"
      role="separator"
      aria-orientation="vertical"
      aria-label="调整编辑器宽度"
      onmousedown={onDrawerResizeStart}
    ></div>
  {/if}

  <!-- ═══ Floating resize handles ═══
         tabindex=0 + onkeydown 让键盘用户也能通过 Arrow 键调整大小（Shift 加速）。
         Svelte 的 `a11y_no_noninteractive_*` 规则不认识 role=separator + 互补
         keydown 这个合法的 "window splitter" 模式，所以为每个 handle 显式抑制。
         参考 WAI-ARIA authoring practices: separator 可聚焦并响应 Arrow 键。 -->
  {#if editorState.displayMode === 'floating'}
    <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <div
      class="rg-float-handle rg-h-n"
      role="separator"
      aria-label="从上边调整"
      tabindex="0"
      onmousedown={(e) => onFloatingResizeStart(e, 'n')}
      onkeydown={(e) => onFloatingResizeKey(e, 'n')}
    ></div>
    <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <div
      class="rg-float-handle rg-h-s"
      role="separator"
      aria-label="从下边调整"
      tabindex="0"
      onmousedown={(e) => onFloatingResizeStart(e, 's')}
      onkeydown={(e) => onFloatingResizeKey(e, 's')}
    ></div>
    <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <div
      class="rg-float-handle rg-h-e"
      role="separator"
      aria-label="从右边调整"
      tabindex="0"
      onmousedown={(e) => onFloatingResizeStart(e, 'e')}
      onkeydown={(e) => onFloatingResizeKey(e, 'e')}
    ></div>
    <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <div
      class="rg-float-handle rg-h-w"
      role="separator"
      aria-label="从左边调整"
      tabindex="0"
      onmousedown={(e) => onFloatingResizeStart(e, 'w')}
      onkeydown={(e) => onFloatingResizeKey(e, 'w')}
    ></div>
    <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <div
      class="rg-float-handle rg-h-ne"
      role="separator"
      aria-label="右上"
      tabindex="0"
      onmousedown={(e) => onFloatingResizeStart(e, 'ne')}
      onkeydown={(e) => onFloatingResizeKey(e, 'ne')}
    ></div>
    <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <div
      class="rg-float-handle rg-h-nw"
      role="separator"
      aria-label="左上"
      tabindex="0"
      onmousedown={(e) => onFloatingResizeStart(e, 'nw')}
      onkeydown={(e) => onFloatingResizeKey(e, 'nw')}
    ></div>
    <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <div
      class="rg-float-handle rg-h-se"
      role="separator"
      aria-label="右下"
      tabindex="0"
      onmousedown={(e) => onFloatingResizeStart(e, 'se')}
      onkeydown={(e) => onFloatingResizeKey(e, 'se')}
    ></div>
    <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <div
      class="rg-float-handle rg-h-sw"
      role="separator"
      aria-label="左下"
      tabindex="0"
      onmousedown={(e) => onFloatingResizeStart(e, 'sw')}
      onkeydown={(e) => onFloatingResizeKey(e, 'sw')}
    ></div>
  {/if}
</div>

<style>
  /* Floating resize handles — small grab zones extending slightly outside the box */
  .rg-float-handle {
    position: absolute;
    z-index: 2;
  }
  .rg-h-n {
    top: -3px;
    left: 8px;
    right: 8px;
    height: 6px;
    cursor: ns-resize;
  }
  .rg-h-s {
    bottom: -3px;
    left: 8px;
    right: 8px;
    height: 6px;
    cursor: ns-resize;
  }
  .rg-h-w {
    top: 8px;
    bottom: 8px;
    left: -3px;
    width: 6px;
    cursor: ew-resize;
  }
  .rg-h-e {
    top: 8px;
    bottom: 8px;
    right: -3px;
    width: 6px;
    cursor: ew-resize;
  }
  .rg-h-nw {
    top: -3px;
    left: -3px;
    width: 10px;
    height: 10px;
    cursor: nwse-resize;
  }
  .rg-h-ne {
    top: -3px;
    right: -3px;
    width: 10px;
    height: 10px;
    cursor: nesw-resize;
  }
  .rg-h-sw {
    bottom: -3px;
    left: -3px;
    width: 10px;
    height: 10px;
    cursor: nesw-resize;
  }
  .rg-h-se {
    bottom: -3px;
    right: -3px;
    width: 10px;
    height: 10px;
    cursor: nwse-resize;
  }
</style>
