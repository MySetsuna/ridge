import { EventEmitter } from 'node:events';
import { describe, expect, it, vi } from 'vitest';
import { main } from './start-vite-dev.mjs';

describe('start-vite-dev', () => {
  it('keeps process signal receiver when wiring child shutdown', async () => {
    const child = new EventEmitter();
    child.kill = vi.fn();
    const signalReceiver = vi.fn(function onSignal() {
      expect(this).toBe(process);
    });

    const exit = main({
      env: { RIDGE_DEV_SERVER_PORT: '4173' },
      spawnImpl: vi.fn(() => child),
      io: { log: vi.fn(), error: vi.fn() },
      onSignal: signalReceiver,
    });
    child.emit('exit', 0);

    await expect(exit).resolves.toBe(0);
    expect(signalReceiver).toHaveBeenCalledTimes(2);
  });
});
