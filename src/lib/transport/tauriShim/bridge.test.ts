import { describe, expect, it } from 'vitest';
import { RpcCancelledError } from '@ridge/remote';
import type {
  AuthListener,
  ChannelTransport,
  ControlListener,
  OutboundFrame,
  PaneBytesListener,
  StateListener,
} from '@ridge/remote';
import { TauriBridge } from './bridge';

function rig() {
  const sent: OutboundFrame[] = [];
  let disposed = 0;
  const subscribe = <T>(_cb: T) => () => { disposed += 1; };
  const transport: ChannelTransport = {
    sendControl: (frame) => sent.push(frame),
    onControl: (cb: ControlListener) => subscribe(cb),
    sendPaneBytes: () => {},
    onPaneBytes: (cb: PaneBytesListener) => subscribe(cb),
    connect: () => {},
    close: () => {},
    state: () => 'connected',
    onStateChange: (cb: StateListener) => subscribe(cb),
    authState: () => 'authorized',
    onAuthChange: (cb: AuthListener) => subscribe(cb),
  };
  return { transport, sent, disposed: () => disposed };
}

describe('TauriBridge isolated attachment', () => {
  it('negotiates hello without enabling global workspace semantics', () => {
    const { transport, sent, disposed } = rig();
    const bridge = new TauriBridge();
    bridge.attach(transport, { useGlobalWorkspace: false });

    expect(bridge.ready).toBe(true);
    expect(sent.some((frame) => 'method' in frame && frame.method === '$/hello')).toBe(true);
    expect(sent.some((frame) => 'method' in frame && frame.method === 'use-global-workspace')).toBe(false);

    bridge.detach();
    expect(bridge.ready).toBe(false);
    expect(disposed()).toBeGreaterThan(0);
  });

  it('forwards AbortSignal to RpcClient and emits $/cancel', async () => {
    const { transport, sent } = rig();
    const bridge = new TauriBridge();
    bridge.attach(transport, { useGlobalWorkspace: false });
    const controller = new AbortController();
    const pending = bridge.invoke(
      'write_to_pty',
      { paneId: 'pane-a' },
      { signal: controller.signal },
    );
    const request = sent.find(
      (frame) => 'id' in frame && 'method' in frame && frame.method === 'write_to_pty',
    );
    expect(request).toBeDefined();

    controller.abort();
    await expect(pending).rejects.toBeInstanceOf(RpcCancelledError);
    expect(sent).toContainEqual({
      jsonrpc: '2.0',
      method: '$/cancel',
      params: { id: request && 'id' in request ? request.id : -1 },
    });
  });
});
