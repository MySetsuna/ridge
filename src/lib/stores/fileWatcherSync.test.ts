import { describe, expect, it, vi, beforeEach, afterEach } from 'vitest';

const h = vi.hoisted(() => {
  const store = <T>(initial: T) => {
    let value = initial;
    const listeners = new Set<(next: T) => void>();
    return {
      subscribe(fn: (next: T) => void) { listeners.add(fn); fn(value); return () => listeners.delete(fn); },
      set(next: T) { value = next; for (const fn of listeners) fn(value); },
    };
  };
  return {
    explorer: store({ columns: [] as Array<{ cwd: string }> }),
    editor: store({ openFiles: [] as Array<{ path: string; diffArgs?: unknown }> }),
    invoke: vi.fn(),
    refresh: vi.fn(async () => undefined),
    prune: vi.fn(async () => undefined),
    onFsChange: vi.fn(),
    fsHandler: null as ((payload: { root: string; paths: string[]; coalesced: boolean }) => void) | null,
    recentlyWritten: vi.fn((_path: string) => false),
    handleExternalChange: vi.fn(async () => undefined),
  };
});

vi.mock('@tauri-apps/api/core', () => ({ invoke: h.invoke, isTauri: () => true }));
vi.mock('./fileExplorer', () => ({
  fileExplorerStore: h.explorer,
  refreshColumnsCovering: h.refresh,
  pruneStaleExpandedPaths: h.prune,
}));
vi.mock('./fileEditor', () => ({ fileEditorStore: { ...h.editor, handleExternalChange: h.handleExternalChange } }));
vi.mock('./fsEvents', () => ({
  onFsChange: (fn: (payload: { root: string; paths: string[]; coalesced: boolean }) => void) => {
    h.fsHandler = fn;
    h.onFsChange(fn);
    return () => { if (h.fsHandler === fn) h.fsHandler = null; };
  },
  isRecentlyWritten: h.recentlyWritten,
}));

const watcher = await import('./fileWatcherSync');

describe('fileWatcherSync', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    h.explorer.set({ columns: [] });
    h.editor.set({ openFiles: [] });
    h.invoke.mockReset();
    h.invoke.mockResolvedValue(undefined);
    h.refresh.mockClear();
    h.prune.mockClear();
    h.onFsChange.mockClear();
    h.recentlyWritten.mockReset();
    h.recentlyWritten.mockReturnValue(false);
    h.handleExternalChange.mockClear();
  });

  afterEach(() => vi.useRealTimers());

  it('coalesces watcher roots, deduplicates covered files, and avoids redundant IPC', async () => {
    watcher.initFileWatcherSync();
    watcher.initFileWatcherSync();
    h.explorer.set({ columns: [{ cwd: 'C:\\repo' }, { cwd: 'C:/repo' }] });
    h.editor.set({ openFiles: [
      { path: 'C:\\repo\\src\\a.ts' },
      { path: 'C:\\outside\\b.ts' },
      { path: '__diff__:working:/repo:a.ts', diffArgs: {} },
    ] });
    await vi.advanceTimersByTimeAsync(100);
    expect(h.invoke).toHaveBeenCalledWith('start_watching_paths', {
      roots: [
        { path: 'C:/outside/b.ts', recursive: false },
        { path: 'C:/repo', recursive: true },
      ],
    });
    h.explorer.set({ columns: [{ cwd: 'C:/repo' }] });
    await vi.advanceTimersByTimeAsync(100);
    expect(h.invoke).toHaveBeenCalledTimes(1);
  });

  it('fans out coalesced and file-level fs changes after root/global debounce', async () => {
    watcher.initFileWatcherSync();
    h.explorer.set({ columns: [{ cwd: '/repo' }] });
    h.editor.set({ openFiles: [{ path: '/outside/a.ts' }] });
    await vi.advanceTimersByTimeAsync(100);
    h.recentlyWritten.mockImplementation((path: string) => path === '/outside/written.ts');
    h.fsHandler?.({ root: '/repo', paths: ['/repo/src/a.ts', '/outside/written.ts', '/outside/live.ts'], coalesced: false });
    h.fsHandler?.({ root: '/repo', paths: [], coalesced: true });
    await vi.advanceTimersByTimeAsync(301);
    await vi.advanceTimersByTimeAsync(1);
    expect(h.refresh).toHaveBeenCalledWith('/repo');
    expect(h.refresh).toHaveBeenCalledWith('/repo/src');
    expect(h.handleExternalChange).toHaveBeenCalledWith('/repo/src/a.ts');
    expect(h.handleExternalChange).toHaveBeenCalledWith('/outside/live.ts');
    expect(h.handleExternalChange).not.toHaveBeenCalledWith('/outside/written.ts');
  });
});
