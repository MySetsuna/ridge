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
});
