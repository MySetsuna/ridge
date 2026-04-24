<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import * as monaco from 'monaco-editor';
  import {
    X,
    Save,
    Settings,
    Pin,
    PanelRightOpen,
    Search,
    RotateCcw,
    XCircle,
    Eye,
    Code2,
  } from 'lucide-svelte';
  import {
    fileEditorStore,
    activeFile,
    clampRectToViewport,
    SIDEBAR_TAB_W,
    type EditorDisplayMode,
    type FloatingRect,
  } from '$lib/stores/fileEditor';
  import MarkdownPreview from './MarkdownPreview.svelte';
  import { isMarkdownPath } from '$lib/utils/markdown';

  let mountPoint: HTMLDivElement | undefined;
  let editor: monaco.editor.IStandaloneCodeEditor | null = null;
  let currentModelPath: string | null = null;
  let settingsOpen = $state(false);
  let isResizingDrawer = $state(false);
  let isDraggingFloating = $state(false);
  let isResizingFloating = $state(false);

  // Tab drag-and-drop reorder state
  let draggingTabIndex = $state<number | null>(null);
  let dragOverTabIndex = $state<number | null>(null);

  let editorState = $derived($fileEditorStore);
  let current = $derived($activeFile);
  // markdown 文件在 preview 模式下不挂 Monaco；切回 source 才实例化/恢复 model。
  let isMarkdownFile = $derived(!!current && isMarkdownPath(current.path));
  let inPreviewMode = $derived(!!current && isMarkdownFile && current.viewMode === 'preview');

  // ─── Monaco lifecycle ──────────────────────────────────────────────────────
  // Monaco is intentionally used without web workers here (same as Pane.svelte's
  // existing editor mode). Syntax highlighting works in the main thread; language
  // server features (linting, go-to-definition) are disabled, which is fine for a
  // light-weight in-place editor.

  onDestroy(() => {
    if (editor) {
      const model = editor.getModel();
      editor.dispose();
      if (model) model.dispose();
      editor = null;
    }
  });

  // Mount editor once the DOM node is available AND the panel is visible.
  $effect(() => {
    if (!mountPoint || !editorState.isVisible) return;
    if (editor) return;
    const initialValue = current?.content ?? '';
    const initialLang = current?.language ?? 'plaintext';
    editor = monaco.editor.create(mountPoint, {
      value: initialValue,
      language: initialLang,
      theme: 'vs-dark',
      automaticLayout: true,
      fontFamily: '"JetBrains Mono", "Cascadia Code", "SF Mono", ui-monospace, Consolas, monospace',
      fontSize: 13,
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
    // Ctrl+S / Cmd+S → save
    editor.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.KeyS, () => {
      void fileEditorStore.saveActive();
    });
  });

  // Swap editor model when active tab changes.
  //
  // ✱ 关键：必须**先读 `current`** 再检查 `editor`。Svelte 5 $effect 只对**实际执行到**
  //   的 reactive reads 建立订阅。如果第一次运行时 editor 尚为 null 直接 return，就会
  //   错过对 `current` 的订阅，导致后续切 Tab / 打开新文件时 effect 不再重跑 ——
  //   表现为：第二个及以后打开的文件 Monaco 完全没反应（也就无法编辑/保存，
  //   因为 onDidChangeModelContent 依然绑在第一个 model 上）。
  $effect(() => {
    const c = current; // 先订阅当前活动文件
    if (!editor) return;
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
        console.warn('[FileEditor] createModel with language failed, falling back to plaintext', c.language, err);
        model = monaco.editor.createModel(c.content, 'plaintext');
      }
      const prev = editor.getModel();
      currentModelPath = c.path;
      editor.setModel(model);
      if (prev) prev.dispose();
      editor.focus();
    } catch (err) {
      console.error('[FileEditor] model swap failed', err);
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
    fileEditorStore.setFloatingRect(clampRectToViewport(editorState.floatingRect));
  }
  // Monaco / AMD loader 偶发把异步加载失败以 Event 对象形式扔到 unhandledrejection，
  // 表现成 "Uncaught [object Event]"。这里接住并降级为 warn，不影响编辑器主流程。
  function onUnhandledRejection(e: PromiseRejectionEvent) {
    const reason = e.reason;
    if (reason instanceof Event || (reason && typeof reason === 'object' && 'type' in reason && 'target' in reason)) {
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
    if (!t.closest('.wf-editor-settings')) settingsOpen = false;
  }
  onMount(() => {
    document.addEventListener('mousedown', onDocClick, true);
    return () => document.removeEventListener('mousedown', onDocClick, true);
  });

  // ─── Style computations ────────────────────────────────────────────────────
  const containerStyle = $derived.by(() => {
    if (!editorState.isVisible || editorState.openFiles.length === 0) return 'display: none;';
    if (editorState.displayMode === 'floating') {
      const r = editorState.floatingRect;
      return `position: fixed; left: ${r.x}px; top: ${r.y}px; width: ${r.w}px; height: ${r.h}px; z-index: 60;`;
    }
    // drawer: anchored to the right, **below the 44px header bar** so the
    // titlebar + workspace tabs remain visible/interactive (用户反馈：抽屉不能遮挡顶部 header)。
    const TOP_OFFSET = 44;
    return `position: fixed; top: ${TOP_OFFSET}px; right: 0; bottom: 0; width: ${editorState.drawerWidth}px; z-index: 40;`;
  });
</script>

<div
  class="wf-file-editor flex flex-col bg-[var(--wf-surface-2)]/98 backdrop-blur-xl border border-[var(--wf-border)] shadow-2xl {editorState.displayMode === 'floating' ? 'rounded-lg overflow-hidden' : 'rounded-l-lg'}"
  style={containerStyle}
>
    <!-- ═══ Header (tabs + actions) ═══ -->
    <div
      class="flex items-center shrink-0 h-9 border-b border-[var(--wf-border)] bg-[var(--wf-surface)]/90 {editorState.displayMode === 'floating' ? 'cursor-grab active:cursor-grabbing' : ''}"
      role="toolbar"
      onmousedown={editorState.displayMode === 'floating' ? onFloatingDragStart : undefined}
    >
      <!-- Tabs -->
      <div class="flex items-center min-w-0 flex-1 overflow-x-auto wf-tab-scroll">
        {#each editorState.openFiles as f, i (f.path)}
          <button
            type="button"
            class="group flex items-center gap-1.5 h-9 pl-3 pr-1.5 text-[12px] shrink-0 border-r border-[var(--wf-border)] transition-colors cursor-grab active:cursor-grabbing {editorState.activePath === f.path
              ? 'bg-[var(--wf-bg-raised)] text-[var(--wf-fg)]'
              : 'text-[var(--wf-fg-muted)] hover:bg-[var(--wf-surface)]/60 hover:text-[var(--wf-fg)]'}
              {draggingTabIndex === i ? 'opacity-50' : ''}
              {dragOverTabIndex === i && draggingTabIndex !== null && draggingTabIndex !== i ? 'ring-1 ring-[var(--wf-accent)]/60 ring-inset' : ''}"
            onclick={() => activateTab(f.path)}
            title={f.path}
            draggable="true"
            ondragstart={(e) => onTabDragStart(e, i)}
            ondragover={(e) => onTabDragOver(e, i)}
            ondragleave={onTabDragLeave}
            ondrop={(e) => onTabDrop(e, i)}
            ondragend={onTabDragEnd}
          >
            <span class="truncate max-w-[160px]">{f.name}</span>
            {#if f.isDirty}
              <span
                class="inline-block h-1.5 w-1.5 rounded-full bg-[var(--wf-accent)]"
                title="未保存"
              ></span>
            {/if}
            <span
              role="button"
              tabindex="0"
              class="flex h-4 w-4 items-center justify-center rounded text-[var(--wf-fg-muted)] hover:bg-[var(--wf-bg)]/50 hover:text-[var(--wf-fg)] {f.isDirty ? '' : 'opacity-0 group-hover:opacity-100'} transition-opacity"
              onclick={(e) => closeTab(e, f.path)}
              onkeydown={(e) => (e.key === 'Enter' || e.key === ' ') && closeTab(e as unknown as MouseEvent, f.path)}
              title="关闭"
            >
              <X class="h-3 w-3" />
            </span>
          </button>
        {/each}
      </div>

      <!-- Right-side actions -->
      <div class="flex items-center gap-0.5 px-1 shrink-0 border-l border-[var(--wf-border)]">
        <button
          type="button"
          class="flex h-7 w-7 items-center justify-center rounded text-[var(--wf-fg-muted)] hover:bg-[var(--wf-surface)] hover:text-[var(--wf-fg)] transition-colors disabled:opacity-30 disabled:cursor-not-allowed"
          title="查找 (Ctrl+F)"
          disabled={!current}
          onclick={triggerFind}
        >
          <Search class="h-3.5 w-3.5" />
        </button>
        <button
          type="button"
          class="flex h-7 w-7 items-center justify-center rounded text-[var(--wf-fg-muted)] hover:bg-[var(--wf-surface)] hover:text-[var(--wf-fg)] transition-colors disabled:opacity-30 disabled:cursor-not-allowed"
          title="保存 (Ctrl+S)"
          disabled={!current?.isDirty}
          onclick={() => fileEditorStore.saveActive()}
        >
          <Save class="h-3.5 w-3.5" />
        </button>

        <div class="wf-editor-settings relative">
          <button
            type="button"
            class="flex h-7 w-7 items-center justify-center rounded text-[var(--wf-fg-muted)] hover:bg-[var(--wf-surface)] hover:text-[var(--wf-fg)] transition-colors {settingsOpen ? 'bg-[var(--wf-surface)] text-[var(--wf-fg)]' : ''}"
            title="设置"
            onclick={() => (settingsOpen = !settingsOpen)}
          >
            <Settings class="h-3.5 w-3.5" />
          </button>
          {#if settingsOpen}
            <div
              class="absolute right-0 top-9 w-56 rounded-lg bg-[var(--wf-surface-2)] border border-[var(--wf-border)] shadow-xl z-10 py-1 text-[12px]"
            >
              <div class="px-3 py-1 text-[10px] uppercase tracking-wider text-[var(--wf-fg-muted)]">
                显示模式
              </div>
              <button
                type="button"
                class="w-full flex items-center gap-2 px-3 py-1.5 text-left hover:bg-[var(--wf-surface)] transition-colors {editorState.displayMode === 'drawer' ? 'text-[var(--wf-accent)]' : 'text-[var(--wf-fg)]'}"
                onclick={() => setMode('drawer')}
              >
                <PanelRightOpen class="h-3.5 w-3.5" /> 抽屉模式
                {#if editorState.displayMode === 'drawer'}<span class="ml-auto text-[10px]">✓</span>{/if}
              </button>
              <button
                type="button"
                class="w-full flex items-center gap-2 px-3 py-1.5 text-left hover:bg-[var(--wf-surface)] transition-colors {editorState.displayMode === 'floating' ? 'text-[var(--wf-accent)]' : 'text-[var(--wf-fg)]'}"
                onclick={() => setMode('floating')}
              >
                <Pin class="h-3.5 w-3.5" /> 悬浮 Pin 模式
                {#if editorState.displayMode === 'floating'}<span class="ml-auto text-[10px]">✓</span>{/if}
              </button>

              <div class="my-1 border-t border-[var(--wf-border)]"></div>
              <button
                type="button"
                class="w-full flex items-center gap-2 px-3 py-1.5 text-left hover:bg-[var(--wf-surface)] transition-colors text-[var(--wf-fg)] disabled:opacity-30 disabled:cursor-not-allowed"
                disabled={!current?.isDirty}
                onclick={() => { revertActive(); }}
              >
                <RotateCcw class="h-3.5 w-3.5" /> 放弃修改（重新从磁盘加载）
              </button>
              <button
                type="button"
                class="w-full flex items-center gap-2 px-3 py-1.5 text-left hover:bg-[var(--wf-surface)] transition-colors text-[var(--wf-fg)]"
                onclick={() => closeAll()}
              >
                <XCircle class="h-3.5 w-3.5" /> 关闭全部标签
              </button>
              <button
                type="button"
                class="w-full flex items-center gap-2 px-3 py-1.5 text-left hover:bg-[var(--wf-surface)] transition-colors text-[var(--wf-fg-muted)]"
                onclick={() => { settingsOpen = false; hidePanel(); }}
              >
                隐藏编辑器面板
              </button>
            </div>
          {/if}
        </div>
      </div>
    </div>

    <!-- ═══ Monaco host ═══
         Monaco 始终挂载；markdown 预览模式下用一块绝对定位的预览盖在上面。
         这样切 source ↔ preview 不需要销毁重建 Monaco，编辑历史 / undo 栈都保留。 -->
    <div class="flex-1 min-h-0 relative">
      <div
        bind:this={mountPoint}
        class="absolute inset-0"
        style={inPreviewMode ? 'visibility: hidden;' : ''}
      ></div>

      {#if current && isMarkdownFile && inPreviewMode}
        <div class="absolute inset-0 overflow-y-auto wf-scroll-overlay bg-[var(--wf-bg-raised)]">
          <MarkdownPreview
            content={current.content}
            onChange={(next) => fileEditorStore.updateContent(current!.path, next)}
            onRequestEdit={() => fileEditorStore.setViewMode(current!.path, 'source')}
          />
        </div>
      {/if}

      <!-- Preview ↔ Source 切换按钮：右上角浮动 pill，半透明玻璃态。
           仅对 markdown 文件渲染；悬停收到正式 accent，保证不抢主内容视觉重量。 -->
      {#if current && isMarkdownFile}
        <button
          type="button"
          class="absolute top-2.5 right-3 z-10 flex items-center gap-1.5 h-7 pl-2 pr-2.5 rounded-full text-[11px] font-medium
                 bg-[var(--wf-surface)]/60 backdrop-blur-md border border-[var(--wf-border)]
                 text-[var(--wf-fg-muted)] hover:text-[var(--wf-fg)] hover:bg-[var(--wf-surface)]/85 hover:border-[var(--wf-accent)]/40
                 transition-colors shadow-lg shadow-black/20"
          title={inPreviewMode ? '切换到源码编辑 (Markdown)' : '切换到预览 (Markdown)'}
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
        class="shrink-0 h-6 flex items-center gap-2 px-3 text-[10px] text-[var(--wf-fg-muted)] border-t border-[var(--wf-border)] bg-[var(--wf-surface)]/70 font-mono"
      >
        <span class="truncate flex-1" title={current.path}>{current.path}</span>
        <span>{current.language}</span>
        {#if current.isDirty}
          <span class="text-[var(--wf-accent)]">● 未保存</span>
        {:else}
          <span>已保存</span>
        {/if}
      </div>
    {/if}

    <!-- ═══ Drawer left-edge resizer ═══ -->
    {#if editorState.displayMode === 'drawer'}
      <div
        class="absolute left-0 top-0 bottom-0 w-1 cursor-col-resize hover:bg-[var(--wf-accent)]/40 transition-colors {isResizingDrawer ? 'bg-[var(--wf-accent)]/60' : ''}"
        role="separator"
        aria-orientation="vertical"
        aria-label="调整编辑器宽度"
        onmousedown={onDrawerResizeStart}
      ></div>
    {/if}

    <!-- ═══ Floating resize handles ═══ -->
    {#if editorState.displayMode === 'floating'}
      <div class="wf-float-handle wf-h-n" onmousedown={(e) => onFloatingResizeStart(e, 'n')} role="separator" aria-label="从上边调整"></div>
      <div class="wf-float-handle wf-h-s" onmousedown={(e) => onFloatingResizeStart(e, 's')} role="separator" aria-label="从下边调整"></div>
      <div class="wf-float-handle wf-h-e" onmousedown={(e) => onFloatingResizeStart(e, 'e')} role="separator" aria-label="从右边调整"></div>
      <div class="wf-float-handle wf-h-w" onmousedown={(e) => onFloatingResizeStart(e, 'w')} role="separator" aria-label="从左边调整"></div>
      <div class="wf-float-handle wf-h-ne" onmousedown={(e) => onFloatingResizeStart(e, 'ne')} role="separator" aria-label="右上"></div>
      <div class="wf-float-handle wf-h-nw" onmousedown={(e) => onFloatingResizeStart(e, 'nw')} role="separator" aria-label="左上"></div>
      <div class="wf-float-handle wf-h-se" onmousedown={(e) => onFloatingResizeStart(e, 'se')} role="separator" aria-label="右下"></div>
      <div class="wf-float-handle wf-h-sw" onmousedown={(e) => onFloatingResizeStart(e, 'sw')} role="separator" aria-label="左下"></div>
    {/if}
</div>

<style>
  .wf-tab-scroll::-webkit-scrollbar {
    height: 3px;
  }
  .wf-tab-scroll::-webkit-scrollbar-thumb {
    background: var(--wf-border);
  }

  /* Floating resize handles — small grab zones extending slightly outside the box */
  .wf-float-handle {
    position: absolute;
    z-index: 2;
  }
  .wf-h-n { top: -3px; left: 8px; right: 8px; height: 6px; cursor: ns-resize; }
  .wf-h-s { bottom: -3px; left: 8px; right: 8px; height: 6px; cursor: ns-resize; }
  .wf-h-w { top: 8px; bottom: 8px; left: -3px; width: 6px; cursor: ew-resize; }
  .wf-h-e { top: 8px; bottom: 8px; right: -3px; width: 6px; cursor: ew-resize; }
  .wf-h-nw { top: -3px; left: -3px; width: 10px; height: 10px; cursor: nwse-resize; }
  .wf-h-ne { top: -3px; right: -3px; width: 10px; height: 10px; cursor: nesw-resize; }
  .wf-h-sw { bottom: -3px; left: -3px; width: 10px; height: 10px; cursor: nesw-resize; }
  .wf-h-se { bottom: -3px; right: -3px; width: 10px; height: 10px; cursor: nwse-resize; }
</style>
