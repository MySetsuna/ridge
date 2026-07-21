import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { encodeChunks } from '../transport/cloudChunk';
import { encodeControlFrame } from '../transport/cloudMux';
import { createCloudWebrtcTransportWith } from '../transport/cloudWebrtcAdapter';
import { RpcClient } from '../transport/rpcClient';
import { RpcReconnectError } from '../transport/types';
import type {
  AuthListener,
  AuthState,
  ChannelTransport,
  ControlFrame,
  ControlListener,
  PaneBytesListener,
  StateListener,
  TransportState,
  Unsubscribe,
} from '../transport/types';
import { CloudHostBridge } from './cloudHostBridge';
import { ControllerCloudProvider } from './controllerCloudProvider';
import {
  bytesToBase64,
  decodeHandshakeFrame,
  deriveSessionKey,
  DIR_HOST_TO_CONTROLLER,
  E2eeSession,
  encodeHandshakeFrame,
  generateEphemeralKeyPair,
} from './e2ee';

vi.mock('./apiClient', async (importOriginal) => {
  const actual = await importOriginal<typeof import('./apiClient')>();
  return {
    ...actual,
    getIceServers: vi.fn(async () => ({ iceServers: [{ urls: 'stun:test.invalid' }] })),
  };
});

class AuthGatedTransport implements ChannelTransport {
  readonly sent: ControlFrame[] = [];
  private readonly controlListeners = new Set<ControlListener>();
  private readonly stateListeners = new Set<StateListener>();
  private readonly authListeners = new Set<AuthListener>();
  private currentState: TransportState = 'disconnected';
  private currentAuth: AuthState = 'pending';

  sendControl(frame: ControlFrame): void {
    this.sent.push(frame);
  }
  onControl(cb: ControlListener): Unsubscribe {
    this.controlListeners.add(cb);
    return () => this.controlListeners.delete(cb);
  }
  sendPaneBytes(): void {}
  onPaneBytes(_cb: PaneBytesListener): Unsubscribe {
    return () => {};
  }
  connect(): void {}
  close(): void {}
  state(): TransportState {
    return this.currentState;
  }
  onStateChange(cb: StateListener): Unsubscribe {
    this.stateListeners.add(cb);
    return () => this.stateListeners.delete(cb);
  }
  authState(): AuthState {
    return this.currentAuth;
  }
  onAuthChange(cb: AuthListener): Unsubscribe {
    this.authListeners.add(cb);
    return () => this.authListeners.delete(cb);
  }

  setState(state: TransportState): void {
    this.currentState = state;
    for (const cb of this.stateListeners) cb(state);
  }
  setAuth(auth: AuthState): void {
    this.currentAuth = auth;
    for (const cb of this.authListeners) cb(auth);
  }
}

class FaultDataChannel {
  binaryType = 'blob';
  readyState: 'connecting' | 'open' | 'closing' | 'closed' = 'connecting';
  onopen: (() => void) | null = null;
  onclose: (() => void) | null = null;
  onmessage: ((event: { data: unknown }) => void) | null = null;
  readonly sent: ArrayBuffer[] = [];

  constructor(
    readonly label: string,
    readonly init?: RTCDataChannelInit,
  ) {}

  send(data: ArrayBuffer): void {
    this.sent.push(data);
  }
  close(): void {
    this.readyState = 'closed';
  }
  fireOpen(): void {
    this.readyState = 'open';
    this.onopen?.();
  }
  deliver(bytes: Uint8Array): void {
    this.onmessage?.({
      data: bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength),
    });
  }
  firstSent(): Uint8Array {
    return new Uint8Array(this.sent[0]);
  }
}

class FaultPeerConnection {
  static instances: FaultPeerConnection[] = [];
  onicecandidate: ((event: { candidate: null }) => void) | null = null;
  onconnectionstatechange: (() => void) | null = null;
  ondatachannel: ((event: { channel: FaultDataChannel }) => void) | null = null;
  connectionState: RTCPeerConnectionState = 'new';
  localDescription: RTCSessionDescriptionInit | null = null;
  remoteDescription: RTCSessionDescriptionInit | null = null;
  readonly offers: Array<RTCOfferOptions | undefined> = [];
  channel: FaultDataChannel | null = null;

  constructor(readonly config?: RTCConfiguration) {
    FaultPeerConnection.instances.push(this);
  }
  createDataChannel(label: string, init?: RTCDataChannelInit): FaultDataChannel {
    this.channel = new FaultDataChannel(label, init);
    return this.channel;
  }
  async createOffer(options?: RTCOfferOptions): Promise<RTCSessionDescriptionInit> {
    this.offers.push(options);
    return { type: 'offer', sdp: 'fault-offer' };
  }
  async setLocalDescription(description: RTCSessionDescriptionInit): Promise<void> {
    this.localDescription = description;
  }
  async setRemoteDescription(description: RTCSessionDescriptionInit): Promise<void> {
    this.remoteDescription = description;
  }
  async addIceCandidate(): Promise<void> {}
  close(): void {
    this.connectionState = 'closed';
  }
}

