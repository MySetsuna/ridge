import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { RpcClient } from '../transport/rpcClient';
import { RpcReconnectError } from '../transport/types';
import { CloudHostBridge } from './cloudHostBridge';
import { encodeJsonFrame } from '../transport/cloudMux';
import {
  AuthGatedTransport,
  authorize,
  completeE2ee,
  createControllerRig,
  FaultPeerConnection,
  FaultWebSocket,
  installFaultGlobals,
} from './__faultRig';

vi.mock('./apiClient', async (importOriginal) => {
  const actual = await importOriginal<typeof import('./apiClient')>();
  return {
    ...actual,
    getIceServers: vi.fn(async () => ({ iceServers: [{ urls: 'stun:test.invalid' }] })),
  };
});

vi.mock('../transport/random', () => ({ secureRandomUnit: () => 0 }));

describe('Cloud Remote deterministic fault injection', () => {
  it('records pane traffic and exposes lifecycle/candidate transitions in the rig', async () => {
    const transport = new AuthGatedTransport();
    const states: string[] = [];
    const panes: Array<[string, Uint8Array]> = [];
    transport.onStateChange((state) => states.push(state));
    transport.onPaneBytes((paneId, bytes) => panes.push([paneId, bytes]));
    transport.connect();
    transport.sendPaneBytes('pane-a', new Uint8Array([1, 2]));
    transport.close();
    expect(states).toEqual(['connecting', 'closed']);
    expect(transport.paneBytes[0]).toEqual({ paneId: 'pane-a', bytes: new Uint8Array([1, 2]) });
    expect(panes).toHaveLength(1);
    expect([...panes[0][1]]).toEqual([1, 2]);

    const pc = new FaultPeerConnection();
    await pc.addIceCandidate({ candidate: 'candidate:test' });
    expect(pc.iceCandidates).toEqual([{ candidate: 'candidate:test' }]);
  });

  it('defers hello and pane recovery until the reconnected transport is authorized', () => {
    const transport = new AuthGatedTransport();
    const rpc = new RpcClient(transport);
    const subscribe = vi.fn(() => rpc.notify('subscribe-pane', { paneId: 'pane-a' }));
    rpc.onReconnected(subscribe);

    rpc.hello();
    transport.setState('connecting');
    transport.setState('connected');
    expect(transport.sent).toHaveLength(0);
    expect(subscribe).not.toHaveBeenCalled();

    transport.setAuth('authorized');
    expect(transport.sent.map((frame) => frame.method)).toEqual(['$/hello', 'subscribe-pane']);
    expect(subscribe).toHaveBeenCalledTimes(1);

    transport.setAuth('pending');
    transport.setState('disconnected');
    transport.setState('connecting');
    transport.setState('connected');
    expect(transport.sent).toHaveLength(2);

    transport.setAuth('authorized');
    expect(transport.sent.map((frame) => frame.method)).toEqual([
      '$/hello',
      'subscribe-pane',
      '$/hello',
      'subscribe-pane',
    ]);
    expect(subscribe).toHaveBeenCalledTimes(2);
  });
});

