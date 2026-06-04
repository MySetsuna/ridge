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

// ── Mocks installed before the dynamic import of the store ──────────────────

// `read_file_for_editor` is the only Tauri command openFile hits. We give the
// mock a controllable "in-flight" gate so the concurrency test can hold the
// first read open while a second openFile() starts — reproducing the TOCTOU
// window the fix closes.
const mockInvoke = vi.fn();
vi.mock('@tauri-apps/api/core', () => ({
  invoke: (...args: unknown[]) => mockInvoke(...args),
  isTauri: vi.fn(() => true),
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
  alertDialog: vi.fn(async () => undefined),
  choiceDialog: vi.fn(async () => 'cancel'),
  confirmDialog: vi.fn(async () => true),
}));

beforeEach(() => {
  mockInvoke.mockReset();
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

const { fileEditorStore } = await import('./fileEditor');

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
