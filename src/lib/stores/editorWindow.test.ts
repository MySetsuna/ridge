import { beforeEach, describe, expect, it, vi } from 'vitest';
import { get } from 'svelte/store';

const tauri = vi.hoisted(() => ({ isTauri: vi.fn(() => false) }));
const eventApi = vi.hoisted(() => ({
  emitTo: vi.fn(async () => {}),
  listen: vi.fn(),
}));
const store = vi.hoisted(() => ({
  setDisplayMode: vi.fn(),
  setOpenInterceptor: vi.fn(),
  snapshot: vi.fn(() => ({ files: [], active: null })),
  clearForHandoff: vi.fn(),
  loadFiles: vi.fn(),
}));
const windows = vi.hoisted(() => ({
  getByLabel: vi.fn(),
  instances: [] as any[],
}));

async function loadEditorWindow() {
  vi.resetModules();
  return import('./editorWindow');
}

vi.mock('@tauri-apps/api/core', () => ({ isTauri: tauri.isTauri }));
vi.mock('@tauri-apps/api/event', () => eventApi);
vi.mock('@tauri-apps/api/webviewWindow', () => ({
  WebviewWindow: class WebviewWindowMock {
    callbacks = new Map<string, (event?: any) => void>();
    once = vi.fn((type: string, callback: (event?: any) => void) => this.callbacks.set(type, callback));
    constructor(public label: string, public options: any) { windows.instances.push(this); }
    static getByLabel = windows.getByLabel;
  },
}));
vi.mock('./fileEditor', () => ({
  fileEditorStore: store,
}));

describe('independent editor window handoff', () => {
  beforeEach(() => {
    tauri.isTauri.mockReset().mockReturnValue(false);
    eventApi.emitTo.mockClear();
    eventApi.listen.mockReset();
    store.setDisplayMode.mockClear();
    store.setOpenInterceptor.mockClear();
    store.snapshot.mockReset().mockReturnValue({ files: [], active: null });
    store.clearForHandoff.mockClear();
    store.loadFiles.mockClear();
    windows.getByLabel.mockReset();
    windows.instances.length = 0;
    vi.stubGlobal('localStorage', { setItem: vi.fn() });
  });

  it('falls back to floating mode outside Tauri and rejects empty snapshots', async () => {
    const module = await loadEditorWindow();
    await module.popOutEditor();
    expect(store.setDisplayMode).toHaveBeenCalledWith('floating');

    tauri.isTauri.mockReturnValue(true);
    windows.getByLabel.mockResolvedValue(null);
    await module.popOutEditor();
    expect(store.snapshot).toHaveBeenCalled();
    expect(windows.instances).toHaveLength(0);
    tauri.isTauri.mockReturnValue(false);
    expect(await module.initEditorWindowHost()).toBeNull();
  });

  it('focuses an existing window and transfers a non-empty snapshot on create', async () => {
    const module = await loadEditorWindow();
    tauri.isTauri.mockReturnValue(true);
    const existing = { setFocus: vi.fn(async () => {}) };
    windows.getByLabel.mockResolvedValueOnce(existing);
    await module.popOutEditor();
    expect(existing.setFocus).toHaveBeenCalled();

    windows.getByLabel.mockResolvedValue(null);
    store.snapshot.mockReturnValue({ files: [{ path: '/repo/a.ts', content: 'x' }], active: '/repo/a.ts' } as any);
    await module.popOutEditor();
    expect(windows.instances).toHaveLength(1);
    const win = windows.instances[0];
    win.callbacks.get('tauri://created')?.();
    expect(store.setOpenInterceptor).toHaveBeenCalledWith(expect.any(Function));
    expect(store.clearForHandoff).toHaveBeenCalled();
    expect(get(module.editorPoppedOut)).toBe(true);

    const interceptor = store.setOpenInterceptor.mock.calls.at(-1)?.[0] as (request: any) => boolean;
    expect(interceptor({ path: '/repo/b.ts' })).toBe(true);
    expect(eventApi.emitTo).toHaveBeenCalledWith('editor', 'editor-window-open-file', { path: '/repo/b.ts' });
  });

  it('restores files when the editor window closes', async () => {
    const module = await loadEditorWindow();
    tauri.isTauri.mockReturnValue(true);
    const unlisten = vi.fn();
    eventApi.listen.mockResolvedValue(unlisten);
    const result = await module.initEditorWindowHost();
    expect(result).toBe(unlisten);
    const handler = eventApi.listen.mock.calls[0][1];
    handler({ payload: { files: [{ path: '/repo/a.ts', content: 'x' }], active: '/repo/a.ts' } });
    expect(store.loadFiles).toHaveBeenCalledWith(
      [{ path: '/repo/a.ts', content: 'x' }],
      '/repo/a.ts',
    );
    expect(store.setOpenInterceptor).toHaveBeenLastCalledWith(null);
    expect(get(module.editorPoppedOut)).toBe(false);
  });
});
