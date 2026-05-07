<script lang="ts">
  import { onMount, onDestroy, tick, untrack } from 'svelte';
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
  import { portal } from '$lib/actions/portal';
  import { popupStyleFor } from '$lib/utils/anchorRect';
  import { settingsStore } from '$lib/stores/settings';
  import { showContextMenu, type ContextMenuItem } from '$lib/stores/contextMenu';
  import { alertDialog } from './RidgeDialog.svelte';
  import { Copy, FolderOpen } from 'lucide-svelte';
  import { dndzone, SOURCES } from 'svelte-dnd-action';

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
  // 搜索命中高亮装饰句柄。tied to editorState.searchHighlight：搜索点击时
  // 在命中范围加 inlineClassName='rg-search-flash-inline'，query 改变 / 关闭
  // 文件 / 切到非命中文件时 clear。
  let searchHighlightDecorations: monaco.editor.IEditorDecorationsCollection | null = null;
  // Keep-alive 缓存：每个 path 一个 Monaco model + view state，跨 tab 切换不丢
  // undo/redo 栈、滚动条、光标和折叠状态。Tab 关闭时才 dispose（在另一个 effect 里
  // 监听 openFiles 做 GC）。空白态用一个单例 emptyModel 兜底，避免每次反复创建。
  const modelCache = new Map<string, monaco.editor.ITextModel>();
  const viewStateCache = new Map<string, monaco.editor.ICodeEditorViewState | null>();
  let emptyModel: monaco.editor.ITextModel | null = null;
  let settingsOpen = $state(false);
  let settingsAnchor: HTMLElement | undefined = $state();
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

  // dndItems 必须挂 file 引用：svelte-dnd-action 拖拽时会插入 shadow
  // placeholder（`{...draggedItem, id: SHADOW_PLACEHOLDER_ITEM_ID}`），
  // placeholder 的 id 是固定常量而不是真实 path，模板渲染必须能从 item 自身
  // 拿到内容，否则 DOM 子节点数量会少于 items.length，库的 index 计算会错位
  // 导致拖拽后丢 tab。
  type DndItem = { id: string; file: typeof editorState.openFiles[number] };
  let dndItems = $state<DndItem[]>([]);
  let dndInProgress = $state(false);

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

  // ─── Markdown preview 滚动位置缓存 ──────────────────────────────────────────
  // preview overlay 现在是 mountPoint 的兄弟（不在 Monaco DOM 内部），
  // 通过 `{#if isMarkdownFile}` 始终渲染 + `style:display` 切显隐，
  // 单实例不重建。跨文件切换时 scrollTop 串台，按 path 暂存/还原。
  const markdownScrollCache = new Map<string, number>();

  /** Svelte action：跨文件切换 / preview ↔ source 切换 / 异步内容（mermaid、
   *  代码高亮）渲染完成后稳定保留 markdown 预览的 scrollTop。
   *
   *  实现要点（与朴素版本的差异）：
   *  1) **不在隐藏态写缓存**：display:none 时 `node.scrollTop` 由规范规定为 0，
   *     盲目 save 会把之前的合法值抹成 0。所以 save 之前先校验可见性。
   *  2) **path 切换后做"重试 restore"窗口**：mermaid / Monaco 代码高亮是异步的，
   *     第一帧 RAF 设的 scrollTop 可能在内容长出来之后失效（被浏览器 clamp 或
   *     被新插入的 view 推开）。开 ~800ms 的 retry 窗口，每帧再 set 一次 scrollTop
   *     直到用户滚动（停止抢用户的位置）或窗口超时。
   *  3) **display:none → block 时主动 restore**：path 没变所以 update() 不会
   *     触发；用 MutationObserver 监听 style 属性变化，回到可见态就 RAF 还原。 */
  function preserveMdScroll(node: HTMLElement, path: string) {
    let currentPath = path;
    const isVisible = () => node.style.display !== 'none' && !!node.offsetParent;
    const restore = () => {
      const cached = markdownScrollCache.get(currentPath);
      if (cached !== undefined) node.scrollTop = cached;
    };

    // —— 重试窗口：用于 path 切换 / 重新可见后等异步内容（mermaid 等）布局到位。
    let retryUntilMs = 0;
    let retryFrame: number | null = null;
    let userScrolled = false;
    const retry = () => {
      retryFrame = null;
      if (userScrolled) return;
      if (performance.now() > retryUntilMs) return;
      if (isVisible()) restore();
      retryFrame = requestAnimationFrame(retry);
    };
    const startRetry = (durationMs: number) => {
      userScrolled = false;
      retryUntilMs = performance.now() + durationMs;
      if (retryFrame === null) retryFrame = requestAnimationFrame(retry);
    };

    startRetry(800);

    const onScroll = () => {
      if (!isVisible()) return; // hidden 时浏览器 spec 上 scrollTop 报 0，跳过
      // 用户实际滚动后停止 retry restore，把控制权还给用户。
      userScrolled = true;
      markdownScrollCache.set(currentPath, node.scrollTop);
    };
    node.addEventListener('scroll', onScroll, { passive: true });

    // —— 监听 style 切换：display:none → block 时再 restore 一次。
    const styleObserver = new MutationObserver(() => {
      if (isVisible()) startRetry(400);
    });
    styleObserver.observe(node, { attributes: true, attributeFilter: ['style'] });

    return {
      update(newPath: string) {
        if (newPath === currentPath) return;
        // 关键：**绝不在 update 里读 node.scrollTop 写缓存**。
        // Svelte 5 同一次 flush 里，children（`<MarkdownPreview content={current.content}>`）
        // 的 prop 更新与 action.update 一起发生：DOM 已替成新文件内容，浏览器
        // 同步把 scrollTop clamp 到新内容的最大值；此时读到的是被污染的值，
        // 写回 cache[oldPath] 会把之前 onScroll 攒的合法值覆盖，下次切回
        // oldPath 就丢位 ——「两个 md preview tab 之间切换 scrollbar 状态丢失」
        // 就是这条路径。onScroll 已在用户每次滚动（含松手停下的最后一帧）
        // 实时写缓存，切 tab 之前的最终位置必然已被捕获。
        currentPath = newPath;
        startRetry(800);
      },
      destroy() {
        // 同 update：destroy 时 DOM 在卸载，scrollTop 不可信；信 onScroll。
        node.removeEventListener('scroll', onScroll);
        styleObserver.disconnect();
        if (retryFrame !== null) cancelAnimationFrame(retryFrame);
      },
    };
  }

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
    if (searchHighlightDecorations) {
      try { searchHighlightDecorations.clear(); } catch {}
      searchHighlightDecorations = null;
    }
    if (editor) {
      editor.dispose();
      editor = null;
    }
    // 释放 keep-alive 缓存里所有 model；emptyModel 也一并 dispose。
    for (const m of modelCache.values()) m.dispose();
    modelCache.clear();
    viewStateCache.clear();
    markdownScrollCache.clear();
    if (emptyModel) {
      emptyModel.dispose();
      emptyModel = null;
    }
    disposeDiffEditor();
  });

  // Mount editor once the DOM node is available AND the panel is visible.
  // Skip for image files and diff tabs — they use their own display layer.
  $effect(() => {
    if (!mountPoint || !editorState.isVisible) return;
    if (editor) return;
    if (current?.isImage || current?.diffArgs) return;
    // 显式构造初始 model 并塞进 modelCache，后续切回这个 path 才能复用 undo/redo 栈。
    let initialModel: monaco.editor.ITextModel;
    if (current) {
      const existing = modelCache.get(current.path);
      if (existing && !existing.isDisposed()) {
        initialModel = existing;
      } else {
        try {
          initialModel = monaco.editor.createModel(current.content, current.language);
        } catch {
          initialModel = monaco.editor.createModel(current.content, 'plaintext');
        }
        modelCache.set(current.path, initialModel);
      }
    } else {
      if (!emptyModel) emptyModel = monaco.editor.createModel('', 'plaintext');
      initialModel = emptyModel;
    }
    editor = monaco.editor.create(mountPoint, {
      model: initialModel,
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
    if (current) {
      const vs = viewStateCache.get(current.path);
      if (vs) editor.restoreViewState(vs);
    }
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

  // Swap editor model when active tab changes —— **keep-alive 模式**：
  //   • 切到另一个 tab 时不 dispose 旧 model；先把当前的 view state（滚动、
  //     光标、selection、折叠等）按 path 存到 viewStateCache。
  //   • 切到目标 tab 时优先复用 modelCache 里已有的 model（含 undo/redo 栈），
  //     再 restoreViewState 回到上次离开的位置。
  //   • Tab 真正关闭（在 openFiles 中消失）时才 dispose model；那一步在
  //     单独的 GC effect 里做（见下方）。
  $effect(() => {
    const c = current; // 先读 current 建立响应式订阅
    if (!editor) return;
    // Image and diff tabs don't use the regular Monaco editor.
    if (c?.isImage || c?.diffArgs) return;
    try {
      // —— 0) 保存上一个 model 的 view state（仅当 path 变化时才存，避免
      //        相同 path 自我覆盖）。
      if (currentModelPath && currentModelPath !== c?.path) {
        const vs = editor.saveViewState();
        viewStateCache.set(currentModelPath, vs);
      }

      if (!c) {
        // 没有活动文件：指向空白单例，不动 modelCache。
        if (!emptyModel) emptyModel = monaco.editor.createModel('', 'plaintext');
        currentModelPath = null;
        editor.setModel(emptyModel);
        return;
      }

      if (c.path === currentModelPath) {
        // 同一个 tab 内的 content 漂移：只在 store 里的 content 与 model 不一致
        // 时才 setValue（会清空 undo），常见于外部文件修改回灌。
        if (editor.getValue() !== c.content) {
          editor.setValue(c.content);
        }
        return;
      }

      // —— 1) 取或建 model（path 唯一缓存）。
      let model = modelCache.get(c.path);
      if (!model || model.isDisposed()) {
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
        modelCache.set(c.path, model);
      }

      // —— 2) 切模型 + 还原 view state。
      currentModelPath = c.path;
      editor.setModel(model);
      const vs = viewStateCache.get(c.path);
      if (vs) editor.restoreViewState(vs);
      // 关键：tab 切换时 editorCursorLine **不** 设为 setPosition 后的行号，
      // 而是设为 null —— 否则 MarkdownPreview 会把它当成"用户在 source 模式
      // 移动了光标"，对新 path 的对应行做 `scrollIntoView({behavior:'smooth'})`，
      // 把刚被 preserveMdScroll restore 的 scrollTop 平滑滚到顶部覆盖掉。
      // 真正的光标移动会通过下面的 onDidChangeCursorPosition 事件再把这个值
      // 填上来，preview 才会跟随。
      editorCursorLine = null;
      editor.focus();
    } catch (err) {
      console.error('[FileEditor] model swap failed', err);
    }
  });

  // —— GC：openFiles 中已经不再存在的 path，对应的 model / view state /
  //    markdown 滚动位置一并释放。Tab 关闭走的是 fileEditorStore.closeFile →
  //    openFiles 移除该项 → 这里命中。
  $effect(() => {
    const openPaths = new Set(editorState.openFiles.map((f) => f.path));
    for (const [path, model] of modelCache) {
      if (!openPaths.has(path)) {
        modelCache.delete(path);
        viewStateCache.delete(path);
        markdownScrollCache.delete(path);
        // 若 editor 当前正指向这个 model（极少见的关闭 active tab 时序），
        // 让 swap effect 先把 setModel 切走再 dispose，避免在仍被使用的
        // model 上 dispose 触发 Monaco 内部 assertion。
        if (model !== editor?.getModel()) {
          model.dispose();
        }
      }
    }
    // markdown-only 文件可能没进过 modelCache（也可能进了），单独再扫一遍 scroll 缓存。
    for (const path of markdownScrollCache.keys()) {
      if (!openPaths.has(path)) markdownScrollCache.delete(path);
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
    // 显式 track `editorState.pendingReveal`：搜索命中点击的目标文件如果已经
    // 是 active tab，`current` 引用不会变（activePath 字符串相同 → activeFile
    // derived 命中同一 OpenFile），仅靠 `current` tracking 不会重跑 effect →
    // 跳转不触发。读 pendingReveal 把它变成 dep，新设的命中能重新驱动 reveal。
    const c = current;
    const pr = editorState.pendingReveal;
    if (!editor || !c) return;
    // 仅当 active model 已经切到目标 path 才消费 —— 否则等 swap effect 把
    // currentModelPath 追上后这个 effect 自然再跑一次。
    if (currentModelPath !== c.path) return;
    if (!pr || pr.path !== c.path) return;
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

  // —— 搜索命中高亮：tied 到 editorState.searchHighlight。
  // 命中信息存在且匹配当前 model 时画装饰；否则清掉。SearchSidebar 在 query
  // 变化时会调 clearSearchHighlight() 把 store 里的字段置空，本 effect 自然
  // 清装饰。 */
  function clearSearchHighlightDecoration() {
    if (searchHighlightDecorations) {
      searchHighlightDecorations.clear();
      searchHighlightDecorations = null;
    }
  }
  $effect(() => {
    // 必须先读 reactive deps 再做 editor 非空校验：editor 是 plain let，
    // 不参与 Svelte tracking。如果先 `if (!editor) return`，首次 mount 时
    // current/searchHits 不会被注册成依赖，之后变化也不会重跑 effect。
    const c = current;
    const hits = editorState.searchHits;
    if (!editor) return;
    if (!c || currentModelPath !== c.path) {
      clearSearchHighlightDecoration();
      return;
    }
    // 当前文件的全部命中都画装饰；matchLength<=0 的容错跳过。
    const fileHits = hits.filter((h) => h.path === c.path && h.matchLength > 0);
    if (fileHits.length === 0) {
      clearSearchHighlightDecoration();
      return;
    }
    try {
      const opts: monaco.editor.IModelDecorationOptions = {
        inlineClassName: 'rg-search-flash-inline',
        // NeverGrows*：用户在端点插入字符不扩张装饰范围。
        stickiness: monaco.editor.TrackedRangeStickiness.NeverGrowsWhenTypingAtEdges,
      };
      const decorations = fileHits.map((h) => ({
        range: new monaco.Range(h.line, h.column, h.line, h.column + h.matchLength),
        options: opts,
      }));
      if (searchHighlightDecorations) {
        searchHighlightDecorations.set(decorations);
      } else {
        searchHighlightDecorations = editor.createDecorationsCollection(decorations);
      }
    } catch (err) {
      console.warn('[FileEditor] search highlight decorations failed', err);
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

  // ─── Tab drag-reorder (svelte-dnd-action) ─────────────────────────────────
  // 同步策略：
  //   • path 序列变化（新增 / 删除 / 重排）→ 用新数组重建 dndItems。
  //   • path 序列没变（仅 dirty / external 等内部字段更新）→ 原地把 file 引用
  //     替换成最新的，不动 dndItems 的数组身份；svelte-dnd-action 的 action
  //     不会因为 items 引用变化而重新跑 FLIP，落位动画不抖。
  // 必须挂 file 引用：svelte-dnd-action 拖拽时插入 shadow placeholder
  // (`{...draggedItem, id: SHADOW_PLACEHOLDER_ITEM_ID}`)，placeholder 的 id
  // 不是真实 path，模板渲染必须能从 item 自身拿到 file，否则 DOM 子节点比
  // items.length 少，库的 index 计算错位 → 拖完丢 tab。
  $effect(() => {
    if (dndInProgress) return;
    const files = editorState.openFiles;
    untrack(() => {
      const paths = files.map((f) => f.path);
      if (!sameIdSeq(dndItems, paths)) {
        dndItems = files.map((f) => ({ id: f.path, file: f }));
        return;
      }
      // path 序列一致：把每一项的 file 引用替换成最新的。注意这里不能新建
      // 数组（否则 dndzone 会重新跑 FLIP，导致闪烁）。Svelte 5 的代理写入
      // 会驱动模板里 `it.file` 读取者重渲染。
      for (let i = 0; i < files.length; i++) {
        if (dndItems[i].file !== files[i]) {
          dndItems[i].file = files[i];
        }
      }
    });
  });

  function sameIdSeq(items: Array<{ id: string }>, paths: string[]): boolean {
    if (items.length !== paths.length) return false;
    for (let i = 0; i < paths.length; i++) {
      if (items[i].id !== paths[i]) return false;
    }
    return true;
  }

  function onTabsConsider(e: CustomEvent<{ items: DndItem[]; info: { source: string } }>) {
    dndInProgress = true;
    dndItems = e.detail.items;
  }
  function onTabsFinalize(e: CustomEvent<{ items: DndItem[]; info: { source: string } }>) {
    dndInProgress = false;
    const next = e.detail.items;
    dndItems = next;
    if (e.detail.info.source !== SOURCES.POINTER) return;
    fileEditorStore.setOrder(next.map((it) => it.id));
  }
  /** 拖拽 tab 视觉反馈 + 锁定 Y 轴：editor tab 只能水平拖动。
   *  原理同 WorkspaceTabs：MutationObserver 监听 `style` 变化，把
   *  svelte-dnd-action 写入的 Y 覆盖回起点 Y，X 仍然跟随指针。 */
  function transformDraggedEditorTab(el: HTMLElement | undefined) {
    if (!el) return;
    el.style.transition = 'box-shadow 120ms ease-out, opacity 120ms ease-out';
    el.style.boxShadow = '0 10px 24px -6px rgba(0,0,0,0.45), 0 0 0 1px var(--rg-accent)';
    el.style.background = 'var(--rg-bg-raised, var(--rg-surface))';
    el.style.opacity = '0.96';
    // 必须高于 pin (floating) 模式编辑器面板的 z-index:60，否则在 pin 模式下
    // 拖拽 tab 时浮动副本会被面板自身遮住，看起来像"消失了"。仍然低于 9990
    // 起跳的 modal 层（见 CLAUDE.md 的 z-index 注册表）。
    el.style.zIndex = '100';

    let lockedY: number | null = null;
    const observer = new MutationObserver(() => {
      const t = el.style.transform;
      const m = t.match(/translate3d\((-?[\d.]+)px,\s*(-?[\d.]+)px,\s*(-?[\d.]+)px\)/);
      if (!m) return;
      const x = m[1];
      const y = parseFloat(m[2]);
      if (lockedY === null) {
        lockedY = y;
        return;
      }
      if (Math.abs(y - lockedY) < 0.5) return;
      el.style.transform = `translate3d(${x}px, ${lockedY}px, 0)`;
    });
    observer.observe(el, { attributes: true, attributeFilter: ['style'] });
  }

  /** 关闭图标拦截 pointer 起手事件，避免在 X 上拖拽误触 reorder。 */
  function blockEditorTabDragStart(e: Event) {
    e.stopPropagation();
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
  <!-- ═══ Header row 1: actions ═══ -->
  <!-- 紧凑高度（h-7=28px），与下方 tabs 行（h-6）之间不画分隔线 —— 视觉上
       是一个整体的两层 header。pin（floating）模式下整行（按钮以外的空白区）
       作为拖拽手柄。`onFloatingDragStart` 内部已过滤掉 button/input/select。 -->
  <div
    class="rg-editor-toolbar flex items-center shrink-0 h-7 bg-[var(--rg-surface)]/90 {editorState.displayMode ===
    'floating'
      ? 'cursor-grab active:cursor-grabbing select-none'
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
        class="rg-no-drag flex h-7 w-7 shrink-0 items-center justify-center text-[var(--rg-fg-muted)] hover:bg-[var(--rg-surface)] hover:text-[var(--rg-fg)] transition-colors border-r border-[var(--rg-border)]"
        title="收起编辑器面板"
        onmousedown={(e) => e.stopPropagation()}
        onclick={hidePanel}
      >
        <PanelRightClose class="h-3.5 w-3.5" />
      </button>
    {/if}

    <!-- 中部状态信息 + 拖拽热区：原 footer status bar 的内容（path / language /
         dirty）合并到这里。floating 模式下整块（含 span 文本）都参与面板拖拽，
         span 不会拦截 mousedown，事件冒泡到 toolbar 上的 `onFloatingDragStart`；
         `select-none` 已经在 toolbar 上设置，文本拖动不会触发选区。 -->
    <div
      class="flex-1 min-w-0 h-full flex items-center gap-2 px-2 text-[10px] text-[var(--rg-fg-muted)] font-mono"
    >
      {#if current}
        <span class="truncate flex-1" title={current.path}>{current.path}</span>
        <span class="shrink-0">{current.language}</span>
        {#if current.isDirty}
          <span class="shrink-0 text-[var(--rg-accent)]">● 未保存</span>
        {:else}
          <span class="shrink-0">已保存</span>
        {/if}
      {/if}
    </div>

    <!-- Right-side actions -->
    <div
      class="flex items-center gap-0.5 px-1 shrink-0 border-l border-[var(--rg-border)]"
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

      <div class="rg-editor-settings relative" bind:this={settingsAnchor}>
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
        {#if settingsOpen && settingsAnchor}
          <div
            class="rg-editor-settings w-56 rounded-lg bg-[var(--rg-surface-2)] border border-[var(--rg-border)] shadow-xl z-[9990] py-1 text-[12px]"
            style={popupStyleFor(settingsAnchor, 'bottom-end')}
            data-rg-portal-id="file-editor-settings"
            use:portal={{ id: 'file-editor-settings' }}
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

  <!-- ═══ Header row 2: file tabs ═══ -->
  <!-- tab 行不参与面板拖拽（不绑定 onFloatingDragStart）。pure CSS horizontal
       scroll；reorder via svelte-dnd-action（pointer-events，Tauri 兼容）。 -->
  <div class="flex items-center shrink-0 h-6 border-b border-[var(--rg-border)] bg-[var(--rg-surface)]/70">
    <div class="flex-1 min-w-0" use:overlayScroll={{ preset: 'horizontal-tabs' }}>
      <div
        class="rg-editor-tabs-dndzone flex items-stretch h-6"
        use:dndzone={{
          items: dndItems,
          flipDurationMs: 160,
          type: 'editor-tabs',
          dropTargetStyle: {},
          transformDraggedElement: transformDraggedEditorTab,
        }}
        onconsider={onTabsConsider}
        onfinalize={onTabsFinalize}
      >
        {#each dndItems as it (it.id)}
        {@const f = it.file}
        <button
          type="button"
          class="group flex items-center gap-1.5 h-6 pl-3 pr-1.5 text-[12px] shrink-0 border-r border-[var(--rg-border)] transition-colors cursor-grab active:cursor-grabbing {editorState.activePath ===
          f.path
            ? 'bg-[var(--rg-bg-raised)] text-[var(--rg-fg)]'
            : 'text-[var(--rg-fg-muted)] hover:bg-[var(--rg-surface)]/60 hover:text-[var(--rg-fg)]'}"
          onclick={() => activateTab(f.path)}
          oncontextmenu={(e) => onTabContextMenu(e, f.path)}
          title={f.path}
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
            onmousedown={blockEditorTabDragStart}
            ontouchstart={blockEditorTabDragStart}
            onpointerdown={blockEditorTabDragStart}
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
    </div>
  </div>

  <!-- ═══ Monaco host ═══ -->
  <div class="flex-1 min-h-0 relative">
    <!-- Regular editor。进入 diff 时隐藏；进入 preview 时也隐藏（preview 是
         mountPoint 的兄弟，独立 visibility，不会有继承问题）。 -->
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

    <!-- Markdown preview 容器：单实例（条件用 isMarkdownFile，不用 inPreviewMode），
         preview ↔ source 切换通过 style:display 切显隐，组件不卸载、状态自然保留。
         滚动条用 `rg-scroll` —— 与 Explorer / SCM 面板（overlayscrollbars 的
         rg-os-theme）视觉上同源（细透明条），跟 Monaco 自己的宽条不一样属于
         设计上的内部一致性 vs Monaco 自家风格的取舍：选了内部一致。 -->
    {#if current && isMarkdownFile}
      <div
        class="absolute inset-0 bg-[var(--rg-bg-raised)] overflow-y-auto overflow-x-hidden rg-scroll"
        style:display={inPreviewMode ? 'block' : 'none'}
        use:preserveMdScroll={current.path}
      >
        <MarkdownPreview
          content={current.content}
          basePath={current.path.replace(/[\\/][^\\/]+$/, '')}
          cursorLine={editorCursorLine}
          onChange={(next) => fileEditorStore.updateContent(current!.path, next)}
          onRevealSource={(line) => {
            if (!editor) return;
            const targetLine = Math.max(1, line + 1);
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
  /* svelte-dnd-action 默认会在 dropzone 外层加描边/动画背景；这里清掉，
     让拖拽过程的视觉只由 tab 自身的 :where() 默认 transform 体现。 */
  :global(.rg-editor-tabs-dndzone) {
    outline: none !important;
  }
  /* 搜索命中持续高亮：Monaco 把 inlineClassName 作为 <span> 的 class 加到
     匹配文本的 token 上。`!important` 防止被 Monaco 主题 token CSS 盖掉。
     主色 + 圆角让命中段落足够醒目。 */
  :global(.rg-search-flash-inline) {
    background-color: rgba(255, 200, 0, 0.45) !important;
    border-radius: 2px;
    box-shadow: 0 0 0 1px rgba(255, 200, 0, 0.5);
  }
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
