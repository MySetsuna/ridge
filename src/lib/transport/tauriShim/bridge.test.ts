import { describe, expect, it } from 'vitest';
import { RpcCancelledError } from '@ridge/remote';
import type {
  AuthListener,
  ChannelTransport,
  ControlFrame,
  ControlListener,
  OutboundFrame,
  PaneBytesListener,
  StateListener,
} from '@ridge/remote';
import { TauriBridge } from './bridge';

function rig() {
  const sent: OutboundFrame[] = [];
  const controlListeners = new Set<ControlListener>();
  let disposed = 0;
  const subscribe = <T>(_cb: T) => () => { disposed += 1; };
  const transport: ChannelTransport = {
    sendControl: (frame) => sent.push(frame),
    onControl: (cb: ControlListener) => {
      controlListeners.add(cb);
      return () => { controlListeners.delete(cb); disposed += 1; };
    },
    sendPaneBytes: () => {},
    onPaneBytes: (cb: PaneBytesListener) => subscribe(cb),
    connect: () => {},
    close: () => {},
    state: () => 'connected',
    onStateChange: (cb: StateListener) => subscribe(cb),
    authState: () => 'authorized',
    onAuthChange: (cb: AuthListener) => subscribe(cb),
  };
  return {
    transport,
    sent,
    receive: (frame: ControlFrame) => controlListeners.forEach((cb) => cb(frame)),
    disposed: () => disposed,
  };
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

  it('unwraps legacy host Result envelopes before resolving invoke', async () => {
    const { transport, sent, receive } = rig();
    const bridge = new TauriBridge();
    bridge.attach(transport, { useGlobalWorkspace: false });

    const pending = bridge.invoke<{ workspaces: string[] }>('list_workspaces');
    const request = sent.find(
      (frame) => 'id' in frame && 'method' in frame && frame.method === 'list_workspaces',
    );
    expect(request).toBeDefined();
    receive({
      jsonrpc: '2.0',
      id: request && 'id' in request ? request.id : -1,
      result: { Ok: { workspaces: ['workspace-a'] } },
    });

    await expect(pending).resolves.toEqual({ workspaces: ['workspace-a'] });
  });

  it('turns a legacy host Err envelope into a rejected invoke', async () => {
    const { transport, sent, receive } = rig();
    const bridge = new TauriBridge();
    bridge.attach(transport, { useGlobalWorkspace: false });

    const pending = bridge.invoke('get_teammate_topology');
    const request = sent.find(
      (frame) => 'id' in frame && 'method' in frame && frame.method === 'get_teammate_topology',
    );
    expect(request).toBeDefined();
    receive({
      jsonrpc: '2.0',
      id: request && 'id' in request ? request.id : -1,
      result: { Err: 'method not supported by kernel host: get_teammate_topology' },
    });

    await expect(pending).rejects.toThrow('method not supported by kernel host');
  });
});
