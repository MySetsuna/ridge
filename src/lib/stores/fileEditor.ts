// src/lib/stores/fileEditor.ts
//
// Global, per-window file editor: a drawer (default) or floating pin window that
// holds open code files. One store instance per window; all explorer/file-tree
// actions route through openFile(). Content is kept as a plain string — Monaco
// owns the text buffer inside the component, this store tracks metadata + dirty
// state + cross-tab coordination.

import { writable, get, derived } from 'svelte/store';
import { invoke, isTauri } from '@tauri-apps/api/core';
import { convertFileSrc } from '@tauri-apps/api/core';
import { isMarkdownPath } from '$lib/utils/markdown';
import { isRecentlyWritten, markRecentlyWritten } from './fsEvents';
import { alertDialog, choiceDialog, confirmDialog } from '$lib/components/RidgeDialog.svelte';

/** 图片文件扩展名 */
const IMAGE_EXTS = ['png', 'jpg', 'jpeg', 'gif', 'webp', 'svg', 'ico', 'bmp'];

export type EditorDisplayMode = 'drawer' | 'floating' | 'embedded';

export interface OpenFile {
  /** Absolute path on disk — also the tab's stable identity. */
  path: string;
  /** Display name (basename). */
  name: string;
  /** Current editor content (synced from Monaco on every edit). */
  content: string;
  /** Content last persisted to disk — for dirty detection and discard. */
  originalContent: string;
  /** Monaco language id inferred from extension. */
  language: string;
  /** True iff content !== originalContent. */
  isDirty: boolean;
  /** Order added; used to restore order when switching tabs. */
  openedAt: number;
  /**
   * View mode. Relevant for languages with a dedicated preview renderer (markdown).
   * Markdown files default to 'preview'; everything else is 'source'.
   */
  viewMode: 'source' | 'preview';
  /** True for image files */
  isImage: boolean;
  /** Image URL (for image files) */
  imageUrl?: string;
  /**
   * If set, this tab is a read-only Monaco diff view.
   *  - `commit` 设置时显示 `<commit>^` vs `<commit>` 的 diff（GitGraph 的"查看 commit diff"）；
   *  - 否则按 `cached` 走 staged (HEAD vs index) 或 working (index vs disk)。
   * Tab path: `__diff__:<staged|working|commit>:<repoRoot>:<filePath>` 或加上 commit hash。
   */
  diffArgs?: { repoRoot: string; path: string; cached: boolean; commit?: string };
  /**
   * External-state marker. `'deleted'` means a filesystem watcher reported
   * the file was removed off-disk; the tab stays open so the user can salvage
   * unsaved edits or save (which recreates the file). Cleared on a successful
   * save or external re-creation.
   */
  external?: 'deleted';
}

export interface FloatingRect {
  x: number;
  y: number;
  w: number;
  h: number;
}

/**
 * Jump target for the Monaco editor. Set by callers (e.g. the Search sidebar
 * clicking a match) and consumed by FileEditor.svelte after it mounts the
 * matching model. Keyed by path — one pending jump per file, latest wins.
 */
export interface PendingReveal {
  path: string;
  /** 1-based Monaco line number. */
  line: number;
  /** 1-based column. Defaults to 1 when the caller doesn't care. */
  column: number;
  /** 命中文本长度（字符数）。> 0 时 FileEditor 会在 [column, column+matchLength)
   *  这段加一段瞬时高亮装饰，2.5s 后自动消失。来自搜索 sidebar 的命中点击。 */
  matchLength?: number;
}