class FaultWebSocket {
  static readonly OPEN = 1;
  static instances: FaultWebSocket[] = [];
  readyState = FaultWebSocket.OPEN;
  onopen: (() => void) | null = null;
  onmessage: ((event: { data: unknown }) => void) | null = null;
  onerror: (() => void) | null = null;
  onclose: ((event: { code: number; reason: string; wasClean: boolean }) => void) | null = null;
  readonly sent: string[] = [];

  constructor(readonly url: string) {
    FaultWebSocket.instances.push(this);
  }
  send(data: string): void {
    this.sent.push(data);
  }
  close(): void {
    this.readyState = 3;
  }
  fireOpen(): void {
    this.onopen?.();
  }
  fireClose(): void {
    this.readyState = 3;
    this.onclose?.({ code: 1006, reason: 'fault', wasClean: false });
  }
  deliver(message: unknown): void {
    this.onmessage?.({ data: JSON.stringify(message) });
  }
}

function installFaultGlobals(): void {
  (globalThis as unknown as { RTCPeerConnection: unknown }).RTCPeerConnection =
    FaultPeerConnection as unknown as typeof RTCPeerConnection;
  (globalThis as unknown as { WebSocket: unknown }).WebSocket =
    FaultWebSocket as unknown as typeof WebSocket;
}

function completeE2ee(
  pc: FaultPeerConnection,
  ws: FaultWebSocket,
): { dc: FaultDataChannel; hostSession: E2eeSession } {
  const dc = pc.channel!;
  dc.fireOpen();
  const controllerPub = decodeHandshakeFrame(dc.firstSent());
  const hostKeyPair = generateEphemeralKeyPair();
  const key = deriveSessionKey(
    hostKeyPair.privateKey,
    hostKeyPair.publicKey,
    controllerPub,
  );
  const hostSession = new E2eeSession(key, DIR_HOST_TO_CONTROLLER);
  ws.deliver({ t: 'e2ee-pubkey', pubkey: bytesToBase64(hostKeyPair.publicKey) });
  dc.deliver(encodeHandshakeFrame(hostKeyPair.publicKey));
  pc.connectionState = 'connected';
  pc.onconnectionstatechange?.();
  return { dc, hostSession };
}

function authorize(dc: FaultDataChannel, hostSession: E2eeSession, messageId: number): void {
  const plaintext = encodeControlFrame({ t: 'totp-result', ok: true });
  for (const chunk of encodeChunks(hostSession.seal(plaintext), messageId)) dc.deliver(chunk);
}

async function createControllerRig() {
  let provider!: ControllerCloudProvider;
  const adapter = createCloudWebrtcTransportWith('fault-host', (callbacks) => {
    provider = new ControllerCloudProvider(
      { userToken: 'fault-token', username: 'alice', baseDomain: 'localhost' },
      callbacks,
    );
    return provider;
  });
  const rpc = new RpcClient(adapter, { defaultTimeoutMs: 0 });
  rpc.hello();
  await adapter.connect();
  await vi.advanceTimersByTimeAsync(0);
  const pc = FaultPeerConnection.instances.at(-1)!;
  const ws = FaultWebSocket.instances.at(-1)!;
  ws.fireOpen();
  const { dc, hostSession } = completeE2ee(pc, ws);
  authorize(dc, hostSession, 0);
  expect(adapter.authState()).toBe('authorized');
  return { provider, adapter, rpc, pc, ws, dc, hostSession };
}

describe('Cloud Remote deterministic fault injection', () => {
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
  it('resyncs each affected pane exactly once per drain without crossing panes', async () => {
    const invoke = vi.fn(async () => null);
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

    bridge.pushPaneOutput('pane-a', new Uint8Array([1]));
    bridge.pushPaneOutput('pane-a', new Uint8Array([2]));
    bridge.pushPaneOutput('pane-b', new Uint8Array([3]));
    buffered = 0;
    drain?.();
    drain?.();
    await Promise.resolve();
    expect(invoke.mock.calls).toEqual([
      ['resync_pane_raw', { paneId: 'pane-a' }],
      ['resync_pane_raw', { paneId: 'pane-b' }],
    ]);

    bridge.pushPaneOutput('pane-c', new Uint8Array([4]));
    buffered = 9 * 1024 * 1024;
    bridge.pushPaneOutput('pane-a', new Uint8Array([5]));
    buffered = 0;
    drain?.();
    await Promise.resolve();
    expect(invoke.mock.calls).toEqual([
      ['resync_pane_raw', { paneId: 'pane-a' }],
      ['resync_pane_raw', { paneId: 'pane-b' }],
      ['resync_pane_raw', { paneId: 'pane-a' }],
    ]);
    expect(sent).toHaveLength(1);
  });
});
