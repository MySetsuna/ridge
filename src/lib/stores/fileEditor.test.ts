/**
 * fileEditor.test.ts — regression locks for the file editor store's openFile
 * de-duplication contract.
 *
 * Round (2026-06-04): a CRITICAL `each_key_duplicate` crash was traced to a
 * TOCTOU race in `openFile`. The store does the "is this path already open?"
 * lookup, then `await`s an async `read_file_for_editor` disk read, then appends
 * a tab. A rapid double-click (or any two concurrent `openFile(path)` calls)
 * both pass the initial lookup while the first read is in flight, so both
 * append a tab with the SAME `path`. The editor tab strip keys its `{#each}` on
 * `path`, and Svelte throws `each_key_duplicate`, dropping/misrendering tabs.
 *
 * The fix re-checks `openFiles.some(f => f.path === path)` INSIDE the atomic
 * `update()` callback and activates the existing tab instead of appending a
 * second one. These tests lock that invariant at the source so the regression
 * cannot silently return.
 *
 * The store depends on `@tauri-apps/api/core.invoke` (the `read_file_for_editor`
 * Tauri command) and statically imports the `RidgeDialog.svelte` dialog
 * helpers; both are mocked so the suite runs in the node environment without a
 * Tauri backend or a Svelte compiler. Mirrors the mocking style in
 * `fileExplorer.test.ts`.
 */
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { get } from 'svelte/store';
import type { ChoiceResult, DialogOptions } from '$lib/components/RidgeDialog.svelte';

// ── Mocks installed before the dynamic import of the store ──────────────────

// `read_file_for_editor` is the only Tauri command openFile hits. We give the
// mock a controllable "in-flight" gate so the concurrency test can hold the
// first read open while a second openFile() starts — reproducing the TOCTOU
// window the fix closes.
const mockInvoke = vi.fn();
const mockIsTauri = vi.fn(() => true);
const mockAlertDialog = vi.fn<(opts: DialogOptions) => Promise<void>>(async () => undefined);
const mockChoiceDialog = vi.fn<(opts: DialogOptions & { secondaryLabel: string }) => Promise<ChoiceResult>>(
  async () => 'cancel',
);
const mockConfirmDialog = vi.fn<(opts: DialogOptions) => Promise<boolean>>(async () => true);
vi.mock('@tauri-apps/api/core', () => ({
  invoke: (...args: unknown[]) => mockInvoke(...args),
  isTauri: () => mockIsTauri(),
  // openFile only calls convertFileSrc for image paths; the tests below use
  // text paths, but the import must resolve.
  convertFileSrc: (p: string) => `asset://${p}`,
}));

// fsEvents transitively imports this; openFile never invokes the listener, but
// the import must resolve in the node env.
vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(async () => () => {}),
}));

// `$lib/utils/markdown` (pulled in for isMarkdownPath) statically imports
// monaco-editor, which dereferences `window` at module-eval time and explodes
// in the node environment. Stub monaco to a no-op; the store only needs
// isMarkdownPath, whose real (pure, path-only) implementation still runs.
vi.mock('monaco-editor', () => ({
  editor: { colorize: vi.fn(async () => '') },
}));

// The store statically imports dialog helpers from a `.svelte` file. vitest's
// node environment has no Svelte compiler, so stub the module. The happy-path
// tests never trigger a dialog (no read failures, no dirty conflicts); the
// stubs simply keep the import graph resolvable.
vi.mock('$lib/components/RidgeDialog.svelte', () => ({
  alertDialog: (opts: DialogOptions) => mockAlertDialog(opts),
  choiceDialog: (opts: DialogOptions & { secondaryLabel: string }) => mockChoiceDialog(opts),
  confirmDialog: (opts: DialogOptions) => mockConfirmDialog(opts),
}));