export interface FileEditorState {
  openFiles: OpenFile[];
  activePath: string | null;
  displayMode: EditorDisplayMode;
  isVisible: boolean;
  /** Drawer width in px. Persisted. Min 280, max 70% of window. */
  drawerWidth: number;
  /** Floating window rect. Persisted. */
  floatingRect: FloatingRect;
  /**
   * Single-shot reveal request. `FileEditor.svelte` reads this after a model
   * swap / selection change and nulls it out via `consumePendingReveal`.
   * Always one-shot: preservation across tab switches would compete with the
   * user's manual cursor movement.
   */
  pendingReveal: PendingReveal | null;
  /**
   * 当前一轮搜索的全部命中（跨文件）。FileEditor 在打开的文件里把所有匹配本
   * 文件 path 的 entry 一起画成 Monaco decoration，**只要搜索 query 不变就
   * 保留**。SearchSidebar 在自己的 results 数组变化时整体写入；query 改 →
   * results 变 → 这个数组同步刷新（清空时同样反应到 decoration 清除）。 */
  searchHits: SearchHit[];
}

/** 搜索 sidebar 给 FileEditor 的命中描述。1-based line/column。 */
export interface SearchHit {
  path: string;
  line: number;
  column: number;
  matchLength: number;
}

const LS_KEY = 'ridge-file-editor-prefs';
const MIN_W = 320;
const MIN_H = 240;
/**
 * Left sidebar icon strip width — floating editor is forbidden from overlapping
 * this zone (spec: "悬浮在所有页面的最上方，除了侧边条tab区域").
 */
export const SIDEBAR_TAB_W = 52;

/** 顶部 workspace tab 行高（与 +page.svelte 中 `h-11` 一致）。
 *  pin 模式悬浮窗的最小 Y 值锁到这里，保证不会盖住主应用 tab 区。 */
export const APP_HEADER_HEIGHT = 44;

function loadPrefs(): Partial<FileEditorState> {
  if (typeof localStorage === 'undefined') return {};
  try {
    const raw = localStorage.getItem(LS_KEY);
    if (!raw) return {};
    return JSON.parse(raw);
  } catch {
    return {};
  }
}

function savePrefs(s: FileEditorState): void {
  if (typeof localStorage === 'undefined') return;
  try {
    localStorage.setItem(
      LS_KEY,
      JSON.stringify({
        displayMode: s.displayMode,
        drawerWidth: s.drawerWidth,
        floatingRect: s.floatingRect,
      })
    );
  } catch {
    /* ignore quota/privacy errors */
  }
}

function defaultFloatingRect(): FloatingRect {
  if (typeof window === 'undefined') return { x: 200, y: APP_HEADER_HEIGHT + 8, w: 720, h: 540 };
  const w = Math.min(720, Math.max(MIN_W, window.innerWidth * 0.5));
  const h = Math.min(540, Math.max(MIN_H, window.innerHeight * 0.65));
  const x = Math.max(SIDEBAR_TAB_W + 8, Math.floor((window.innerWidth - w) / 2));
  const y = Math.max(APP_HEADER_HEIGHT, Math.floor((window.innerHeight - h) / 2));
  return { x, y, w, h };
}

const prefs = loadPrefs();

const initial: FileEditorState = {
  openFiles: [],
  activePath: null,
  displayMode: (prefs.displayMode as EditorDisplayMode) ?? 'drawer',
  isVisible: false,
  drawerWidth: typeof prefs.drawerWidth === 'number' ? prefs.drawerWidth : 520,
  // 持久化的 rect 可能源自旧版本（minY=0 时存的），再 clamp 一次确保
  // 不会落在被 workspace tab 覆盖的区域。
  floatingRect: clampRectToViewport((prefs.floatingRect as FloatingRect) ?? defaultFloatingRect()),
  pendingReveal: null,
  searchHits: [],
};

