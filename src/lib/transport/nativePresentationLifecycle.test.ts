import { beforeEach, describe, expect, it, vi } from 'vitest';
const { listen, manager } = vi.hoisted(() => ({
  listen: vi.fn(),
  manager: { reclaimTerminalMemory: vi.fn(), restoreTerminalMemory: vi.fn() },
}));
vi.mock('@tauri-apps/api/event', () => ({ listen }));
vi.mock('@ridge/remote/shared/terminal/manager', () => ({
  TerminalManager: { tryInstance: () => manager },
}));
import { startNativePresentationLifecycle } from './nativePresentationLifecycle';

describe('native presentation visibility', () => {
  beforeEach(() => { vi.resetAllMocks(); });
  it('reclaims and restores from OS events, then releases both subscriptions', async () => {
    const handlers = new Map<string, () => void>();
    const stops = [vi.fn(), vi.fn()];
    listen.mockImplementation((event, handler) => {
      handlers.set(event, handler);
      return Promise.resolve(stops[handlers.size - 1]);
    });
    const stop = startNativePresentationLifecycle();
    await Promise.resolve();
    handlers.get('ridge://memory-reclaim')!();
    handlers.get('ridge://memory-restore')!();
    expect(manager.reclaimTerminalMemory).toHaveBeenCalledWith({ documentHidden: true, forceHeapPressure: true });
    expect(manager.restoreTerminalMemory).toHaveBeenCalledOnce();
    stop(); stop();
    stops.forEach(unlisten => expect(unlisten).toHaveBeenCalledOnce());
    handlers.get('ridge://memory-restore')!();
    expect(manager.restoreTerminalMemory).toHaveBeenCalledOnce();
  });
  it('cleans late registration even when the sibling registration fails', async () => {
    let resolve!: (stop: () => void) => void;
    listen.mockReturnValueOnce(new Promise<() => void>(done => { resolve = done; }))
      .mockRejectedValueOnce(new Error('closing'));
    const stop = startNativePresentationLifecycle();
    stop();
    const unlisten = vi.fn(); resolve(unlisten);
    await Promise.resolve(); await Promise.resolve();
    expect(unlisten).toHaveBeenCalledOnce();
  });
});
