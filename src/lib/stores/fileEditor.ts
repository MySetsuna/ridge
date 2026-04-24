// src/lib/stores/fileEditor.ts
//
// Global, per-window file editor: a drawer (default) or floating pin window that
// holds open code files. One store instance per window; all explorer/file-tree
// actions route through openFile(). Content is kept as a plain string — Monaco
// owns the text buffer inside the component, this store tracks metadata + dirty
// state + cross-tab coordination.

import { writable, get, derived } from 'svelte/store';
import { invoke, isTauri } from '@tauri-apps/api/core';
import { isMarkdownPath } from '$lib/utils/markdown';

export type EditorDisplayMode = 'drawer' | 'floating';

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
}

export interface FloatingRect {
  x: number;
  y: number;
  w: number;
  h: number;
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
}

const LS_KEY = 'wind-file-editor-prefs';
const MIN_W = 320;
const MIN_H = 240;
/**
 * Left sidebar icon strip width — floating editor is forbidden from overlapping
 * this zone (spec: "悬浮在所有页面的最上方，除了侧边条tab区域").
 */
export const SIDEBAR_TAB_W = 52;

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
  if (typeof window === 'undefined') return { x: 200, y: 100, w: 720, h: 540 };
  const w = Math.min(720, Math.max(MIN_W, window.innerWidth * 0.5));
  const h = Math.min(540, Math.max(MIN_H, window.innerHeight * 0.65));
  const x = Math.max(SIDEBAR_TAB_W + 8, Math.floor((window.innerWidth - w) / 2));
  const y = Math.max(40, Math.floor((window.innerHeight - h) / 2));
  return { x, y, w, h };
}

const prefs = loadPrefs();

const initial: FileEditorState = {
  openFiles: [],
  activePath: null,
  displayMode: (prefs.displayMode as EditorDisplayMode) ?? 'drawer',
  isVisible: false,
  drawerWidth: typeof prefs.drawerWidth === 'number' ? prefs.drawerWidth : 520,
  floatingRect: (prefs.floatingRect as FloatingRect) ?? defaultFloatingRect(),
};

function langFromPath(path: string): string {
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

function createStore() {
  const { subscribe, update, set } = writable<FileEditorState>(initial);

  function persist() {
    savePrefs(get({ subscribe }));
  }

  return {
    subscribe,

    /** Open a file (or activate its existing tab). Auto-shows the editor. */
    async openFile(path: string): Promise<void> {
      const state = get({ subscribe });
      const existing = state.openFiles.find((f) => f.path === path);
      if (existing) {
        update((s) => ({ ...s, activePath: path, isVisible: true }));
        return;
      }

      let content = '';
      let isBinary = false;
      if (isTauri()) {
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
          alert(`打开文件失败: ${e}`);
          return;
        }
      }
      if (isBinary) {
        alert('二进制文件暂不支持在编辑器中打开。');
        return;
      }

      const file: OpenFile = {
        path,
        name: basename(path),
        content,
        originalContent: content,
        language: langFromPath(path),
        isDirty: false,
        openedAt: Date.now(),
        // markdown 默认进 preview；其他语言没有 preview 概念，统一 source。
        viewMode: isMarkdownPath(path) ? 'preview' : 'source',
      };
      update((s) => ({
        ...s,
        openFiles: [...s.openFiles, file],
        activePath: path,
        isVisible: true,
      }));
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
        const ok = confirm(`"${file.name}" 有未保存的修改，确认关闭？`);
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
        const ok = confirm('存在未保存的修改，全部关闭？');
        if (!ok) return;
      }
      update((s) => ({ ...s, openFiles: [], activePath: null, isVisible: false }));
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
        update((s) => ({
          ...s,
          openFiles: s.openFiles.map((f) =>
            f.path === path ? { ...f, originalContent: f.content, isDirty: false } : f
          ),
        }));
      } catch (e) {
        alert(`保存失败: ${e}`);
      }
    },

    /** Revert active tab to disk contents, discarding edits. */
    async revertActive(): Promise<void> {
      const state = get({ subscribe });
      const file = state.openFiles.find((f) => f.path === state.activePath);
      if (!file) return;
      if (file.isDirty) {
        const ok = confirm('放弃所有未保存的修改？');
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
        alert(`重载失败: ${e}`);
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
  };
}

/**
 * Clamp a floating rect to the viewport, enforcing:
 * - min 320 × 240
 * - left edge ≥ SIDEBAR_TAB_W (don't cover the left icon strip)
 * - at least 64 px of width/height always remains inside the viewport
 *   (so the user can grab it back)
 */
export function clampRectToViewport(rect: FloatingRect): FloatingRect {
  if (typeof window === 'undefined') return rect;
  const vw = window.innerWidth;
  const vh = window.innerHeight;
  const w = Math.max(MIN_W, Math.min(rect.w, vw - SIDEBAR_TAB_W - 4));
  const h = Math.max(MIN_H, Math.min(rect.h, vh - 4));
  const minX = SIDEBAR_TAB_W;
  const maxX = vw - 64; // keep ≥64 px grabbable
  const minY = 0;
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