export function langFromPath(path: string): string {
  const lower = path.toLowerCase();
  const ext = lower.split(/[.\\/]/).pop() || '';
  const map: Record<string, string> = {
    ts: 'typescript',
    tsx: 'typescript',
    js: 'javascript',
    jsx: 'javascript',
    mjs: 'javascript',
    cjs: 'javascript',
    svelte: 'html',
    vue: 'html',
    html: 'html',
    htm: 'html',
    css: 'css',
    scss: 'scss',
    sass: 'scss',
    less: 'less',
    json: 'json',
    md: 'markdown',
    markdown: 'markdown',
    py: 'python',
    rs: 'rust',
    go: 'go',
    java: 'java',
    kt: 'kotlin',
    kts: 'kotlin',
    c: 'c',
    h: 'c',
    cpp: 'cpp',
    cc: 'cpp',
    hpp: 'cpp',
    cs: 'csharp',
    rb: 'ruby',
    php: 'php',
    sh: 'shell',
    bash: 'shell',
    zsh: 'shell',
    fish: 'shell',
    ps1: 'powershell',
    psm1: 'powershell',
    yaml: 'yaml',
    yml: 'yaml',
    toml: 'ini',
    ini: 'ini',
    xml: 'xml',
    sql: 'sql',
    lua: 'lua',
    dart: 'dart',
    swift: 'swift',
    r: 'r',
    dockerfile: 'dockerfile',
  };
  if (lower.endsWith('/dockerfile') || lower === 'dockerfile') return 'dockerfile';
  return map[ext] ?? 'plaintext';
}

function basename(p: string): string {
  return p.split(/[/\\]/).filter(Boolean).pop() || p;
}

function isImagePath(path: string): boolean {
  const lower = path.toLowerCase();
  const ext = lower.split(/[.\\/]/).pop() || '';
  return IMAGE_EXTS.includes(ext);
}

