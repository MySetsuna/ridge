// 测试专用共享 rig（命名沿 __cloudE2eHarness.ts 先例，仅被 *.test.ts 引用，不入产物）。
// 从 faultInjection.test.ts 抽出（iteration 7 G2），供确定性故障注入门禁与弱网实验室
// 参数化扫描（weakNetLab.test.ts）共用同一套 fake RTC/WS/E2EE 组件——两处不得漂移。
//
// 注意：`vi.mock('./apiClient', …)` 必须留在各 test 文件顶层（vitest hoist 语义），
// 本模块只提供类与构造器。

import { expect, vi } from 'vitest';
import { encodeChunks } from '../transport/cloudChunk';
import { encodeControlFrame } from '../transport/cloudMux';
import { createCloudWebrtcTransportWith } from '../transport/cloudWebrtcAdapter';
import { RpcClient } from '../transport/rpcClient';
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

export class AuthGatedTransport implements ChannelTransport {
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

export class FaultDataChannel {
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

export class FaultPeerConnection {
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

export class FaultWebSocket {
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

export function installFaultGlobals(): void {
  (globalThis as unknown as { RTCPeerConnection: unknown }).RTCPeerConnection =
    FaultPeerConnection as unknown as typeof RTCPeerConnection;
  (globalThis as unknown as { WebSocket: unknown }).WebSocket =
    FaultWebSocket as unknown as typeof WebSocket;
}

export function completeE2ee(
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

export function authorize(dc: FaultDataChannel, hostSession: E2eeSession, messageId: number): void {
  const plaintext = encodeControlFrame({ t: 'totp-result', ok: true });
  for (const chunk of encodeChunks(hostSession.seal(plaintext), messageId)) dc.deliver(chunk);
}

export async function createControllerRig() {
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
