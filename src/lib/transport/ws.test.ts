import { describe, expect, it, vi } from 'vitest';
import type { RemoteConnection } from '@ridge/remote';
import { WsDataProvider } from './ws';

function fakeConnection() {
  let onMessage: ((message: unknown) => void) | undefined;
  let onState: ((state: 'connecting' | 'connected' | 'disconnected' | 'error') => void) | undefined;
  const connection = {
    onMessage: vi.fn((fn: (message: unknown) => void) => {
      onMessage = fn;
      return () => { onMessage = undefined; };
    }),
    onStateChange: vi.fn((fn: (state: 'connecting' | 'connected' | 'disconnected' | 'error') => void) => {
      onState = fn;
      return () => { onState = undefined; };
    }),
    send: vi.fn(),
    emitMessage(message: unknown) { onMessage?.(message); },
    emitState(state: 'connecting' | 'connected' | 'disconnected' | 'error') { onState?.(state); },
  };
  return connection;
}

describe('WsDataProvider lifecycle', () => {
  it('rejects all in-flight data queries as soon as transport disconnects', async () => {
    const connection = fakeConnection();
    const provider = new WsDataProvider(connection as unknown as RemoteConnection);
    const pending = provider.readFile('/repo/file.txt');

    connection.emitState('disconnected');

    await expect(pending).rejects.toThrow('transport disconnected');
    provider.dispose();
  });

  it('cleans the pending entry when send throws synchronously', async () => {
    const connection = fakeConnection();
    connection.send.mockImplementation(() => { throw new Error('socket closed'); });
    const provider = new WsDataProvider(connection as unknown as RemoteConnection);

    await expect(provider.gitStatus('/repo')).rejects.toThrow('socket closed');
    provider.dispose();
  });

  it('honors an AbortSignal without waiting for the transport timeout', async () => {
    const connection = fakeConnection();
    const provider = new WsDataProvider(connection as unknown as RemoteConnection);
    const controller = new AbortController();
    const pending = provider.readFile('/repo/file.txt', controller.signal);

    controller.abort();

    await expect(pending).rejects.toMatchObject({ name: 'AbortError' });
    provider.dispose();
  });
});