function createStore() {
  const { subscribe, update, set } = writable<FileEditorState>(initial);

  function persist() {
    savePrefs(get({ subscribe }));
  }

  return {
    subscribe,

    /**
     * Open a file (or activate its existing tab). Auto-shows the editor.
     *
     * `opts.line` / `opts.column` (both 1-based) stash a one-shot reveal
     * request which `FileEditor.svelte` consumes after the Monaco model is
     * swapped in. Subsequent manual cursor moves stay where the user put them.
     */
    async openFile(
      path: string,
      opts?: { line?: number; column?: number; matchLength?: number }
    ): Promise<void> {
      const reveal: PendingReveal | null = opts?.line && opts.line > 0
        ? {
            path,
            line: opts.line,
            column: Math.max(1, opts.column ?? 1),
            matchLength: opts.matchLength && opts.matchLength > 0 ? opts.matchLength : undefined,
          }
        : null;
      const state = get({ subscribe });
      const existing = state.openFiles.find((f) => f.path === path);
      if (existing) {
        // Already-open tab: re-read from disk so the editor reflects
        // any external mutations that happened while the tab was
        // hidden / inactive (terminal commands, git pull, another
        // editor, AI agent writes). Skip the re-read when:
        //   - the tab has unsaved edits (isDirty) — overwriting would
        //     silently destroy the user's work; the existing
        //     file-watcher prompt path handles that case.
        //   - the tab is a diff view (handled separately via its own
        //     reload control in the toolbar).
        //   - the file is an image / binary (no in-place reload path).
        //   - we're outside Tauri (no `read_file_for_editor`).
        const canReload =
          !existing.isDirty &&
          !existing.diffArgs &&
          !existing.isImage &&
          isTauri();
        if (canReload) {
          try {
            const result = await invoke<{ content: string; is_binary: boolean; size: number }>(
              'read_file_for_editor',
              { path }
            );
            if (!result.is_binary) {
              update((s) => ({
                ...s,
                openFiles: s.openFiles.map((f) =>
                  f.path === path
                    ? { ...f, content: result.content, originalContent: result.content, isDirty: false }
                    : f
                ),
                activePath: path,
                isVisible: true,
                pendingReveal: reveal ?? s.pendingReveal,
              }));
              return;
            }
            // Binary now where it was text before — fall through to the
            // simple activate path; we don't try to switch view mode.
          } catch (e) {
            // Read failed (deleted, permission lost, etc.) — keep the
            // last known content visible; the user can react via the
            // existing fs-event prompt or a manual reload.
            console.warn('[fileEditor] re-read on focus failed', path, e);
          }
        }
        update((s) => ({
          ...s,
          activePath: path,
          isVisible: true,
          pendingReveal: reveal ?? s.pendingReveal,
        }));
        return;
      }

      // 图片文件特殊处理：不需要读取内容，直接用 convertFileSrc 生成 URL
      const isImage = isImagePath(path);
      let imageUrl: string | undefined;

      let content = '';
      let isBinary = false;
      if (isImage) {
        // 图片文件：使用 Tauri 的 convertFileSrc 生成 asset URL。
        // Windows 路径统一成正斜杠后再传，避免某些 webview 把混合分隔符
        // 解析成 `https://asset.localhost//C%3A%2Fxxx` 这种双斜杠形式
        // 导致中文 / 含空格 / 含特殊字符的文件名加载失败。
        if (isTauri()) {
          const normalized = path.replace(/\\/g, '/');
          imageUrl = convertFileSrc(normalized);
        } else {
          // 非 Tauri 环境（开发模式）使用 file:// 协议
          imageUrl = path.replace(/\\/g, '/');
          if (!imageUrl.startsWith('/')) {
            imageUrl = '/' + imageUrl;
          }
          imageUrl = 'file://' + imageUrl;
        }
      } else if (isTauri()) {
        try {
          const result = await invoke<{ content: string; is_binary: boolean; size: number }>(
            'read_file_for_editor',
            { path }
          );
          if (result.is_binary) {
            isBinary = true;
          }
          content = result.content;
        } catch (e) {
          console.error('read_file_for_editor failed', path, e);
          await alertDialog({ title: '打开文件失败', message: String(e), danger: true });
          return;
        }
      }
      if (isBinary && !isImage) {
        await alertDialog({ title: '无法打开', message: '二进制文件暂不支持在编辑器中打开。' });
        return;
      }

      const file: OpenFile = {
        path,
        name: basename(path),
        content,
        originalContent: content,
        language: isImage ? 'image' : langFromPath(path),
        isDirty: false,
        openedAt: Date.now(),
        // markdown 默认进 preview；其他语言没有 preview 概念，统一 source。
        viewMode: isMarkdownPath(path) ? 'preview' : 'source',
        isImage,
        imageUrl,
      };
      update((s) => ({
        ...s,
        openFiles: [...s.openFiles, file],
        activePath: path,
        isVisible: true,
        pendingReveal: reveal ?? s.pendingReveal,
      }));
    },

    /** SearchSidebar 在 results 数组变化时整体写入；空数组等价 clear。 */
    setSearchHits(hits: SearchHit[]): void {
      update((s) => ({ ...s, searchHits: hits }));
    },
    /** 显式清掉所有搜索命中高亮（与 setSearchHits([]) 等价）。 */
    clearSearchHits(): void {
      update((s) => (s.searchHits.length ? { ...s, searchHits: [] } : s));
    },

    /**
     * Read + clear the one-shot reveal target for `path`. Returns null if no
     * reveal is queued or if it's for a different path (callers should only
     * consume after they've finished mounting the matching Monaco model).
     */
    consumePendingReveal(path: string): PendingReveal | null {
      const state = get({ subscribe });
      const r = state.pendingReveal;
      if (!r || r.path !== path) return null;
      update((s) => ({ ...s, pendingReveal: null }));
      return r;
    },

    /** Switch to a tab. No-op if unknown. */
    setActive(path: string): void {
      update((s) => {
        if (!s.openFiles.some((f) => f.path === path)) return s;
        return { ...s, activePath: path };
      });
    },

    /** Set the view mode (source/preview) for the given tab. */
    setViewMode(path: string, mode: 'source' | 'preview'): void {
      update((s) => ({
        ...s,
        openFiles: s.openFiles.map((f) => (f.path === path ? { ...f, viewMode: mode } : f)),
      }));
    },

    /** Reorder open tabs (drag-and-drop). No-op if indices equal or OOB. */
    reorder(fromIndex: number, toIndex: number): void {
      update((s) => {
        if (
          fromIndex === toIndex ||
          fromIndex < 0 ||
          toIndex < 0 ||
          fromIndex >= s.openFiles.length ||
          toIndex >= s.openFiles.length
        ) {
          return s;
        }
        const next = [...s.openFiles];
        const [moved] = next.splice(fromIndex, 1);
        next.splice(toIndex, 0, moved);
        return { ...s, openFiles: next };
      });
    },

    /** 按 paths 给定的新顺序重排 openFiles。`paths` 必须是当前 openFiles
     *  里 path 的一个排列，否则保持原状（防御性）。供 svelte-dnd-action
     *  finalize 直接调用，避免再倒推 from/to。 */
    setOrder(paths: string[]): void {
      update((s) => {
        if (paths.length !== s.openFiles.length) return s;
        const byPath = new Map(s.openFiles.map((f) => [f.path, f]));
        const next: typeof s.openFiles = [];
        for (const p of paths) {
          const f = byPath.get(p);
          if (!f) return s;
          next.push(f);
        }
        return { ...s, openFiles: next };
      });
    },

    /** Mirror Monaco edits back into the store — no disk write. */
    updateContent(path: string, content: string): void {
      update((s) => ({
        ...s,
        openFiles: s.openFiles.map((f) =>
          f.path === path ? { ...f, content, isDirty: content !== f.originalContent } : f
        ),
      }));
    },

    /** Close a tab; prompts if dirty. Returns true if closed. */
    async closeFile(path: string, confirmDirty = true): Promise<boolean> {
      const state = get({ subscribe });
      const file = state.openFiles.find((f) => f.path === path);
      if (!file) return true;
      if (file.isDirty && confirmDirty) {
        const ok = await confirmDialog({
          title: '关闭未保存的标签页',
          message: `"${file.name}" 有未保存的修改，确认关闭？`,
          okLabel: '关闭',
          danger: true,
        });
        if (!ok) return false;
      }
      update((s) => {
        const remaining = s.openFiles.filter((f) => f.path !== path);
        let next: string | null = s.activePath;
        if (s.activePath === path) {
          const idx = s.openFiles.findIndex((f) => f.path === path);
          next = remaining[Math.min(idx, remaining.length - 1)]?.path ?? null;
        }
        return {
          ...s,
          openFiles: remaining,
          activePath: next,
          isVisible: remaining.length > 0 ? s.isVisible : false,
        };
      });
      return true;
    },

    /** Close all tabs; prompts once if any are dirty. */
    async closeAll(): Promise<void> {
      const state = get({ subscribe });
      const anyDirty = state.openFiles.some((f) => f.isDirty);
      if (anyDirty) {
        const ok = await confirmDialog({
          title: '关闭全部',
          message: '存在未保存的修改，全部关闭？',
          okLabel: '全部关闭',
          danger: true,
        });
        if (!ok) return;
      }
      update((s) => ({ ...s, openFiles: [], activePath: null, isVisible: false }));
    },

    /** 关闭除指定 path 外的所有 tab。脏 tab 集合非空时统一弹一次确认。 */
    async closeOthers(keepPath: string): Promise<void> {
      const state = get({ subscribe });
      const others = state.openFiles.filter((f) => f.path !== keepPath);
      if (others.length === 0) return;
      const dirtyOthers = others.filter((f) => f.isDirty);
      if (dirtyOthers.length > 0) {
        const ok = await confirmDialog({
          title: '关闭其他标签页',
          message: `${dirtyOthers.length} 个未保存的标签页将被关闭，确认？`,
          okLabel: '关闭',
          danger: true,
        });
        if (!ok) return;
      }
      update((s) => {
        const remaining = s.openFiles.filter((f) => f.path === keepPath);
        return {
          ...s,
          openFiles: remaining,
          activePath: keepPath,
          isVisible: remaining.length > 0 ? s.isVisible : false,
        };
      });
    },

    /** 关闭指定 path 右侧的所有 tab（不含自身）。 */
    async closeToRight(anchorPath: string): Promise<void> {
      const state = get({ subscribe });
      const idx = state.openFiles.findIndex((f) => f.path === anchorPath);
      if (idx === -1) return;
      const toClose = state.openFiles.slice(idx + 1);
      if (toClose.length === 0) return;
      const dirty = toClose.filter((f) => f.isDirty);
      if (dirty.length > 0) {
        const ok = await confirmDialog({
          title: '关闭右侧标签页',
          message: `右侧有 ${dirty.length} 个未保存的标签页将被关闭，确认？`,
          okLabel: '关闭',
          danger: true,
        });
        if (!ok) return;
      }
      update((s) => {
        const closeSet = new Set(toClose.map((f) => f.path));
        const remaining = s.openFiles.filter((f) => !closeSet.has(f.path));
        let next: string | null = s.activePath;
        if (s.activePath != null && closeSet.has(s.activePath)) {
          next = anchorPath;
        }
        return {
          ...s,
          openFiles: remaining,
          activePath: next,
          isVisible: remaining.length > 0 ? s.isVisible : false,
        };
      });
    },

    /** 关闭所有未脏（已保存或从未编辑过）的 tab。脏 tab 全部保留，无确认。 */
    closeSaved(): void {
      update((s) => {
        const remaining = s.openFiles.filter((f) => f.isDirty);
        let next: string | null = s.activePath;
        if (s.activePath != null && !remaining.some((f) => f.path === s.activePath)) {
          next = remaining[0]?.path ?? null;
        }
        return {
          ...s,
          openFiles: remaining,
          activePath: next,
          isVisible: remaining.length > 0 ? s.isVisible : false,
        };
      });
    },

    /** Save the active tab to disk. */
    async saveActive(): Promise<void> {
      const state = get({ subscribe });
      const file = state.openFiles.find((f) => f.path === state.activePath);
      if (!file) return;
      await this.saveFile(file.path);
    },

    async saveFile(path: string): Promise<void> {
      const state = get({ subscribe });
      const file = state.openFiles.find((f) => f.path === path);
      if (!file) return;
      if (!file.isDirty) return;
      if (!isTauri()) {
        update((s) => ({
          ...s,
          openFiles: s.openFiles.map((f) =>
            f.path === path ? { ...f, originalContent: f.content, isDirty: false } : f
          ),
        }));
        return;
      }
      try {
        await invoke('write_file', { path, content: file.content });
        // Suppress the round-trip fs-changed event for ~800ms so the editor
        // doesn't prompt "this file changed externally" right after our own save.
        markRecentlyWritten(path);
        update((s) => ({
          ...s,
          openFiles: s.openFiles.map((f) =>
            f.path === path
              ? { ...f, originalContent: f.content, isDirty: false, external: undefined }
              : f
          ),
        }));
      } catch (e) {
        await alertDialog({ title: '保存失败', message: String(e), danger: true });
      }
    },

    /**
     * Reconcile an open file with an external on-disk change reported by the
     * filesystem watcher. Behaviour:
     *
     * - `isRecentlyWritten(path)` → silent (Ridge's own save round-tripping back).
     * - File can no longer be read → mark `external: 'deleted'`; user keeps
     *   the tab and can re-save to recreate.
     * - File is image / diff tab → ignored (no editable content path).
     * - Clean (non-dirty) → silently sync new content into Monaco.
     * - Dirty → ask via `choiceDialog`: reload-discard / keep-editing.
     */
    async handleExternalChange(path: string): Promise<void> {
      const state = get({ subscribe });
      const file = state.openFiles.find((f) => f.path === path);
      if (!file) return;
      if (file.diffArgs) return;
      if (file.isImage) return;
      if (isRecentlyWritten(path)) return;
      if (!isTauri()) return;

      let result: { content: string; is_binary: boolean };
      try {
        result = await invoke<{ content: string; is_binary: boolean; size: number }>(
          'read_file_for_editor',
          { path }
        );
      } catch {
        // Read failed — assume the file was deleted/moved.
        update((s) => ({
          ...s,
          openFiles: s.openFiles.map((f) =>
            f.path === path ? { ...f, external: 'deleted' } : f
          ),
        }));
        return;
      }
      if (result.is_binary) return;

      // File came back: clear any prior 'deleted' marker.
      const fresh = result.content;
      if (fresh === file.content && fresh === file.originalContent && !file.external) {
        // Truly no-op (e.g. mtime touch with identical bytes).
        return;
      }

      if (!file.isDirty) {
        update((s) => ({
          ...s,
          openFiles: s.openFiles.map((f) =>
            f.path === path
              ? { ...f, content: fresh, originalContent: fresh, isDirty: false, external: undefined }
              : f
          ),
        }));
        return;
      }

      // Dirty: ask the user. The dialog is non-blocking for the rest of the
      // app, but we await for this code path so concurrent fs events on the
      // same path queue up serially.
      const choice = await choiceDialog({
        title: '文件已在外部被修改',
        message: `"${file.name}" 已在 Ridge 之外被修改，但你有未保存的改动。`,
        okLabel: '重载并丢弃修改',
        secondaryLabel: '保留当前编辑',
        cancelLabel: '取消',
        danger: true,
      });
      if (choice === 'primary') {
        update((s) => ({
          ...s,
          openFiles: s.openFiles.map((f) =>
            f.path === path
              ? { ...f, content: fresh, originalContent: fresh, isDirty: false, external: undefined }
              : f
          ),
        }));
      } else {
        // Keep-current: rebase originalContent on the new disk version so the
        // dirty flag reflects "differs from disk now", and a subsequent save
        // doesn't get short-circuited as "no change".
        update((s) => ({
          ...s,
          openFiles: s.openFiles.map((f) =>
            f.path === path
              ? {
                  ...f,
                  originalContent: fresh,
                  isDirty: f.content !== fresh,
                  external: undefined,
                }
              : f
          ),
        }));
      }
    },

    /** Revert active tab to disk contents, discarding edits. */
    async revertActive(): Promise<void> {
      const state = get({ subscribe });
      const file = state.openFiles.find((f) => f.path === state.activePath);
      if (!file) return;
      if (file.isDirty) {
        const ok = await confirmDialog({
          title: '放弃修改',
          message: '放弃所有未保存的修改？',
          okLabel: '放弃',
          danger: true,
        });
        if (!ok) return;
      }
      if (!isTauri()) {
        update((s) => ({
          ...s,
          openFiles: s.openFiles.map((f) =>
            f.path === file.path ? { ...f, content: f.originalContent, isDirty: false } : f
          ),
        }));
        return;
      }
      try {
        const result = await invoke<{ content: string; is_binary: boolean }>(
          'read_file_for_editor',
          { path: file.path }
        );
        if (result.is_binary) return;
        update((s) => ({
          ...s,
          openFiles: s.openFiles.map((f) =>
            f.path === file.path
              ? { ...f, content: result.content, originalContent: result.content, isDirty: false }
              : f
          ),
        }));
      } catch (e) {
        await alertDialog({ title: '重载失败', message: String(e), danger: true });
      }
    },

    setDisplayMode(mode: EditorDisplayMode): void {
      update((s) => ({ ...s, displayMode: mode }));
      persist();
    },

    setDrawerWidth(width: number): void {
      const w = Math.max(280, Math.min(width, typeof window !== 'undefined' ? window.innerWidth * 0.8 : 800));
      update((s) => ({ ...s, drawerWidth: w }));
      persist();
    },

    setFloatingRect(rect: FloatingRect): void {
      const clamped = clampRectToViewport(rect);
      update((s) => ({ ...s, floatingRect: clamped }));
      persist();
    },

    toggleVisibility(): void {
      update((s) => ({ ...s, isVisible: !s.isVisible }));
    },

    show(): void {
      update((s) => ({ ...s, isVisible: true }));
    },

    hide(): void {
      update((s) => ({ ...s, isVisible: false }));
    },

    /**
     * Open a diff tab (or activate the existing one).
     * Tab path:
     *   - commit 模式：`__diff__:commit:<shortHash>:<repoRoot>:<filePath>`
     *   - staged：    `__diff__:staged:<repoRoot>:<filePath>`
     *   - working：   `__diff__:working:<repoRoot>:<filePath>`
     */
    openDiffTab(args: { repoRoot: string; path: string; cached: boolean; commit?: string }): void {
      const repoNorm = args.repoRoot.replace(/\\/g, '/');
      const tabPath = args.commit
        ? `__diff__:commit:${args.commit.slice(0, 7)}:${repoNorm}:${args.path}`
        : `__diff__:${args.cached ? 'staged' : 'working'}:${repoNorm}:${args.path}`;
      const filePart = args.path.split('/').pop() ?? args.path;
      const label = args.commit
        ? `@${args.commit.slice(0, 7)}`
        : args.cached
          ? '已暂存'
          : '工作区';
      const name = `${filePart} (${label})`;

      update((s) => {
        const existing = s.openFiles.findIndex((f) => f.path === tabPath);
        if (existing >= 0) {
          return { ...s, activePath: tabPath, isVisible: true };
        }
        const newFile: OpenFile = {
          path: tabPath,
          name,
          content: '',
          originalContent: '',
          language: langFromPath(args.path),
          isDirty: false,
          openedAt: Date.now(),
          viewMode: 'source',
          isImage: false,
          diffArgs: args,
        };
        return { ...s, openFiles: [...s.openFiles, newFile], activePath: tabPath, isVisible: true };
      });
    },
  };
}