beforeEach(() => {
  mockInvoke.mockReset();
  mockIsTauri.mockReset();
  mockIsTauri.mockReturnValue(true);
  mockAlertDialog.mockReset();
  mockChoiceDialog.mockReset();
  mockChoiceDialog.mockResolvedValue('cancel');
  mockConfirmDialog.mockReset();
  mockConfirmDialog.mockResolvedValue(true);
  // localStorage shim — the store reads/writes prefs on construction + persist.
  const store: Record<string, string> = {};
  (globalThis as unknown as { localStorage: Storage }).localStorage = {
    getItem: (k: string) => (k in store ? store[k] : null),
    setItem: (k: string, v: string) => {
      store[k] = v;
    },
    removeItem: (k: string) => {
      delete store[k];
    },
    clear: () => {
      for (const k of Object.keys(store)) delete store[k];
    },
    key: (i: number) => Object.keys(store)[i] ?? null,
    get length() {
      return Object.keys(store).length;
    },
  };
});

const { fileEditorStore, langFromPath, clampRectToViewport } = await import('./fileEditor');

/** Drain all currently-open tabs so each test starts from a known-empty store. */
async function resetEditor(): Promise<void> {
  await fileEditorStore.closeAll();
}

/** Count how many open tabs carry the given path. The invariant is "exactly 1". */
function tabCountFor(path: string): number {
  return get(fileEditorStore).openFiles.filter((f) => f.path === path).length;
}

describe('fileEditorStore.openFile — duplicate-tab guard (each_key_duplicate regression)', () => {
  beforeEach(async () => {
    await resetEditor();
  });

  it('opening the same path twice sequentially keeps exactly one tab', async () => {
    // Arrange — read_file_for_editor returns deterministic text content.
    mockInvoke.mockResolvedValue({
      content: 'export const a = 1;\n',
      is_binary: false,
      size: 20,
    });

    // Act — open, await, then open the same path again.
    await fileEditorStore.openFile('/proj/src/a.ts');
    await fileEditorStore.openFile('/proj/src/a.ts');

    // Assert — one tab, and it is the active one.
    const state = get(fileEditorStore);
    expect(tabCountFor('/proj/src/a.ts')).toBe(1);
    expect(state.openFiles).toHaveLength(1);
    expect(state.activePath).toBe('/proj/src/a.ts');
  });

  it('two concurrent openFile(path) calls (rapid double-click TOCTOU) create exactly one tab and do not duplicate', async () => {
    // Arrange — hold the first disk read in flight so BOTH openFile calls pass
    // the pre-read "already open?" lookup before either appends a tab. This is
    // the exact race that produced `each_key_duplicate`.
    let releaseFirstRead!: () => void;
    const firstReadGate = new Promise<void>((resolve) => {
      releaseFirstRead = resolve;
    });
    let callCount = 0;
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd !== 'read_file_for_editor') throw new Error(`unexpected invoke ${cmd}`);
      callCount += 1;
      if (callCount === 1) await firstReadGate; // first read blocks until released
      return { content: 'fn main() {}\n', is_binary: false, size: 12 };
    });

    // Act — fire both opens WITHOUT awaiting, so the second enters while the
    // first is still awaiting its read.
    const first = fileEditorStore.openFile('/proj/src/main.rs');
    const second = fileEditorStore.openFile('/proj/src/main.rs');
    // Let the second call run far enough to begin (and possibly finish) its
    // own read before the first read resolves.
    await Promise.resolve();
    releaseFirstRead();
    await Promise.all([first, second]);

    // Assert — the atomic re-check inside update() collapsed the second append
    // into an activate, so there is exactly one tab for the path.
    const state = get(fileEditorStore);
    expect(tabCountFor('/proj/src/main.rs')).toBe(1);
    expect(state.openFiles).toHaveLength(1);
    expect(state.activePath).toBe('/proj/src/main.rs');
  });

  it('all open tab paths remain unique after a burst of concurrent opens of the same path', async () => {
    // Arrange — five simultaneous opens of one path. Even gated reads must not
    // produce two entries with the same key, or the keyed {#each} throws.
    mockInvoke.mockResolvedValue({
      content: '{"k":1}\n',
      is_binary: false,
      size: 8,
    });

    // Act — fire five concurrent opens of the SAME path.
    await Promise.all(
      Array.from({ length: 5 }, () => fileEditorStore.openFile('/proj/data.json')),
    );

    // Assert — the keyed-each invariant: no duplicate paths in openFiles.
    const paths = get(fileEditorStore).openFiles.map((f) => f.path);
    const unique = new Set(paths);
    expect(paths).toHaveLength(unique.size); // no key collisions
    expect(tabCountFor('/proj/data.json')).toBe(1);
  });

  it('re-opening an already-open tab activates it without reading or appending a duplicate', async () => {
    // Arrange — open once, then switch the active tab away by opening a second
    // distinct file.
    mockInvoke.mockResolvedValue({ content: 'a\n', is_binary: false, size: 2 });
    await fileEditorStore.openFile('/proj/one.ts');
    await fileEditorStore.openFile('/proj/two.ts');
    expect(get(fileEditorStore).activePath).toBe('/proj/two.ts');

    // Act — re-open the first file. It is already open and clean, so the store
    // re-reads it from disk (focus refresh) and re-activates it — but must not
    // append a second tab.
    await fileEditorStore.openFile('/proj/one.ts');

    // Assert — still two tabs total, first is active, no duplicate.
    const state = get(fileEditorStore);
    expect(state.openFiles).toHaveLength(2);
    expect(tabCountFor('/proj/one.ts')).toBe(1);
    expect(state.activePath).toBe('/proj/one.ts');
  });
});

