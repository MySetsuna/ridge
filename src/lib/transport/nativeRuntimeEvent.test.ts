import { beforeEach, describe, expect, it, vi } from 'vitest';
const { os, listeners } = vi.hoisted(() => ({
  os: { listen: vi.fn(), once: vi.fn(), emit: vi.fn(), emitTo: vi.fn() },
  listeners: new Map<string, (event: unknown) => void>(),
}));
vi.mock('@ridge/native-os-event', () => ({ ...os, TauriEvent: {} }));
vi.mock('./tauriShim/bridge', () => ({ bridge: { listen: (name: string, handler: (event: unknown) => void) => {
  listeners.set(name, handler); return () => listeners.delete(name);
} } }));
import { listen, once, emit, emitTo } from './nativeRuntimeEvent';

describe('native event boundary', () => {
  beforeEach(() => { listeners.clear(); vi.clearAllMocks(); });
  it('delivers business events from Runtime and removes listeners', async () => {
    const handler = vi.fn();
    const stop = await listen('workspace-renamed', handler);
    const event = { event: 'workspace-renamed', id: 1, payload: { name: 'renamed' } };
    listeners.get('workspace-renamed')!(event);
    expect(handler).toHaveBeenCalledWith(event);
    expect(os.listen).not.toHaveBeenCalled();
    stop(); expect(listeners.size).toBe(0);
  });
  it('removes a once listener before invoking a reentrant business handler', async () => {
    const handler = vi.fn(() => { expect(listeners.has('theme-changed')).toBe(false); });
    await once('theme-changed', handler);
    listeners.get('theme-changed')!({ payload: 'theme' });
    expect(handler).toHaveBeenCalledOnce();
  });
  it('rejects manufactured domain events and keeps window events native', async () => {
    await expect(emit('workspace-renamed')).rejects.toThrow('Domain events originate in Runtime');
    await expect(emitTo('other-window', 'theme-changed')).rejects.toThrow('Domain events originate in Runtime');
    expect(os.emit).not.toHaveBeenCalled(); expect(os.emitTo).not.toHaveBeenCalled();
    const handler = vi.fn(); await listen('tauri://focus', handler);
    expect(os.listen).toHaveBeenCalledWith('tauri://focus', handler, undefined);
  });
  it.each(['ridge://memory-reclaim', 'ridge://memory-restore', 'ridge://auth-focus', 'editor-window-open-file', 'editor-window-closed'])('routes actual host event %s through OS IPC', async event => {
    const handler = vi.fn();
    await listen(event, handler);
    await once(event, handler);
    expect(os.listen).toHaveBeenCalledWith(event, handler, undefined);
    expect(os.once).toHaveBeenCalledWith(event, handler, undefined);
    expect(listeners.has(event)).toBe(false);
  });
  it('hands unsaved editor presentation state between local windows without a domain event', async () => {
    const payload = { files: [{ path: '/project/file.ts', content: 'unsaved text' }], active: '/project/file.ts' };
    await emitTo('editor', 'editor-window-open-file', { kind: 'file', path: '/project/file.ts' });
    await emitTo('main', 'editor-window-closed', payload);
    expect(os.emitTo).toHaveBeenCalledWith('main', 'editor-window-closed', payload);
    expect(listeners.size).toBe(0);
    await expect(emit('commune-members-changed', payload)).rejects.toThrow('Domain events originate in Runtime');
  });
});