/**
 * Clamp a floating rect to the viewport, enforcing:
 * - min 320 × 240
 * - left edge ≥ SIDEBAR_TAB_W (don't cover the left icon strip)
 * - top edge ≥ APP_HEADER_HEIGHT（不允许遮挡顶部 workspace tab 区）
 * - at least 64 px of width/height always remains inside the viewport
 * (so the user can grab it back)
 */
export function clampRectToViewport(rect: FloatingRect): FloatingRect {
  if (typeof window === 'undefined') return rect;
  const vw = window.innerWidth;
  const vh = window.innerHeight;
  const w = Math.max(MIN_W, Math.min(rect.w, vw - SIDEBAR_TAB_W - 4));
  const h = Math.max(MIN_H, Math.min(rect.h, vh - APP_HEADER_HEIGHT - 4));
  const minX = SIDEBAR_TAB_W;
  const maxX = vw - 64; // keep ≥64 px grabbable
  const minY = APP_HEADER_HEIGHT;
  const maxY = vh - 32;
  let x = Math.max(minX, Math.min(rect.x, maxX));
  let y = Math.max(minY, Math.min(rect.y, maxY));
  // If the resulting box would overflow right/bottom, pull it back
  if (x + w > vw) x = Math.max(minX, vw - w);
  if (y + h > vh) y = Math.max(minY, vh - h);
  return { x, y, w, h };
}

export const fileEditorStore = createStore();
export const activeFile = derived(fileEditorStore, ($s) =>
  $s.openFiles.find((f) => f.path === $s.activePath) ?? null
);