describe('Cloud Remote provider → adapter → RpcClient recovery', () => {
  beforeEach(() => {
    FaultPeerConnection.instances = [];
    FaultWebSocket.instances = [];
    installFaultGlobals();
    vi.useFakeTimers();
    vi.spyOn(Math, 'random').mockReturnValue(0);
    vi.spyOn(console, 'log').mockImplementation(() => {});
    vi.spyOn(console, 'warn').mockImplementation(() => {});
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  it('settles in-flight RPC immediately, ICE-restarts once, then recovers once after auth', async () => {
    const rig = await createControllerRig();
    const subscribe = vi.fn(() => rig.rpc.notify('subscribe-pane', { paneId: 'pane-a' }));
    rig.rpc.onReconnected(subscribe);
    const framesBeforeFailure = rig.dc.sent.length;
    const pending = rig.rpc.request('read_file', { path: '/tmp/a' });

    rig.pc.connectionState = 'failed';
    rig.pc.onconnectionstatechange?.();
    await expect(pending).rejects.toBeInstanceOf(RpcReconnectError);
    expect(rig.rpc.inFlight).toBe(0);
    expect(rig.adapter.authState()).toBe('pending');

    await vi.advanceTimersByTimeAsync(999);
    expect(rig.pc.offers).toHaveLength(0);
    await vi.advanceTimersByTimeAsync(1);
    expect(FaultPeerConnection.instances).toHaveLength(1);
    expect(rig.pc.offers).toEqual([{ iceRestart: true }]);

    rig.pc.connectionState = 'connected';
    rig.pc.onconnectionstatechange?.();
    expect(subscribe).not.toHaveBeenCalled();
    expect(rig.dc.sent).toHaveLength(framesBeforeFailure + 1);

    authorize(rig.dc, rig.hostSession, 1);
    expect(subscribe).toHaveBeenCalledTimes(1);
    expect(rig.dc.sent).toHaveLength(framesBeforeFailure + 3);
    expect(vi.getTimerCount()).toBe(0);

    rig.rpc.dispose();
    rig.adapter.close();
    rig.adapter.dispose();
  });

  it('rebuilds PC/DC when signaling is gone and does not restore panes before fresh auth', async () => {
    const rig = await createControllerRig();
    const subscribe = vi.fn(() => rig.rpc.notify('subscribe-pane', { paneId: 'pane-a' }));
    rig.rpc.onReconnected(subscribe);

    rig.ws.fireClose();
    rig.pc.connectionState = 'failed';
    rig.pc.onconnectionstatechange?.();
    await vi.advanceTimersByTimeAsync(1000);
    expect(FaultPeerConnection.instances).toHaveLength(2);
    expect(FaultWebSocket.instances).toHaveLength(2);
    expect(rig.pc.connectionState).toBe('closed');

    const pc2 = FaultPeerConnection.instances[1];
    const ws2 = FaultWebSocket.instances[1];
    ws2.fireOpen();
    const next = completeE2ee(pc2, ws2);
    expect(rig.adapter.authState()).toBe('pending');
    expect(subscribe).not.toHaveBeenCalled();
    expect(next.dc.sent).toHaveLength(1);

    authorize(next.dc, next.hostSession, 0);
    expect(rig.adapter.authState()).toBe('authorized');
    expect(subscribe).toHaveBeenCalledTimes(1);
    expect(next.dc.sent).toHaveLength(3);
    expect(vi.getTimerCount()).toBe(0);

    rig.rpc.dispose();
    rig.adapter.close();
    rig.adapter.dispose();
  });

  it('lets a sub-15s disconnected pulse self-heal without ICE restart or duplicate recovery', async () => {
    const rig = await createControllerRig();
    const subscribe = vi.fn(() => rig.rpc.notify('subscribe-pane', { paneId: 'pane-a' }));
    rig.rpc.onReconnected(subscribe);
    const sentBeforePulse = rig.dc.sent.length;

    rig.pc.connectionState = 'disconnected';
    rig.pc.onconnectionstatechange?.();
    await vi.advanceTimersByTimeAsync(14_999);
    expect(rig.pc.offers).toHaveLength(0);
    expect(FaultPeerConnection.instances).toHaveLength(1);
    expect(FaultWebSocket.instances).toHaveLength(1);

    rig.pc.connectionState = 'connected';
    rig.pc.onconnectionstatechange?.();
    expect(rig.adapter.authState()).toBe('authorized');
    expect(subscribe).not.toHaveBeenCalled();
    expect(rig.dc.sent).toHaveLength(sentBeforePulse);
    expect(vi.getTimerCount()).toBe(0);

    rig.rpc.dispose();
    rig.adapter.close();
    rig.adapter.dispose();
  });

  it('escalates watchdog to ICE deadline rebuild and recovers once after fresh auth', async () => {
    const rig = await createControllerRig();
    const subscribe = vi.fn(() => rig.rpc.notify('subscribe-pane', { paneId: 'pane-a' }));
    rig.rpc.onReconnected(subscribe);

    rig.pc.connectionState = 'disconnected';
    rig.pc.onconnectionstatechange?.();
    await vi.advanceTimersByTimeAsync(15_000);
    expect(rig.adapter.authState()).toBe('pending');
    expect(rig.pc.offers).toHaveLength(0);

    await vi.advanceTimersByTimeAsync(1_000);
    expect(rig.pc.offers).toEqual([{ iceRestart: true }]);
    expect(FaultPeerConnection.instances).toHaveLength(1);

    await vi.advanceTimersByTimeAsync(12_000);
    expect(FaultPeerConnection.instances).toHaveLength(2);
    expect(FaultWebSocket.instances).toHaveLength(2);
    expect(rig.pc.connectionState).toBe('closed');
    expect(rig.dc.readyState).toBe('closed');

    const pc2 = FaultPeerConnection.instances[1];
    const ws2 = FaultWebSocket.instances[1];
    ws2.fireOpen();
    const next = completeE2ee(pc2, ws2);
    expect(rig.adapter.authState()).toBe('pending');
    expect(subscribe).not.toHaveBeenCalled();
    expect(next.dc.sent).toHaveLength(1);

    authorize(next.dc, next.hostSession, 0);
    expect(rig.adapter.authState()).toBe('authorized');
    expect(subscribe).toHaveBeenCalledTimes(1);
    expect(next.dc.sent).toHaveLength(3);
    expect(vi.getTimerCount()).toBe(0);

    rig.rpc.dispose();
    rig.adapter.close();
    rig.adapter.dispose();
  });

  it('runs 100 deterministic fail/recover cycles without pending RPCs, duplicate recovery, or timers', async () => {
    const rig = await createControllerRig();
    const subscribe = vi.fn(() => rig.rpc.notify('subscribe-pane', { paneId: 'pane-a' }));
    rig.rpc.onReconnected(subscribe);
    const initialSent = rig.dc.sent.length;

    for (let cycle = 1; cycle <= 100; cycle += 1) {
      const pending = rig.rpc.request('get_pane_layout');
      rig.pc.connectionState = 'failed';
      rig.pc.onconnectionstatechange?.();
      await expect(pending).rejects.toBeInstanceOf(RpcReconnectError);
      await vi.advanceTimersByTimeAsync(1000);
      rig.pc.connectionState = 'connected';
      rig.pc.onconnectionstatechange?.();
      expect(subscribe).toHaveBeenCalledTimes(cycle - 1);
      authorize(rig.dc, rig.hostSession, cycle);
      expect(subscribe).toHaveBeenCalledTimes(cycle);
      expect(rig.rpc.inFlight).toBe(0);
      expect(vi.getTimerCount()).toBe(0);
    }

    expect(rig.pc.offers).toHaveLength(100);
    expect(rig.pc.offers.every((offer) => offer?.iceRestart === true)).toBe(true);
    expect(rig.dc.sent).toHaveLength(initialSent + 100 * 3);

    rig.rpc.dispose();
    rig.adapter.close();
    rig.adapter.dispose();
  });
});

describe('CloudHostBridge deterministic pane backpressure', () => {
  it('reserves drain recovery for active pane; dirty background recovers on promotion', async () => {
    const invoke = vi.fn(async () => ({ frame: '\x1bcRECOVERED' }));
    const sent: Uint8Array[] = [];
    const bridge = new CloudHostBridge({ invoke, sendFrame: (frame) => sent.push(frame) });
    let buffered = 9 * 1024 * 1024;
    let drain: (() => void) | null = null;
    bridge.attachChannelControl({
      bufferedAmount: () => buffered,
      onDrained: (cb) => {
        drain = cb;
        return () => {
          drain = null;
        };
      },
    });

    bridge.handleFrame(encodeJsonFrame({
      jsonrpc: '2.0',
      method: 'subscribe-pane',
      params: { paneId: 'pane-a', active: true },
    }));
    bridge.handleFrame(encodeJsonFrame({
      jsonrpc: '2.0',
      method: 'subscribe-pane',
      params: { paneId: 'pane-b', active: false },
    }));
    bridge.pushPaneOutput('pane-a', new Uint8Array([1]));
    bridge.pushPaneOutput('pane-a', new Uint8Array([2]));
    bridge.pushPaneOutput('pane-b', new Uint8Array([3]));
    buffered = 0;
    drain?.();
    drain?.();
    await vi.waitFor(() => expect(invoke).toHaveBeenCalledTimes(1));
    expect(invoke.mock.calls[0][0]).toBe('get_pane_resync_frame');
    expect(invoke.mock.calls[0][1]).toEqual({
      paneId: 'pane-a',
      workspaceId: undefined,
      maxBytes: 256 * 1024,
    });

    bridge.handleFrame(encodeJsonFrame({
      jsonrpc: '2.0',
      method: 'subscribe-pane',
      params: { paneId: 'pane-b', active: true },
    }));
    await vi.waitFor(() => expect(invoke).toHaveBeenCalledTimes(2));
    expect(invoke.mock.calls[1][1]).toEqual({
      paneId: 'pane-b',
      workspaceId: undefined,
      maxBytes: 256 * 1024,
    });
    expect(sent).toHaveLength(2);
  });
});