describe('fileEditorStore.openFile — same-basename distinct-path tab keys', () => {
  beforeEach(async () => {
    await resetEditor();
  });

  it('opening two different files that share a basename keeps two distinct tab keys', async () => {
    // Arrange — two files named index.ts in different directories. The tab key
    // is the full path, so these must coexist as two tabs (the display name
    // collides, the key must not).
    mockInvoke.mockResolvedValue({
      content: 'export {};\n',
      is_binary: false,
      size: 11,
    });

    // Act — open both distinct paths.
    await fileEditorStore.openFile('/proj/a/index.ts');
    await fileEditorStore.openFile('/proj/b/index.ts');

    // Assert — two tabs, two unique path keys, identical display names.
    const state = get(fileEditorStore);
    expect(state.openFiles).toHaveLength(2);
    const paths = state.openFiles.map((f) => f.path);
    expect(new Set(paths).size).toBe(2); // distinct keys → no each_key_duplicate
    const names = state.openFiles.map((f) => f.name);
    expect(names).toEqual(['index.ts', 'index.ts']); // same basename, by design
    expect(state.activePath).toBe('/proj/b/index.ts');
  });
});

describe('fileEditorStore public state and lifecycle APIs', () => {
  beforeEach(async () => {
    await resetEditor();
    mockInvoke.mockResolvedValue({ content: 'initial\n', is_binary: false, size: 8 });
  });

  it('maps common languages, clamps floating bounds, and persists display preferences', () => {
    expect(langFromPath('Dockerfile')).toBe('dockerfile');
    expect(langFromPath('src/main.tsx')).toBe('typescript');
    expect(langFromPath('notes.unknown')).toBe('plaintext');

    vi.stubGlobal('window', { innerWidth: 1000, innerHeight: 700 });
    expect(clampRectToViewport({ x: -20, y: -20, w: 10, h: 10 })).toEqual({
      x: 52, y: 44, w: 320, h: 240,
    });
    expect(clampRectToViewport({ x: 9999, y: 9999, w: 900, h: 800 })).toEqual({
      x: 100, y: 48, w: 900, h: 652,
    });

    fileEditorStore.setDisplayMode('floating');
    fileEditorStore.setDrawerWidth(100);
    fileEditorStore.setFloatingRect({ x: 0, y: 0, w: 400, h: 300 });
    fileEditorStore.toggleVisibility();
    const state = get(fileEditorStore);
    expect(state.displayMode).toBe('floating');
    expect(state.drawerWidth).toBe(280);
    expect(state.floatingRect).toEqual({ x: 52, y: 44, w: 400, h: 300 });
    expect(state.isVisible).toBe(true);
  });

  it('supports snapshots, handoff, ordering, reveal, search, and interceptors', async () => {
    await fileEditorStore.openFile('/p/a.ts', { line: 3, column: 0, matchLength: 2 });
    await fileEditorStore.openFile('/p/b.md');
    await fileEditorStore.openFile('/p/c.rs');
    fileEditorStore.updateContent('/p/a.ts', 'changed');
    fileEditorStore.setViewMode('/p/a.ts', 'preview');
    fileEditorStore.setSearchHits([{ path: '/p/a.ts', line: 3, column: 1, matchLength: 2 }]);
    expect(fileEditorStore.consumePendingReveal('/p/b.md')).toBeNull();
    expect(fileEditorStore.consumePendingReveal('/p/a.ts')).toMatchObject({ line: 3, column: 1 });
    fileEditorStore.reorder(2, 0);
    fileEditorStore.setOrder(['/p/b.md', '/p/a.ts', '/p/c.rs']);
    expect(get(fileEditorStore).openFiles.map((f) => f.path)).toEqual(['/p/b.md', '/p/a.ts', '/p/c.rs']);
    fileEditorStore.setActive('/p/a.ts');
    expect(fileEditorStore.snapshot().active).toBe('/p/a.ts');
    fileEditorStore.clearSearchHits();
    expect(get(fileEditorStore).searchHits).toEqual([]);

    const snapshot = fileEditorStore.snapshot();
    fileEditorStore.clearForHandoff();
    expect(get(fileEditorStore).openFiles).toHaveLength(0);
    fileEditorStore.loadFiles(snapshot.files, snapshot.active);
    expect(get(fileEditorStore).openFiles.map((f) => f.path)).toEqual(['/p/b.md', '/p/a.ts', '/p/c.rs']);

    const interceptor = vi.fn(() => true);
    fileEditorStore.setOpenInterceptor(interceptor);
    await fileEditorStore.openFile('/p/intercepted.ts');
    fileEditorStore.openDiffTab({ repoRoot: 'C:\\repo', path: 'src/a.ts', cached: true });
    expect(interceptor).toHaveBeenCalledTimes(2);
    expect(get(fileEditorStore).openFiles.some((f) => f.path.includes('intercepted'))).toBe(false);
    fileEditorStore.setOpenInterceptor(null);
  });

  it('closes tabs with dirty confirmation, preserves active selection, and closes saved tabs', async () => {
    await fileEditorStore.openFile('/p/a.ts');
    await fileEditorStore.openFile('/p/b.ts');
    await fileEditorStore.openFile('/p/c.ts');
    fileEditorStore.updateContent('/p/a.ts', 'dirty-a');
    fileEditorStore.updateContent('/p/b.ts', 'dirty-b');
    fileEditorStore.setActive('/p/b.ts');

    mockConfirmDialog.mockResolvedValueOnce(false);
    expect(await fileEditorStore.closeFile('/p/b.ts')).toBe(false);
    expect(get(fileEditorStore).openFiles).toHaveLength(3);
    mockConfirmDialog.mockResolvedValueOnce(true);
    expect(await fileEditorStore.closeFile('/p/b.ts')).toBe(true);
    expect(get(fileEditorStore).activePath).toBe('/p/c.ts');

    mockConfirmDialog.mockResolvedValueOnce(false);
    await fileEditorStore.closeOthers('/p/c.ts');
    expect(get(fileEditorStore).openFiles.map((f) => f.path)).toEqual(['/p/a.ts', '/p/c.ts']);
    mockConfirmDialog.mockResolvedValueOnce(true);
    await fileEditorStore.closeOthers('/p/c.ts');
    expect(get(fileEditorStore).openFiles.map((f) => f.path)).toEqual(['/p/c.ts']);

    await fileEditorStore.openFile('/p/d.ts');
    await fileEditorStore.openFile('/p/e.ts');
    fileEditorStore.updateContent('/p/d.ts', 'dirty-d');
    fileEditorStore.setActive('/p/e.ts');
    mockConfirmDialog.mockResolvedValueOnce(true);
    await fileEditorStore.closeToRight('/p/d.ts');
    expect(get(fileEditorStore).openFiles.map((f) => f.path)).toEqual(['/p/c.ts', '/p/d.ts']);
    fileEditorStore.closeSaved();
    expect(get(fileEditorStore).openFiles.map((f) => f.path)).toEqual(['/p/d.ts']);
  });

  it('opens image, markdown, binary, and diff tabs through their distinct paths', async () => {
    await fileEditorStore.openFile('C:\\work\\photo.PNG');
    expect(get(fileEditorStore).openFiles[0]).toMatchObject({
      language: 'image', imageUrl: 'asset://C:/work/photo.PNG', imageVersion: 0,
    });
    await fileEditorStore.openFile('/p/readme.md');
    expect(get(fileEditorStore).openFiles.find((f) => f.path === '/p/readme.md')?.viewMode).toBe('preview');

    mockInvoke.mockResolvedValueOnce({ content: '', is_binary: true, size: 4 });
    await fileEditorStore.openFile('/p/archive.bin');
    expect(mockAlertDialog).toHaveBeenCalledWith(expect.objectContaining({ title: '无法打开' }));
    expect(get(fileEditorStore).openFiles.some((f) => f.path.endsWith('archive.bin'))).toBe(false);

    fileEditorStore.openDiffTab({ repoRoot: 'C:\\repo', path: 'src/a.ts', cached: true });
    fileEditorStore.openDiffTab({ repoRoot: 'C:\\repo', path: 'src/a.ts', cached: false, commit: 'abcdef123456' });
    fileEditorStore.openDiffTab({ repoRoot: 'C:\\repo', path: 'src/a.ts', cached: false, commit: 'abcdef123456', compareBase: '123456789' });
    const diffs = get(fileEditorStore).openFiles.filter((f) => f.diffArgs);
    expect(diffs).toHaveLength(3);
    fileEditorStore.openDiffTab({ repoRoot: 'C:\\repo', path: 'src/a.ts', cached: true });
    expect(get(fileEditorStore).openFiles.filter((f) => f.diffArgs)).toHaveLength(3);
  });

  it('saves, reverts, and handles external clean, dirty, deleted, image, and diff changes', async () => {
    await fileEditorStore.openFile('/p/a.ts');
    fileEditorStore.updateContent('/p/a.ts', 'local');
    mockInvoke.mockClear();
    await fileEditorStore.saveFile('/p/a.ts');
    expect(mockInvoke).toHaveBeenCalledWith('write_file', { path: '/p/a.ts', content: 'local' });
    expect(get(fileEditorStore).openFiles[0].isDirty).toBe(false);

    await fileEditorStore.openFile('/p/external.ts');
    mockInvoke.mockResolvedValueOnce({ content: 'disk', is_binary: false, size: 4 });
    await fileEditorStore.handleExternalChange('/p/external.ts');
    expect(get(fileEditorStore).openFiles.find((f) => f.path === '/p/external.ts')?.content).toBe('disk');
    fileEditorStore.updateContent('/p/external.ts', 'local-again');
    mockInvoke.mockResolvedValueOnce({ content: 'new-disk', is_binary: false, size: 8 });
    mockChoiceDialog.mockResolvedValueOnce('primary');
    await fileEditorStore.handleExternalChange('/p/external.ts');
    expect(get(fileEditorStore).openFiles.find((f) => f.path === '/p/external.ts')).toMatchObject({ content: 'new-disk', originalContent: 'new-disk', isDirty: false });

    fileEditorStore.updateContent('/p/external.ts', 'keep-edit');
    mockInvoke.mockResolvedValueOnce({ content: 'other-disk', is_binary: false, size: 10 });
    mockChoiceDialog.mockResolvedValueOnce('secondary');
    await fileEditorStore.handleExternalChange('/p/external.ts');
    expect(get(fileEditorStore).openFiles.find((f) => f.path === '/p/external.ts')).toMatchObject({ content: 'keep-edit', originalContent: 'other-disk', isDirty: true });

    mockInvoke.mockRejectedValueOnce(new Error('gone'));
    await fileEditorStore.handleExternalChange('/p/external.ts');
    expect(get(fileEditorStore).openFiles.find((f) => f.path === '/p/external.ts')?.external).toBe('deleted');

    await fileEditorStore.openFile('/p/photo.png');
    await fileEditorStore.handleExternalChange('/p/photo.png');
    expect(get(fileEditorStore).openFiles.find((f) => f.path === '/p/photo.png')?.imageVersion).toBe(1);
    fileEditorStore.openDiffTab({ repoRoot: '/repo', path: 'a.ts', cached: false });
    const callsBeforeDiffChange = mockInvoke.mock.calls.length;
    await fileEditorStore.handleExternalChange('__diff__:working:/repo:a.ts');
    expect(mockInvoke).toHaveBeenCalledTimes(callsBeforeDiffChange);
  });

  it('fails closed across reload, binary, external, save, and revert error paths', async () => {
    await fileEditorStore.openFile('/p/reload.ts');
    mockInvoke.mockResolvedValueOnce({ content: 'from-disk', is_binary: false, size: 9 });
    await fileEditorStore.openFile('/p/reload.ts');
    expect(get(fileEditorStore).openFiles[0]).toMatchObject({
      content: 'from-disk', originalContent: 'from-disk', isDirty: false,
    });

    mockInvoke.mockResolvedValueOnce({ content: '', is_binary: true, size: 4 });
    await fileEditorStore.openFile('/p/reload.ts');
    expect(get(fileEditorStore).openFiles[0].content).toBe('from-disk');

    mockInvoke.mockRejectedValueOnce(new Error('read denied'));
    await fileEditorStore.openFile('/p/reload.ts');
    expect(get(fileEditorStore).activePath).toBe('/p/reload.ts');

    mockInvoke.mockRejectedValueOnce(new Error('open denied'));
    await fileEditorStore.openFile('/p/fail.ts');
    expect(mockAlertDialog).toHaveBeenCalledWith(expect.objectContaining({ title: '打开文件失败' }));

    await fileEditorStore.openFile('/p/external-bin.ts');
    mockInvoke.mockResolvedValueOnce({ content: '', is_binary: true, size: 1 });
    await fileEditorStore.handleExternalChange('/p/external-bin.ts');
    expect(get(fileEditorStore).openFiles.find((f) => f.path === '/p/external-bin.ts')?.external).toBeUndefined();

    fileEditorStore.updateContent('/p/external-bin.ts', 'dirty');
    mockInvoke.mockRejectedValueOnce(new Error('gone again'));
    await fileEditorStore.revertActive();
    expect(mockAlertDialog).toHaveBeenCalledWith(expect.objectContaining({ title: '重载失败' }));

    fileEditorStore.setActive('/p/external-bin.ts');
    fileEditorStore.updateContent('/p/external-bin.ts', 'dirty-save');
    mockInvoke.mockRejectedValueOnce(new Error('write denied'));
    await fileEditorStore.saveFile('/p/external-bin.ts');
    expect(mockAlertDialog).toHaveBeenCalledWith(expect.objectContaining({ title: '保存失败' }));
  });

  it('covers browser image URLs and no-op/invalid lifecycle requests', async () => {
    mockIsTauri.mockReturnValue(false);
    await fileEditorStore.openFile('C:\\work\\preview.PNG');
    expect(get(fileEditorStore).openFiles[0]).toMatchObject({
      isImage: true, imageUrl: 'file:///C:/work/preview.PNG',
    });
    fileEditorStore.show();
    fileEditorStore.hide();
    fileEditorStore.setActive('/missing');
    fileEditorStore.reorder(-1, 0);
    fileEditorStore.setOrder(['/missing']);
    await fileEditorStore.closeFile('/missing');
    await fileEditorStore.closeOthers('C:\\work\\preview.PNG');
    await fileEditorStore.closeToRight('C:\\work\\preview.PNG');
    fileEditorStore.closeSaved();
    expect(get(fileEditorStore).openFiles).toHaveLength(0);

    await fileEditorStore.openFile('C:\\work\\preview.PNG');
    fileEditorStore.updateContent('C:\\work\\preview.PNG', 'changed');
    mockConfirmDialog.mockResolvedValueOnce(false);
    await fileEditorStore.closeAll();
    expect(get(fileEditorStore).openFiles).toHaveLength(1);
  });

  it('uses local browser save/revert paths without Tauri commands', async () => {
    mockIsTauri.mockReturnValue(false);
    await fileEditorStore.openFile('/p/browser.ts');
    fileEditorStore.updateContent('/p/browser.ts', 'browser-edit');
    await fileEditorStore.saveFile('/p/browser.ts');
    expect(mockInvoke).not.toHaveBeenCalled();
    expect(get(fileEditorStore).openFiles[0]).toMatchObject({ originalContent: 'browser-edit', isDirty: false });
    fileEditorStore.updateContent('/p/browser.ts', 'again');
    await fileEditorStore.revertActive();
    expect(get(fileEditorStore).openFiles[0]).toMatchObject({ content: 'browser-edit', isDirty: false });
    await fileEditorStore.openFile('relative.txt');
    expect(get(fileEditorStore).openFiles.find((f) => f.path === 'relative.txt')?.content).toBe('');
  });
});
