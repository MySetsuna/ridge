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
  } from 'lucide-svelte';
  import {
    fileEditorStore,
    activeFile,
    clampRectToViewport,
    SIDEBAR_TAB_W,
    type EditorDisplayMode,
    type FloatingRect,
  } from '$lib/stores/fileEditor';

  let mountPoint: HTMLDivElement | undefined;
  let editor: monaco.editor.IStandaloneCodeEditor | null = null;
  let currentModelPath: string | null = null;
  let settingsOpen = $state(false);
  let isResizingDrawer = $state(false);
  let isDraggingFloating = $state(false);
  let isResizingFloating = $state(false);

  let editorState = $derived($fileEditorStore);
  let current = $derived($activeFile);

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
  $effect(() => {
    if (!editor) return;
    if (!current) {
      // No file — empty buffer
      const m = monaco.editor.createModel('', 'plaintext');
      const prev = editor.getModel();
      editor.setModel(m);
      if (prev) prev.dispose();
      currentModelPath = null;
      return;
    }
    if (current.path === currentModelPath) {
      // Same tab — but content may have been reverted externally; sync if differs
      if (editor.getValue() !== current.content) {
        editor.setValue(current.content);
      }
      return;
    }
    const model = monaco.editor.createModel(current.content, current.language);
    const prev = editor.getModel();
    editor.setModel(model);
    if (prev) prev.dispose();
    currentModelPath = current.path;
    editor.focus();
  });

  // ─── Tab actions ───────────────────────────────────────────────────────────
  function activateTab(path: string) {
    fileEditorStore.setActive(path);
  }
  async function closeTab(e: MouseEvent, path: string) {
    e.stopPropagation();
    await fileEditorStore.closeFile(path);
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
  onMount(() => {
    window.addEventListener('resize', onWindowResize);
    return () => window.removeEventListener('resize', onWindowResize);
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
    // drawer: anchored to the right, full-height
    return `position: fixed; top: 0; right: 0; bottom: 0; width: ${editorState.drawerWidth}px; z-index: 40;`;
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
        {#each editorState.openFiles as f (f.path)}
          <button
            type="button"
            class="group flex items-center gap-1.5 h-9 pl-3 pr-1.5 text-[12px] shrink-0 border-r border-[var(--wf-border)] transition-colors {editorState.activePath === f.path
              ? 'bg-[var(--wf-bg-raised)] text-[var(--wf-fg)]'
              : 'text-[var(--wf-fg-muted)] hover:bg-[var(--wf-surface)]/60 hover:text-[var(--wf-fg)]'}"
            onclick={() => activateTab(f.path)}
            title={f.path}
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

    <!-- ═══ Monaco host ═══ -->
    <div class="flex-1 min-h-0 relative">
      <div bind:this={mountPoint} class="absolute inset-0"></div>
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
