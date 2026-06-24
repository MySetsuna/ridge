<script lang="ts" module>
  // Monaco Editor Worker 配置 - 必须在使用 monaco 之前配置
  // 放在 FileEditor.svelte 的 module script 中，随编辑器首次加载时初始化，
  // 不与 +page.svelte 首屏 chunk 捆绑，避免未使用编辑器时加载 ~500KB worker 代码。
  import editorWorker from 'monaco-editor/esm/vs/editor/editor.worker?worker';
  import jsonWorker from 'monaco-editor/esm/vs/language/json/json.worker?worker';
  import cssWorker from 'monaco-editor/esm/vs/language/css/css.worker?worker';
  import htmlWorker from 'monaco-editor/esm/vs/language/html/html.worker?worker';
  import tsWorker from 'monaco-editor/esm/vs/language/typescript/ts.worker?worker';

  self.MonacoEnvironment = {
    getWorker(_: unknown, label: string) {
      if (label === 'json') return new jsonWorker();
      if (label === 'css' || label === 'scss' || label === 'less') return new cssWorker();
      if (label === 'html' || label === 'handlebars' || label === 'razor') return new htmlWorker();
      if (label === 'typescript' || label === 'javascript') return new tsWorker();
      return new editorWorker();
    },
  };
</script>

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
  // §IDE Ctrl/Cmd+Click 路径跳转：复用 linkResolver（解析相对/绝对路径 + 工程内
  // 判定）+ pathToken（提取光标下路径 token 与 :line:col 后缀）。
  import { resolveLink, executeAction } from '$lib/utils/linkResolver';
  import { pathTokenAt } from '$lib/utils/pathToken';
  import { projectStore } from '$lib/stores/project';
  import { markRecentlyWritten, onFsChange, isRecentlyWritten } from '$lib/stores/fsEvents';
  import { get } from 'svelte/store';
  // §IDE 全量 LSP（go-to-definition）：Ctrl+Click 非路径 token 时调 LSP 符号跳转；
  // TS/JS 文件激活/编辑时同步给 LSP host。详见 src/lib/lsp/lspClient.ts。
  import {
    lspSupports,
    lspDidOpen,
    lspDidChange,
    lspDefinition,
    lspReferences,
    lspHover,
    onLspDiagnostics,
    uriToPath,
  } from '$lib/lsp/lspClient';
  import type { UnlistenFn } from '@tauri-apps/api/event';
  import { overlayScroll } from '$lib/actions/overlayScroll';
  import { portal } from '$lib/actions/portal';
  import { popupStyleFor } from '$lib/utils/anchorRect';
  import { settingsStore } from '$lib/stores/settings';
  import { applyRidgeMonacoTheme, ridgeMonacoThemeId } from '$lib/monaco/ridgeTheme';
  import { showContextMenu, type ContextMenuItem } from '$lib/stores/contextMenu';
  import { alertDialog } from './RidgeDialog.svelte';
  import { popOutEditor } from '$lib/stores/editorWindow';
  import { Copy, FolderOpen, ExternalLink } from 'lucide-svelte';
  import { dndzone, SOURCES } from 'svelte-dnd-action';
  import { t, tr } from '$lib/i18n';

  /** 默认 monospace 栈：用户自定义 fontFamily 留空时回退到这一串。 */
  const DEFAULT_MONO =
    '"JetBrains Mono", "Cascadia Code", "SF Mono", ui-monospace, Consolas, monospace';
  const editorFontFamily = $derived(
    $settingsStore.editorFontFamily.trim() || DEFAULT_MONO
  );
  const editorFontSize = $derived($settingsStore.editorFontSize);
  // Monaco theme follows the active Ridge theme. Each Ridge theme id
  // (dark / sand / grass / soil / wheat / starsky) gets its own custom
  // Monaco theme registered as `ridge-${id}` whose editor.background /
  // foreground / selection / cursor / etc. are read from the live
  // `--rg-*` CSS variables. See $lib/monaco/ridgeTheme.
  //
  // The $effect re-runs on every theme change, which re-defines the
  // theme (picking up the latest CSS-var values) and retints both the
  // inline editor and any active diff editor via setTheme.
  const monacoTheme = $derived(ridgeMonacoThemeId($settingsStore.theme));
  $effect(() => {
    applyRidgeMonacoTheme($settingsStore.theme);
    // `monaco.editor.setTheme` is the global retint — it switches the
    // active registered theme for every Monaco editor instance at once,
    // including diff editors whose `IDiffEditorOptions` doesn't accept
    // `theme` via `updateOptions`. Using setTheme here both unifies the
    // code path and fixes the type error on the diff branch.
    monaco.editor.setTheme(monacoTheme);
  });

  let mountPoint: HTMLDivElement | undefined;
  let panelRootEl: HTMLDivElement | undefined = $state();
  let editor: monaco.editor.IStandaloneCodeEditor | null = null;
  let currentModelPath: string | null = null;
  // 搜索命中高亮装饰句柄。tied to editorState.searchHighlight：搜索点击时
  // 在命中范围加 inlineClassName='rg-search-flash-inline'，query 改变 / 关闭
  // 文件 / 切到非命中文件时 clear。
  let searchHighlightDecorations: monaco.editor.IEditorDecorationsCollection | null = null;
  // §IDE 行级 Git blame：Alt+B / 右键「切换 Git 行注释」开关。开启时拉 git_blame
  // 在每行行尾注入「作者 · 相对时间 · 摘要」灰字注释；编辑内容即清除（避免错位）。
  interface BlameLine {
    line: number;
    commit: string;
    author: string;
    timestamp: number;
    summary: string;
  }
  let blameVisible = $state(false);
  let blameDecorations: monaco.editor.IEditorDecorationsCollection | null = null;
  // §IDE LSP 文档同步状态：已 didOpen 的路径集 + 单调递增的 didChange 版本号。
  const lspOpenedPaths = new Set<string>();
  let lspVersion = 1;
  // P2：LSP provider/监听句柄，onDestroy 时清理。
  const lspDisposables: monaco.IDisposable[] = [];
  let lspDiagUnlisten: UnlistenFn | null = null;
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
  // Keep-alive 模式（与主 source editor 对齐）：单一 diffEditor 实例 + 按 path
  // 缓存的 (originalModel, modifiedModel) pair 与 view state。切走 diff tab 时
  // 仅 saveViewState；切回时 setModel + restoreViewState，scroll/折叠/光标全部
  // 还原。tab 真正关闭（在 openFiles 中消失）才在 GC effect 里 dispose models。
  let diffMountPoint: HTMLDivElement | undefined;
  // diffEditor 保持普通 let（**勿**改 $state）：做成响应式会让所有读取它的 effect
  // （renderSideBySide / 字体 / 布局 / GC）在 diff 实例创建/销毁时一并重跑，打乱编辑器
  // 生命周期 → 切到普通文件后展示区卡住、只显示上一个文件内容。模式切换的修复改由下方
  // renderSideBySide effect「先读 diffRenderSideBySide 再判空」实现：初始模式已在
  // ensureDiffEditor() 内 updateOptions 设好；点击切换时 diffRenderSideBySide（$state）
  // 变化即让该 effect 重跑生效，无需 diffEditor 响应式。
  let diffEditor: monaco.editor.IStandaloneDiffEditor | null = null;
  type DiffPair = {
    original: monaco.editor.ITextModel;
    modified: monaco.editor.ITextModel;
  };
  const diffModelCache = new Map<string, DiffPair>();
  const diffViewStateCache = new Map<string, monaco.editor.IDiffEditorViewState>();
  let diffCurrentPath: string | null = null;
  let diffLoading = $state(false);
  let diffError = $state('');
  // §SCM 可编辑 diff（对标 VSCode）：仅「工作区改动」diff（非 commit / 非 staged）的
  // modified 侧可编辑；记录其 {repoRoot, git-relative path} 供 diff 编辑器 Ctrl+S 直接
  // write_file 落盘。历史 commit / 已暂存 diff 保持只读。
  let diffEditableArgs: { repoRoot: string; path: string } | null = null;
  // §SCM diff 实时刷新：订阅 fs-changed，外部改动当前所看工作区 diff 的文件时重载。
  let fsDiffUnsub: (() => void) | null = null;
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

  /** 仅在整个 FileEditor 卸载时调用：彻底销毁 diff editor 实例 + 全部缓存的
   *  models / view states。切 tab 时不要调这个，用 saveAndDetachDiff() 代替。 */
  function disposeDiffEditor(): void {
    diffEditor?.dispose();
    diffEditor = null;
    for (const pair of diffModelCache.values()) {
      pair.original.dispose();
      pair.modified.dispose();
    }
    diffModelCache.clear();
    diffViewStateCache.clear();
    diffCurrentPath = null;
  }

  /** 切走 diff tab 时调用：保留实例 + cache，仅保存 view state。 */
  function saveDiffViewState(): void {
    if (!diffEditor || !diffCurrentPath) return;
    const vs = diffEditor.saveViewState();
    if (vs) diffViewStateCache.set(diffCurrentPath, vs);
  }

  function ensureDiffEditor(): monaco.editor.IStandaloneDiffEditor | null {
    if (diffEditor) return diffEditor;
    if (!diffMountPoint) return null;
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
      // §SCM diff 模式切换：Monaco 默认 useInlineViewWhenSpaceIsLimited=true，空间
      // 不够时自动改 inline，会**覆盖**用户显式的 inline↔并排（renderSideBySide）切换
      // → 点了不生效。关掉它，让工具栏的模式切换始终权威。
      useInlineViewWhenSpaceIsLimited: false,
    });
    diffEditor.updateOptions({ renderSideBySide: diffRenderSideBySide });
    // §光标对齐：若 diff 实例在 webfont 就绪前创建，等字体到位后重测字符宽度。
    void remeasureWhenFontReady();
    // §SCM 可编辑 diff：Ctrl/Cmd+S 在可编辑（工作区）diff 上把 modified 侧落盘。
    diffEditor.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.KeyS, () => {
      void saveDiffModified();
    });
    return diffEditor;
  }

  /** 把可编辑（工作区）diff 的 modified 内容写回文件（绝对路径 = repoRoot/git-relative）。 */
  async function saveDiffModified(): Promise<void> {
    if (!diffEditableArgs || !diffEditor || !isTauri()) return;
    const content = diffEditor.getModifiedEditor().getValue();
    const abs = `${diffEditableArgs.repoRoot.replace(/[/\\]+$/, '')}/${diffEditableArgs.path}`;
    try {
      await invoke('write_file', { path: abs, content });
      markRecentlyWritten(abs); // 抑制自写触发的 fs-changed「外部修改」提示
    } catch (e) {
      diffError = e instanceof Error ? e.message : String(e);
    }
  }

  /** 设定当前 diff 是否可编辑（仅工作区改动），并切 modified 侧 readOnly。 */
  function applyDiffEditable(
    ed: monaco.editor.IStandaloneDiffEditor,
    args: { repoRoot: string; path: string; cached: boolean; commit?: string }
  ): void {
    const editable = !args.commit && !args.cached;
    ed.updateOptions({ readOnly: !editable });
    diffEditableArgs = editable ? { repoRoot: args.repoRoot, path: args.path } : null;
  }

  async function loadDiff(
    args: { repoRoot: string; path: string; cached: boolean; commit?: string; compareBase?: string },
    tabPath: string,
    forceReload: boolean = false
  ): Promise<void> {
    if (forceReload) {
      const existing = diffModelCache.get(tabPath);
      if (existing) {
        existing.original.dispose();
        existing.modified.dispose();
        diffModelCache.delete(tabPath);
      }
    }

    // 命中缓存：跳过 IPC 直接切模型 + 还原 view state（与主 source editor 一致）。
    const cached = diffModelCache.get(tabPath);
    if (cached) {
      const ed = ensureDiffEditor();
      if (!ed) return;
      // 切走当前 path 前先存 view state。
      if (diffCurrentPath && diffCurrentPath !== tabPath) saveDiffViewState();
      // 显式 detach 再 attach 旧 model 对，防止 Monaco 内部在 model 不变时跳过
      // 重渲染导致新旧内容叠加（#残留）。
      ed.setModel(null);
      ed.setModel({ original: cached.original, modified: cached.modified });
      diffCurrentPath = tabPath;
      applyDiffEditable(ed, args);
      const vs = diffViewStateCache.get(tabPath);
      if (vs) ed.restoreViewState(vs);
      diffError = '';
      return;
    }
    const myId = ++diffReqId;
    diffLoading = true;
    diffError = '';
    // 切走当前正在显示的 path 之前先存 view state，避免被新加载覆盖。
    if (diffCurrentPath && diffCurrentPath !== tabPath) saveDiffViewState();
    // 显式 detach 确保 mount 点干净，避免残留覆盖
    if (diffEditor && diffEditor.getModel()) {
      diffEditor.setModel(null);
    }
    try {
      if (!isTauri()) throw new Error(tr('editor.requiresTauri'));
      const v =
        args.compareBase && args.commit
          ? await invoke<{ original: string; modified: string }>(
              'git_get_file_versions_between',
              { repoRoot: args.repoRoot, path: args.path, from: args.compareBase, to: args.commit }
            )
          : args.commit
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
      const original = monaco.editor.createModel(v.original, lang);
      const modified = monaco.editor.createModel(v.modified, lang);
      diffModelCache.set(tabPath, { original, modified });
      const ed = ensureDiffEditor();
      if (!ed) return;
      ed.setModel({ original, modified });
      diffCurrentPath = tabPath;
      applyDiffEditable(ed, args);
      const vs = diffViewStateCache.get(tabPath);
      if (vs) ed.restoreViewState(vs);
    } catch (e) {
      if (myId !== diffReqId) return;
      diffError = e instanceof Error ? e.message : String(e);
    } finally {
      if (myId === diffReqId) diffLoading = false;
    }
  }

  onDestroy(() => {
    if (searchHighlightDecorations) {
      try { searchHighlightDecorations.clear(); } catch {}
      searchHighlightDecorations = null;
    }
    // §IDE LSP P2：释放 hover provider + 诊断监听。
    for (const d of lspDisposables) {
      try { d.dispose(); } catch {}
    }
    lspDisposables.length = 0;
    if (lspDiagUnlisten) {
      try { lspDiagUnlisten(); } catch {}
      lspDiagUnlisten = null;
    }
    if (fsDiffUnsub) {
      try { fsDiffUnsub(); } catch {}
      fsDiffUnsub = null;
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
    
    // Ensure theme is applied before creating editor
    applyRidgeMonacoTheme($settingsStore.theme);

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
      fontSize: coarsePointer ? Math.max(editorFontSize, 15) : editorFontSize,
      minimap: { enabled: false },
      scrollBeyondLastLine: false,
      tabSize: 2,
      wordWrap: 'on',
      padding: { top: 8, bottom: 8 },
      // §IDE：与 VS Code 一致用 Alt+Click 加多光标，腾出 Ctrl/Cmd+Click 给「跳转」
      // （路径跳转见下方 onMouseDown；未来 LSP go-to-definition 也走 Ctrl+Click）。
      multiCursorModifier: 'alt',
      // §C1 touch-friendly options on coarse-pointer devices.
      ...(coarsePointer ? {
        scrollbar: { verticalScrollbarSize: 16, horizontalScrollbarSize: 16 },
        folding: false,
        lineNumbersMinChars: 3,
        overviewRulerLanes: 0,
      } : {}),
    });
    currentModelPath = current?.path ?? null;
    if (current) {
      const vs = viewStateCache.get(current.path);
      if (vs) editor.restoreViewState(vs);
    }
    // §光标对齐：editor 多在 webfont 就绪前创建，等字体到位后重测字符宽度，
    // 消除行末光标因回退字体测宽导致的累积左偏。
    void remeasureWhenFontReady();
    editor.onDidChangeModelContent(() => {
      if (!editor || !currentModelPath) return;
      const value = editor.getValue();
      fileEditorStore.updateContent(currentModelPath, value);
      // §IDE LSP：把变更全量同步给 LSP（仅已 didOpen 的 TS/JS），保证 definition 位置准确。
      if (lspOpenedPaths.has(currentModelPath)) {
        const root = get(projectStore).currentPath;
        if (root) void lspDidChange(root, currentModelPath, ++lspVersion, value);
      }
    });
    // Track cursor line so the markdown preview can follow in preview mode.
    editor.onDidChangeCursorPosition((ev) => {
      editorCursorLine = ev.position.lineNumber;
    });
    // Ctrl+S / Cmd+S → save
    editor.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.KeyS, () => {
      void fileEditorStore.saveActive();
    });
    // §IDE Ctrl/Cmd+Click 路径跳转（VS Code 对齐）：命中可解析的文件路径 token →
    // 在内置编辑器打开（带 :line:col 定位）。非路径 token 不拦截，留给未来 LSP
    // go-to-definition（同样走 Ctrl+Click，因上面已把多光标改到 Alt）。
    editor.onMouseDown((e) => {
      const oe = e.event;
      if (!(oe.ctrlKey || oe.metaKey) || !oe.leftButton) return;
      if (
        e.target.type !== monaco.editor.MouseTargetType.CONTENT_TEXT ||
        !e.target.position ||
        !currentModelPath
      )
        return;
      const model = editor?.getModel();
      if (!model) return;
      const pos = e.target.position;
      const tok = pathTokenAt(model.getLineContent(pos.lineNumber), pos.column);
      if (!tok) return;
      // 相对路径以当前文件目录为基；工程根纳入 knownCwds → 工程内文件 open-file
      // （而非 reveal 到资源管理器）。
      const fileDir = currentModelPath.replace(/[/\\][^/\\]*$/, '');
      const projectRoot = get(projectStore).currentPath ?? undefined;
      const known = projectRoot ? [projectRoot, fileDir] : [fileDir];
      const action = resolveLink(tok.path, { cwd: fileDir, basePath: fileDir, knownCwds: known });
      // 1) 明确的工程内文件路径 → 直接打开（高置信，路径跳转）。
      if (action.kind === 'open-file') {
        oe.preventDefault();
        oe.stopPropagation();
        void executeAction({ ...action, line: tok.line, col: tok.col });
        return;
      }
      // 2) 非明确文件路径：TS/JS 文件优先试 LSP go-to-definition（符号/方法跳转）；
      //    LSP 无结果时回退到路径 reveal（如外部目录/文件）。
      const issued = gotoDefinitionAt(
        pos.lineNumber,
        pos.column,
        action.kind === 'reveal' ? () => void executeAction(action) : undefined
      );
      if (issued) {
        oe.preventDefault();
        oe.stopPropagation();
        return;
      }
      // 3) 非 TS/JS：保留 F2a 的 reveal（外部路径在资源管理器打开）。
      if (action.kind === 'reveal') {
        oe.preventDefault();
        oe.stopPropagation();
        void executeAction(action);
      }
    });
    // §IDE 行级 Git blame 开关：Alt+B + 右键菜单「切换 Git 行注释」。
    editor.addAction({
      id: 'rg.toggleBlame',
      label: '切换 Git 行注释 (Blame)',
      keybindings: [monaco.KeyMod.Alt | monaco.KeyCode.KeyB],
      contextMenuGroupId: 'navigation',
      contextMenuOrder: 1.5,
      run: () => {
        blameVisible = !blameVisible; // 同步由下方 $effect(refreshBlame) 处理
      },
    });
    // §IDE LSP P2 — F12 / 右键「转到定义」（与 Ctrl+Click 同走 gotoDefinitionAt）。
    editor.addAction({
      id: 'rg.gotoDefinition',
      label: '转到定义 (Go to Definition)',
      keybindings: [monaco.KeyCode.F12],
      contextMenuGroupId: 'navigation',
      contextMenuOrder: 1.1,
      run: (ed) => {
        const p = ed.getPosition();
        if (p) gotoDefinitionAt(p.lineNumber, p.column);
      },
    });
    // §IDE LSP — providers 全局注册（FileEditor 单例懒挂载），onDestroy 释放。
    const LSP_LANGS = ['typescript', 'javascript', 'typescriptreact', 'javascriptreact', 'rust'];
    // P2 hover：签名/文档。
    lspDisposables.push(
      monaco.languages.registerHoverProvider(LSP_LANGS, {
        async provideHover(model, position) {
          const path = pathForModel(model);
          if (!path || !lspSupports(path)) return null;
          const root = get(projectStore).currentPath;
          if (!root) return null;
          const hover = await lspHover(root, path, position.lineNumber - 1, position.column - 1);
          return hover ? { contents: [{ value: hover.markdown }] } : null;
        },
      })
    );
    // P3 references：Shift+F12 / 右键「查找所有引用」→ Monaco peek。
    lspDisposables.push(
      monaco.languages.registerReferenceProvider(LSP_LANGS, {
        async provideReferences(model, position) {
          const path = pathForModel(model);
          if (!path || !lspSupports(path)) return null;
          const root = get(projectStore).currentPath;
          if (!root) return null;
          const refs = await lspReferences(
            root,
            path,
            position.lineNumber - 1,
            position.column - 1
          );
          return refs.map((r) => ({
            uri: monaco.Uri.file(r.path),
            range: new monaco.Range(r.line, r.column, r.line, r.column),
          }));
        },
      })
    );
    // P3 editorOpener：Monaco 内部导航（references peek 点击等）路由到 Ridge 编辑器，
    // 而非默认的 standalone 行为（cross-file 打不开）。uriToPath 已归一盘符大小写。
    lspDisposables.push(
      monaco.editor.registerEditorOpener({
        openCodeEditor(_source, resource, selectionOrPosition) {
          const path = uriToPath(resource.toString());
          let line: number | undefined;
          let column: number | undefined;
          if (selectionOrPosition && 'lineNumber' in selectionOrPosition) {
            line = selectionOrPosition.lineNumber;
            column = selectionOrPosition.column;
          } else if (selectionOrPosition && 'startLineNumber' in selectionOrPosition) {
            line = selectionOrPosition.startLineNumber;
            column = selectionOrPosition.startColumn;
          }
          void fileEditorStore.openFile(path, { line, column });
          return true;
        },
      })
    );
    // §IDE LSP P2 — 诊断：LSP host 经 Tauri event 推送 → 设到对应 model 的 markers。
    void onLspDiagnostics((payload) => {
      const path = uriToPath(payload.uri);
      const model =
        modelCache.get(path) ?? (path === currentModelPath ? editor?.getModel() ?? null : null);
      if (!model) return;
      const markers = payload.diagnostics.map((d) => ({
        severity: lspSeverityToMonaco(d.severity),
        message: d.message,
        startLineNumber: d.range.start.line + 1,
        startColumn: d.range.start.character + 1,
        endLineNumber: d.range.end.line + 1,
        endColumn: d.range.end.character + 1,
        source: d.source,
      }));
      monaco.editor.setModelMarkers(model, 'lsp', markers);
    }).then((un) => {
      lspDiagUnlisten = un;
    });
    // §SCM diff 实时刷新：外部修改当前所看「工作区改动」diff 的文件时重载（自写经
    // isRecentlyWritten 排除，故不会因自己 Ctrl+S 保存而丢正在编辑的内容）。
    fsDiffUnsub = onFsChange((payload) => {
      const c = current;
      if (!c?.diffArgs || c.diffArgs.commit || c.diffArgs.cached) return;
      const abs = `${c.diffArgs.repoRoot.replace(/[/\\]+$/, '')}/${c.diffArgs.path}`;
      const norm = (p: string) => p.replace(/\\/g, '/').toLowerCase();
      const target = norm(abs);
      if (payload.paths.some((p) => norm(p) === target) && !isRecentlyWritten(abs)) {
        void loadDiff(c.diffArgs, c.path, true);
      }
    });
  });

  /** 相对时间（zh）：用于 blame 行尾注释。 */
  function relTime(unixSec: number): string {
    if (!unixSec) return '未提交';
    const diff = Date.now() / 1000 - unixSec;
    if (diff < 60) return '刚刚';
    if (diff < 3600) return `${Math.floor(diff / 60)} 分钟前`;
    if (diff < 86400) return `${Math.floor(diff / 3600)} 小时前`;
    if (diff < 86400 * 30) return `${Math.floor(diff / 86400)} 天前`;
    const d = new Date(unixSec * 1000);
    return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
  }

  /** 清除 blame 行注释（切文件 / 关闭时）。 */
  function clearBlame(): void {
    blameDecorations?.clear();
    blameDecorations = null;
  }

  /** 拉取并渲染当前文件的行级 blame 注释。blameVisible 为假或无文件时清空。 */
  async function refreshBlame(): Promise<void> {
    if (!editor) return;
    if (!blameVisible || !currentModelPath) {
      clearBlame();
      return;
    }
    const repoRoot = get(projectStore).currentPath;
    if (!repoRoot) return;
    const reqPath = currentModelPath;
    let blame: BlameLine[];
    try {
      blame = await invoke<BlameLine[]>('git_blame', { repoRoot, path: reqPath });
    } catch (err) {
      console.warn('[blame] git_blame failed', reqPath, err);
      return;
    }
    // stale guard：await 期间可能已切到别的文件或关闭了 blame。
    if (!editor || !blameVisible || currentModelPath !== reqPath) return;
    const model = editor.getModel();
    if (!model) return;
    const maxLine = model.getLineCount();
    const decos: monaco.editor.IModelDeltaDecoration[] = blame
      .filter((b) => b.line >= 1 && b.line <= maxLine)
      .map((b) => {
        const col = model.getLineMaxColumn(b.line);
        return {
          range: new monaco.Range(b.line, col, b.line, col),
          options: {
            after: {
              content: `      ${b.author} · ${relTime(b.timestamp)}${b.summary ? ` · ${b.summary}` : ''}`,
              inlineClassName: 'rg-blame-annotation',
            },
            showIfCollapsed: true,
          },
        };
      });
    // 每次重建集合绑定到当前 model（跨 tab 切换不残留旧 model 的装饰）。
    clearBlame();
    blameDecorations = editor.createDecorationsCollection(decos);
  }

  // ── §IDE LSP P2 helpers ──────────────────────────────────────────────────

  /** 反查 Monaco model 对应的文件路径（active 用 currentModelPath，否则查 modelCache）。 */
  function pathForModel(model: monaco.editor.ITextModel): string | null {
    if (model === editor?.getModel()) return currentModelPath;
    for (const [p, m] of modelCache) if (m === model) return p;
    return null;
  }

  /** LSP DiagnosticSeverity（1-4）→ Monaco MarkerSeverity。 */
  function lspSeverityToMonaco(sev?: number): monaco.MarkerSeverity {
    switch (sev) {
      case 1:
        return monaco.MarkerSeverity.Error;
      case 2:
        return monaco.MarkerSeverity.Warning;
      case 3:
        return monaco.MarkerSeverity.Info;
      default:
        return monaco.MarkerSeverity.Hint;
    }
  }

  /**
   * 在指定位置（1-based）触发 LSP go-to-definition → openFile 落点。Ctrl+Click /
   * F12 / 右键「转到定义」共用。返回是否已发起请求（用于调用方决定是否拦默认行为）；
   * `onEmpty` 在无定义结果时回调（Ctrl+Click 用它回退路径 reveal）。
   */
  function gotoDefinitionAt(
    lineNumber: number,
    column: number,
    onEmpty?: () => void
  ): boolean {
    if (!currentModelPath || !lspSupports(currentModelPath)) return false;
    const root = get(projectStore).currentPath;
    if (!root) return false;
    const path = currentModelPath;
    void lspDefinition(root, path, lineNumber - 1, column - 1).then((targets) => {
      if (targets.length > 0) {
        void fileEditorStore.openFile(targets[0].path, {
          line: targets[0].line,
          column: targets[0].column,
        });
      } else {
        onEmpty?.();
      }
    });
    return true;
  }

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

    try {
      // —— 0) 保存上一个 model 的 view state（仅当 path 变化时才存，避免
      //        相同 path 自我覆盖）。
      if (currentModelPath && currentModelPath !== c?.path) {
        const vs = editor.saveViewState();
        viewStateCache.set(currentModelPath, vs);
      }

      // Image and diff tabs don't use the regular Monaco editor.
      if (!c || c.isImage || c.diffArgs) {
        // 没有活动文件，或者正在显示 Image / Diff：指向空白单例，不动 modelCache。
        if (!emptyModel) emptyModel = monaco.editor.createModel('', 'plaintext');
        currentModelPath = null;
        editor.setModel(emptyModel);
        return;
      }

      if (c.path === currentModelPath) {
        // 同一个 tab 内的 content 漂移：只在 store 里的 content 与 model 不一致
        // 时才 setValue（会清空 undo），常见于外部文件修改回灌。
        // setValue 会把光标/滚动重置到顶部 —— 如果用户正停在这个 tab 上看着
        // （外部 clean reconcile），位置被弹到第 1 行体验很差。围绕 setValue
        // 存/还原 view state，让滚动与光标尽量留在原处。
        if (editor.getValue() !== c.content) {
          const vs = editor.saveViewState();
          editor.setValue(c.content);
          if (vs) editor.restoreViewState(vs);
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
      } else if (model.getValue() !== c.content) {
        // Cached model exists but its text drifted from the store.
        // Happens when `fileEditor.openFile` re-reads from disk on
        // re-activation (the bug where reopening a tab showed the
        // stale content from when it was first opened, even after the
        // file changed on disk in the background). Push the fresh
        // content into the model so the next `setModel` swap shows
        // the current bytes. Resets undo, which is the right call
        // here — the prior undo stack was relative to a pre-refresh
        // snapshot that no longer represents anything on disk.
        model.setValue(c.content);
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

  // §IDE blame 同步：声明在 swap effect 之后 → 同一 flush 里后跑（currentModelPath
  // 已更新）。读 current 建立对 tab 切换的依赖；refreshBlame 内同步读 blameVisible，
  // 故开关切换也会触发本 effect（开→拉取，关→清除）。
  $effect(() => {
    void current; // 活动 tab 变化 → 重跑
    void refreshBlame();
  });

  // §IDE LSP didOpen：TS/JS 文件首次激活时把内容同步给 LSP host（definition 需要打开
  // 缓冲区）。声明在 swap effect 之后 → currentModelPath 已更新。每路径只 didOpen 一次，
  // 后续编辑走 didChange（onDidChangeModelContent）。
  $effect(() => {
    void current; // tab 切换依赖
    const path = currentModelPath;
    if (!path || !lspSupports(path) || lspOpenedPaths.has(path)) return;
    const root = get(projectStore).currentPath;
    const model = editor?.getModel();
    if (!root || !model) return;
    lspOpenedPaths.add(path);
    void lspDidOpen(root, path, model.getValue());
  });

  // —— GC：openFiles 中已经不再存在的 path，对应的 model / view state /
  //    markdown 滚动位置 / diff pair 一并释放。Tab 关闭走的是
  //    fileEditorStore.closeFile → openFiles 移除该项 → 这里命中。
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
    // diff tab 关闭：dispose models + 删 view state。守卫 active diff
    // 不做 dispose（与主 model GC 同思路），由后续 setModel 切走后自然释放。
    const activeDiffModels = diffEditor?.getModel();
    for (const [path, pair] of diffModelCache) {
      if (!openPaths.has(path)) {
        diffModelCache.delete(path);
        diffViewStateCache.delete(path);
        if (
          !activeDiffModels ||
          (pair.original !== activeDiffModels.original &&
            pair.modified !== activeDiffModels.modified)
        ) {
          pair.original.dispose();
          pair.modified.dispose();
        }
        if (path === diffCurrentPath) diffCurrentPath = null;
      }
    }
  });

  // ─── Diff editor lifecycle ────────────────────────────────────────────────
  // Keep-alive：切走 diff tab 仅保存 view state；切回 / 切换到不同 diff path
  // 走 loadDiff（命中缓存即跳 IPC）。diffReqId 防止快速切换时 stale 异步覆盖。
  // 在切离 diff 时先 detach model + dispose view state，避免 display:none 后
  // 的残留在新 tab 被 GC dispose 时导致 Monaco 内部断言或视觉残留。
  $effect(() => {
    const c = current;
    if (!c?.diffArgs) {
      if (diffCurrentPath !== null) {
        saveDiffViewState();
        diffCurrentPath = null;
        if (diffEditor) diffEditor.setModel(null);
      }
      return;
    }
    if (!diffMountPoint) return;
    // 已在显示同一个 diff tab：不做任何事（保持 keep-alive 现有 model）。
    if (c.path === diffCurrentPath) return;
    // 切到不同的 diff tab：先 detach 当前 model，再 load 新的。
    // detach 在前、load 在后确保中间态没有残留 model 被 display 切换暴露。
    if (diffEditor && diffEditor.getModel()) {
      saveDiffViewState();
      diffEditor.setModel(null);
    }
    void loadDiff(c.diffArgs, c.path);
  });

  // §光标对齐：Monaco 在 create 时即用「当前可用字体」测量并缓存字符宽度。若 webfont
  // （JetBrains Mono 等）尚未加载完成，测的是回退字体的宽度，字体到位后 Monaco 不会自动
  // 重测 → 列宽逐列累积偏差，行末光标视觉左偏。等字体真正就绪后 remeasureFonts() 让
  // Monaco 重测所有实例。remeasureFonts 是全局的，故由下方 effect 的 key 门控，仅在字体
  // 族/字号真正变化时触发，避免无关设置变更引起整页重排抖动。
  let lastFontKey = '';
  async function remeasureWhenFontReady(): Promise<void> {
    try {
      await document.fonts.load(`${editorFontSize}px ${editorFontFamily}`);
      await document.fonts.ready;
      monaco.editor.remeasureFonts();
    } catch {
      /* 无 document.fonts（测试/SSR）或加载失败：忽略，退化为不重测 */
    }
  }

  // 字体设置变化时，让已存在的 editor / diffEditor 实时更新（无需重建）。
  // Monaco 的 updateOptions 是幂等的，重复 set 同值无副作用。
  $effect(() => {
    const key = `${editorFontFamily}|${editorFontSize}`;
    const opts = { fontFamily: editorFontFamily, fontSize: editorFontSize };
    editor?.updateOptions(opts);
    diffEditor?.updateOptions(opts);
    if (key !== lastFontKey) {
      lastFontKey = key;
      void remeasureWhenFontReady();
    }
  });

  // Apply renderSideBySide toggle without a full IPC reload.
  // Monaco 在 inline ↔ sideBySide 切换时，仅 updateOptions 不会重建右侧 diff
  // widget（表面上选项已改但视觉仍是旧模式）。setModel(null) → setModel(real)
  // 的 null-cycle 强制 Monaco 彻底销毁并重建内部 sub-editor，确保立即以新模式渲染。
  // 重建后再 restoreViewState 防止 toggle 时 scroll/折叠位置丢失。
  // diffMountPoint 用 display:none 互斥后，切换模式时还需经过 tick() 确保
  // display 已变为 block 再操作 Monaco。
  $effect(() => {
    const sideBySide = diffRenderSideBySide;
    if (!diffEditor) return;
    diffEditor.updateOptions({ renderSideBySide: sideBySide });
    const cur = diffCurrentPath ? diffModelCache.get(diffCurrentPath) : null;
    if (cur) {
      const vs = diffEditor.saveViewState();
      diffEditor.setModel(null);
      diffEditor.setModel({ original: cur.original, modified: cur.modified });
      if (vs) diffEditor.restoreViewState(vs);
    }
    void tick().then(() => diffEditor?.layout());
  });

  // When switching to a diff tab, the diff mount point transitions from
  // display:none to display:block. Monaco's automaticLayout handles ResizeObserver,
  // but there can be a one-frame gap. Force layout after the DOM settles.
  $effect(() => {
    if (isDiffTab && !diffError && diffEditor) {
      void tick().then(() => diffEditor?.layout());
    }
  });

  // When switching back to a regular editor from a diff tab or preview,
  // the mount point transitions from display:none to display:block.
  $effect(() => {
    const hidden = inPreviewMode || isDiffTab;
    if (!hidden && editor) {
      void tick().then(() => editor?.layout());
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
        await alertDialog({ title: tr('editor.ctxCopyFailed'), message: `${label}: ${err}`, danger: true });
      }
    };

    const items: ContextMenuItem[] = [
      {
        id: 'close',
        label: tr('editor.ctxClose'),
        shortcut: 'Ctrl+W',
        icon: X,
        action: () => void fileEditorStore.closeFile(path),
      },
      {
        id: 'close-others',
        label: tr('editor.ctxCloseOthers'),
        disabled: !hasOthers,
        action: () => void fileEditorStore.closeOthers(path),
      },
      {
        id: 'close-right',
        label: tr('editor.ctxCloseRight'),
        disabled: !hasRight,
        action: () => void fileEditorStore.closeToRight(path),
      },
      {
        id: 'close-saved',
        label: tr('editor.ctxCloseSaved'),
        disabled: !hasSaved,
        action: () => fileEditorStore.closeSaved(),
      },
      {
        id: 'close-all',
        label: tr('editor.ctxCloseAll'),
        action: () => void fileEditorStore.closeAll(),
      },
      { id: 'div1', divider: true },
      {
        id: 'copy-path',
        label: tr('editor.ctxCopyPath'),
        icon: Copy,
        disabled: isDiff,
        action: () => void copyToClipboard(path, tr('editor.ctxCopyPath')),
      },
      {
        id: 'copy-name',
        label: tr('editor.ctxCopyName'),
        icon: Copy,
        disabled: isDiff,
        action: () => void copyToClipboard(file.name, tr('editor.ctxCopyName')),
      },
      { id: 'div2', divider: true },
      {
        id: 'reveal',
        label: tr('editor.ctxReveal'),
        icon: FolderOpen,
        // §web-remote: reveal opens a file manager on the HOST desktop — invisible
        // to a remote/mobile viewer. isTauri() is true under the shim, so gate on
        // the build flag instead to disable it in web-remote.
        disabled: isDiff || !isTauri() || import.meta.env.RIDGE_WEB_REMOTE === true,
        action: () => {
          if (!isTauri() || import.meta.env.RIDGE_WEB_REMOTE === true) return;
          void invoke('reveal_in_file_manager', { path }).catch(async (err) => {
            await alertDialog({ title: tr('editor.ctxOpenFailed'), message: String(err), danger: true });
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
    // De-dup by path before keying the tab `{#each ... (it.id)}`: the store's
    // `openFile` guards against duplicate-path inserts, but a defensive pass
    // here makes a duplicate key (`each_key_duplicate`, which drops/misrenders
    // tabs) unreachable from the render layer even if some future code path
    // slips a dup into `openFiles`. First occurrence wins.
    const seen = new Set<string>();
    const files = editorState.openFiles.filter((f) => {
      if (seen.has(f.path)) return false;
      seen.add(f.path);
      return true;
    });
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
    el.style.zIndex = '300';

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

  // ─── ESC 双击隐藏面板 ──────────────────────────────────────────────────────
  // 第一次 ESC：让 Monaco 自己处理（关 find widget / autocomplete / hover）；
  // 500 ms 内的第二次 ESC（焦点仍在面板内）才隐藏整个面板。capture-phase
  // 监听确保我们先于 Monaco 看到事件，但仅在第二次时 stopPropagation —— 第一次
  // 不阻断让 Monaco 正常清场。焦点不在面板内时不响应（避免在终端等场景把
  // 用户的 ESC 键序列吃掉）。
  let lastEscAt: number | null = null;
  function onWindowKeyDown(e: KeyboardEvent) {
    if (e.key !== 'Escape') return;
    if (!editorState.isVisible) return;
    const ae = document.activeElement as Node | null;
    if (!panelRootEl || !ae || !panelRootEl.contains(ae)) return;
    const now = performance.now();
    if (lastEscAt !== null && now - lastEscAt < 500) {
      e.preventDefault();
      e.stopPropagation();
      lastEscAt = null;
      fileEditorStore.hide();
      return;
    }
    lastEscAt = now;
  }
  onMount(() => {
    window.addEventListener('keydown', onWindowKeyDown, true);
    return () => window.removeEventListener('keydown', onWindowKeyDown, true);
  });

  // ─── Style computations ────────────────────────────────────────────────────
  // §C1 touch (coarse pointer): Monaco's drawer/floating chrome is mouse-only and
  // a narrow drawer is unusable on a phone. Detect once (pointer type is stable
  // per device) and force a full-screen, touch-friendly editor on touch devices.
  const coarsePointer = typeof window !== 'undefined'
    && typeof window.matchMedia === 'function'
    && window.matchMedia('(pointer: coarse)').matches;

  // §独立窗口：本组件运行在弹出的独立 OS 窗口里（URL ?win=editor）。此时编辑器铺满
  // 整个窗口，并隐藏 drawer/floating/embedded 的窗内 chrome（折叠/缩放手柄/显示模式
  // 切换）——这些都会写共享 localStorage prefs，必须在独立窗口里禁用以免污染主窗口。
  const popout = typeof window !== 'undefined'
    && new URLSearchParams(window.location.search).get('win') === 'editor';

  const containerStyle = $derived.by(() => {
    if (!editorState.isVisible || editorState.openFiles.length === 0)
      return 'display: none;';
    // §独立窗口：铺满整个 OS 窗口（必须在 coarsePointer 分支之前判定）。
    if (popout) return 'position: fixed; inset: 0; z-index: 0;';
    // §C1 touch: full-screen overlay below the Dynamic Island / status bar so the
    // code is actually readable/editable (the collapse button still closes it).
    if (coarsePointer) {
      return 'position: fixed; left: 0; right: 0; bottom: 0; top: env(safe-area-inset-top, 0px); z-index: 200;';
    }
    if (editorState.displayMode === 'floating') {
      const r = editorState.floatingRect;
      return `position: fixed; left: ${r.x}px; top: ${r.y}px; width: ${r.w}px; height: ${r.h}px; z-index: 200;`;
    }
    if (editorState.displayMode === 'embedded') {
      // Embedded: part of the normal flex layout — no position:fixed.
      // Width driven by drawerWidth (shared with drawer mode / resizable).
      return `width: ${editorState.drawerWidth}px; flex-shrink: 0;`;
    }
    // drawer: anchored to the right, **below the 44px header bar** so the
    // titlebar + workspace tabs remain visible/interactive (用户反馈：抽屉不能遮挡顶部 header)。
    const TOP_OFFSET = 44;
    // §safe-area: drop the drawer below the header, which itself grows by the
    // top safe-area inset (Dynamic Island / notch) in web-remote on mobile.
    return `position: fixed; top: calc(${TOP_OFFSET}px + env(safe-area-inset-top, 0px)); right: 0; bottom: 0; width: ${editorState.drawerWidth}px; z-index: 200;`;
  });
</script>

<div
  bind:this={panelRootEl}
  class="rg-file-editor flex flex-col bg-[var(--rg-surface-2)]/98 backdrop-blur-xl {popout
    ? ''
    : `border border-[var(--rg-border)] shadow-2xl ${editorState.displayMode === 'floating'
        ? 'rounded-lg overflow-hidden'
        : editorState.displayMode === 'drawer'
          ? 'rounded-l-lg'
          : ''}`}"
  style={containerStyle}
>
  <!-- ═══ Header row 1: actions ═══ -->
  <!-- 紧凑高度（h-7=28px），与下方 tabs 行（h-6）之间不画分隔线 —— 视觉上
       是一个整体的两层 header。pin（floating）模式下整行（按钮以外的空白区）
       作为拖拽手柄。`onFloatingDragStart` 内部已过滤掉 button/input/select。 -->
  <div
    class="rg-editor-toolbar flex items-center shrink-0 h-7 bg-[var(--rg-surface)]/90 {editorState.displayMode ===
      'floating' && !popout
      ? 'cursor-grab active:cursor-grabbing select-none'
      : ''}"
    role="toolbar"
    tabindex="-1"
    aria-label={$t('editor.toolbarAriaLabel')}
    onmousedown={editorState.displayMode === 'floating' && !popout
      ? onFloatingDragStart
      : undefined}
  >
    {#if (editorState.displayMode === 'drawer' || editorState.displayMode === 'embedded') && !popout}
      <button
        type="button"
        class="rg-no-drag flex h-7 w-7 shrink-0 items-center justify-center text-[var(--rg-fg-muted)] hover:bg-[var(--rg-surface)] hover:text-[var(--rg-fg)] transition-colors border-r border-[var(--rg-border)]"
        title={$t('editor.collapsePanel')}
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
          <span class="shrink-0 text-[var(--rg-accent)]">{$t('editor.unsaved')}</span>
        {:else}
          <span class="shrink-0">{$t('editor.saved')}</span>
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
            title={$t('editor.diffSideBySide')}
            onclick={() => (diffRenderSideBySide = true)}
          >
            <Columns class="h-3.5 w-3.5" />
          </button>
          <button
            type="button"
            class="flex h-6 w-7 items-center justify-center text-[var(--rg-fg-muted)] hover:bg-[var(--rg-surface)] hover:text-[var(--rg-fg)] transition-colors {!diffRenderSideBySide ? 'bg-[var(--rg-accent)]/20 text-[var(--rg-accent)]' : ''}"
            title={$t('editor.diffInline')}
            onclick={() => (diffRenderSideBySide = false)}
          >
            <AlignLeft class="h-3.5 w-3.5" />
          </button>
        </div>
          <button
            type="button"
            class="flex h-7 w-7 items-center justify-center rounded text-[var(--rg-fg-muted)] hover:bg-[var(--rg-surface)] hover:text-[var(--rg-fg)] transition-colors"
            title={$t('editor.diffReload')}
            onclick={() => { if (current?.diffArgs) void loadDiff(current.diffArgs, current.path, true); }}
          >
          <RotateCw class="h-3.5 w-3.5 {diffLoading ? 'animate-spin' : ''}" />
        </button>
      {:else}
        <button
          type="button"
          class="flex h-7 w-7 items-center justify-center rounded text-[var(--rg-fg-muted)] hover:bg-[var(--rg-surface)] hover:text-[var(--rg-fg)] transition-colors disabled:opacity-30 disabled:cursor-not-allowed"
          title={$t('editor.find')}
          disabled={!current}
          onclick={triggerFind}
        >
          <Search class="h-3.5 w-3.5" />
        </button>
        <button
          type="button"
          class="flex h-7 w-7 items-center justify-center rounded text-[var(--rg-fg-muted)] hover:bg-[var(--rg-surface)] hover:text-[var(--rg-fg)] transition-colors disabled:opacity-30 disabled:cursor-not-allowed"
          title={$t('editor.save')}
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
          title={$t('editor.settings')}
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
            <!-- §独立窗口：在弹出的独立窗口里隐藏整个「显示模式」段——切 dock 模式无意义
                 且会写共享 prefs 污染主窗口。「独立窗口」动作也只在主窗口出现。 -->
            {#if !popout}
            <div
              class="px-3 py-1 text-[10px] uppercase tracking-wider text-[var(--rg-fg-muted)]"
            >
              {$t('editor.displayMode')}
            </div>
            <button
              type="button"
              class="w-full flex items-center gap-2 px-3 py-1.5 text-left hover:bg-[var(--rg-surface)] transition-colors {editorState.displayMode ===
              'embedded'
                ? 'text-[var(--rg-accent)]'
                : 'text-[var(--rg-fg)]'}"
              onclick={() => setMode('embedded')}
            >
              <PanelRight class="h-3.5 w-3.5" /> {$t('editor.modeEmbedded')}
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
              <PanelRightOpen class="h-3.5 w-3.5" /> {$t('editor.modeDrawer')}
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
              <Pin class="h-3.5 w-3.5" /> {$t('editor.modeFloating')}
              {#if editorState.displayMode === 'floating'}<span
                  class="ml-auto text-[10px]">✓</span
                >{/if}
            </button>
            <!-- §独立窗口：弹出整个编辑器（含所有标签）到一个真正独立的 OS 窗口。
                 非 Tauri / web-remote 下 popOutEditor 自动回退到悬浮模式。 -->
            <button
              type="button"
              class="w-full flex items-center gap-2 px-3 py-1.5 text-left hover:bg-[var(--rg-surface)] transition-colors text-[var(--rg-fg)]"
              onclick={() => {
                settingsOpen = false;
                void popOutEditor();
              }}
            >
              <ExternalLink class="h-3.5 w-3.5" /> {$t('editor.modeWindow')}
            </button>
            {/if}

            <div class="my-1 border-t border-[var(--rg-border)]"></div>
            <button
              type="button"
              class="w-full flex items-center gap-2 px-3 py-1.5 text-left hover:bg-[var(--rg-surface)] transition-colors text-[var(--rg-fg)] disabled:opacity-30 disabled:cursor-not-allowed"
              disabled={!current?.isDirty}
              onclick={() => {
                revertActive();
              }}
            >
              <RotateCcw class="h-3.5 w-3.5" /> {$t('editor.revertFile')}
            </button>
            <button
              type="button"
              class="w-full flex items-center gap-2 px-3 py-1.5 text-left hover:bg-[var(--rg-surface)] transition-colors text-[var(--rg-fg)]"
              onclick={() => closeAll()}
            >
              <XCircle class="h-3.5 w-3.5" /> {$t('editor.closeAllTabs')}
            </button>
            {#if !popout}
            <button
              type="button"
              class="w-full flex items-center gap-2 px-3 py-1.5 text-left hover:bg-[var(--rg-surface)] transition-colors text-[var(--rg-fg-muted)]"
              onclick={() => {
                settingsOpen = false;
                hidePanel();
              }}
            >
              {$t('editor.hidePanel')}
            </button>
            {/if}
          </div>
        {/if}
      </div>
      {#if editorState.displayMode === 'floating' && !popout}
        <!-- pin 模式专属关闭按钮：drawer 模式下左侧已经有收起按钮，这里
               重复一次反而冗余；只有 floating 时为了贴合标准窗口的"关闭在
               右上角"心智模型才显示。功能与左侧收起一致——隐藏面板，
               不销毁打开的文件。 -->
        <button
          type="button"
          class="rg-no-drag flex h-7 w-7 items-center justify-center rounded text-[var(--rg-fg-muted)] hover:bg-red-500/10 hover:text-red-300 transition-colors"
          title={$t('editor.closeFloating')}
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
            title={f.external === 'deleted' ? $t('editor.externalDeleted', { name: f.name }) : f.name}
          >{f.name}</span>
          {#if f.external === 'deleted'}
            <span
              class="text-[10px] px-1 py-px rounded bg-red-500/15 text-red-500 leading-none"
              title={$t('editor.externalDeletedTitle')}
            >{$t('editor.deletedBadge')}</span>
          {/if}
          {#if f.isDirty}
            <span
              class="inline-block h-1.5 w-1.5 rounded-full bg-[var(--rg-accent)]"
              title={$t('editor.unsavedDot')}
            ></span>
          {/if}
          <span
            role="button"
            tabindex="0"
            class="rg-tab-close flex h-4 w-4 items-center justify-center rounded text-[var(--rg-fg-muted)] hover:bg-[var(--rg-bg)]/50 hover:text-[var(--rg-fg)] {f.isDirty
              ? ''
              : 'opacity-0 group-hover:opacity-100'} transition-opacity"
            onmousedown={blockEditorTabDragStart}
            ontouchstart={blockEditorTabDragStart}
            onpointerdown={blockEditorTabDragStart}
            onclick={(e) => closeTab(e, f.path)}
            onkeydown={(e) =>
              (e.key === 'Enter' || e.key === ' ') &&
              closeTab(e as unknown as MouseEvent, f.path)}
            title={$t('editor.closeTab')}
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
    <!-- Regular editor。使用 display:none 实现严格互斥：两个 mount 点都是 absolute
         inset-0，用 visibility:hidden 时二者同时处在渲染树中，Monaco 内部可能把
         visibility:hidden 子元素重新置为 visible，导致点击被隐藏元素吞掉（"点不进
         编辑器"）。display:none 完全移除渲染层，无此问题；automaticLayout 在切换
         回 display:block 后由 ResizeObserver 自动恢复，下方 layout() effect 加倍
         保障。 -->
    <div
      bind:this={mountPoint}
      class="absolute inset-0 rg-monaco-host"
      style={inPreviewMode || isDiffTab ? 'display: none;' : ''}
    ></div>

    <!-- Diff editor mount point — always in DOM so bind:this is stable -->
    <div
      bind:this={diffMountPoint}
      class="absolute inset-0 rg-monaco-host"
      style={!isDiffTab || !!diffError ? 'display: none;' : ''}
    ></div>

    <!-- Diff loading / error overlays -->
    {#if isDiffTab && diffLoading}
      <div class="absolute top-2 right-3 text-[10px] text-[var(--rg-fg-muted)] bg-[var(--rg-surface)]/80 px-2 py-0.5 rounded pointer-events-none">
        {$t('editor.diffLoading')}
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
    onerror={(e) => {
      const img = e.currentTarget as HTMLImageElement;
      console.warn('[FileEditor] image failed to load', current!.path, img.src);
    }}
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
          ? $t('editor.switchToSource')
          : $t('editor.switchToPreview')}
        onclick={() =>
          fileEditorStore.setViewMode(
            current!.path,
            inPreviewMode ? 'source' : 'preview'
          )}
      >
        {#if inPreviewMode}
          <Code2 class="h-3.5 w-3.5" />
          <span>{$t('editor.sourceLabel')}</span>
        {:else}
          <Eye class="h-3.5 w-3.5" />
          <span>{$t('editor.previewLabel')}</span>
        {/if}
      </button>
    {/if}
  </div>

  <!-- ═══ Drawer / Embedded left-edge resizer ═══ -->
  <!-- §C1 hidden on touch (mouse-only drag; the editor is full-screen there). -->
  {#if (editorState.displayMode === 'drawer' || editorState.displayMode === 'embedded') && !coarsePointer && !popout}
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <div
      class="absolute left-0 top-0 bottom-0 w-1 cursor-col-resize hover:bg-[var(--rg-accent)]/40 transition-colors {isResizingDrawer
        ? 'bg-[var(--rg-accent)]/60'
        : ''}"
      role="separator"
      aria-orientation="vertical"
      aria-label={$t('editor.resizeEditorWidth')}
      onmousedown={onDrawerResizeStart}
    ></div>
  {/if}

  <!-- ═══ Floating resize handles ═══
         tabindex=0 + onkeydown 让键盘用户也能通过 Arrow 键调整大小（Shift 加速）。
         Svelte 的 `a11y_no_noninteractive_*` 规则不认识 role=separator + 互补
         keydown 这个合法的 "window splitter" 模式，所以为每个 handle 显式抑制。
         参考 WAI-ARIA authoring practices: separator 可聚焦并响应 Arrow 键。 -->
  {#if editorState.displayMode === 'floating' && !coarsePointer && !popout}
    <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <div
      class="rg-float-handle rg-h-n"
      role="separator"
      aria-label={$t('editor.resizeTop')}
      tabindex="0"
      onmousedown={(e) => onFloatingResizeStart(e, 'n')}
      onkeydown={(e) => onFloatingResizeKey(e, 'n')}
    ></div>
    <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <div
      class="rg-float-handle rg-h-s"
      role="separator"
      aria-label={$t('editor.resizeBottom')}
      tabindex="0"
      onmousedown={(e) => onFloatingResizeStart(e, 's')}
      onkeydown={(e) => onFloatingResizeKey(e, 's')}
    ></div>
    <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <div
      class="rg-float-handle rg-h-e"
      role="separator"
      aria-label={$t('editor.resizeRight')}
      tabindex="0"
      onmousedown={(e) => onFloatingResizeStart(e, 'e')}
      onkeydown={(e) => onFloatingResizeKey(e, 'e')}
    ></div>
    <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <div
      class="rg-float-handle rg-h-w"
      role="separator"
      aria-label={$t('editor.resizeLeft')}
      tabindex="0"
      onmousedown={(e) => onFloatingResizeStart(e, 'w')}
      onkeydown={(e) => onFloatingResizeKey(e, 'w')}
    ></div>
    <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <div
      class="rg-float-handle rg-h-ne"
      role="separator"
      aria-label={$t('editor.resizeNE')}
      tabindex="0"
      onmousedown={(e) => onFloatingResizeStart(e, 'ne')}
      onkeydown={(e) => onFloatingResizeKey(e, 'ne')}
    ></div>
    <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <div
      class="rg-float-handle rg-h-nw"
      role="separator"
      aria-label={$t('editor.resizeNW')}
      tabindex="0"
      onmousedown={(e) => onFloatingResizeStart(e, 'nw')}
      onkeydown={(e) => onFloatingResizeKey(e, 'nw')}
    ></div>
    <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <div
      class="rg-float-handle rg-h-se"
      role="separator"
      aria-label={$t('editor.resizeSE')}
      tabindex="0"
      onmousedown={(e) => onFloatingResizeStart(e, 'se')}
      onkeydown={(e) => onFloatingResizeKey(e, 'se')}
    ></div>
    <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <div
      class="rg-float-handle rg-h-sw"
      role="separator"
      aria-label={$t('editor.resizeSW')}
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
  /* §IDE 行级 Git blame：行尾灰字注释（作者 · 相对时间 · 摘要）。不抢眼、不可选。 */
  :global(.rg-blame-annotation) {
    color: var(--rg-fg-muted, #8b8b9a);
    opacity: 0.65;
    font-style: italic;
    user-select: none;
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
