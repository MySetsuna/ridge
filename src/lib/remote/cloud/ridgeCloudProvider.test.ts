// ridgeCloudProvider 单测（契约 §5 信令 / §7 E2EE；零信任 #1/#2 概念 4-桌面）。
//
// 聚焦 host 端「握手时序反转（先收后发 0x02）」这一最高回归点（设计 §7.3/§7.4）：
//   • dc.onopen 时 host **不发** DataChannel 握手帧，仅经信令旁路上报 e2ee-pubkey。
//   • 收到 controller 的 0x01 握手帧后才派生会话 + 用注入的 signContext 异步签名 +
//     发 0x02（host_eph‖id_pub‖sig），且 sig 对 controller 验证通过。
//   • 未注入 signContext/identityPub → 回落发 0x01（向后兼容）。
//   • 握手不死锁：controller 先发、host 后发，最终建桥 connected。
//   • 异步签名期间断连不崩、不在 teardown 后发帧。
//   • 建桥时把本会话 bindTranscript 透传给 createBridge（供概念 5 的 totp-bind）。
//
// harness 与 controllerCloudProvider.test.ts 同构：fake WebSocket/RTCPeerConnection/
// RTCDataChannel；host 是 answerer，DataChannel 由 controller 创建（测试 fire ondatachannel）。

import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import { ed25519 } from '@noble/curves/ed25519.js';
import {
  E2eeSession,
  DIR_CONTROLLER_TO_HOST,
  generateEphemeralKeyPair,
  encodeHandshakeFrame,
  decodeSignedHandshakeFrame,
  deriveSessionKey,
  buildIdBindContext,
  buildBindTranscript,
  verifyIdBindSignature,
  bytesToBase64,
  ID_BIND_DOMAIN,
  DEVICE_BOUND_TAG,
  HANDSHAKE_TAG,
  type EphemeralKeyPair,
} from './e2ee';
import type { CloudHostBridgeLike } from './ridgeCloudProvider';

vi.mock('./apiClient', async (importOriginal) => {
  const actual = await importOriginal<typeof import('./apiClient')>();
  return {
    ...actual,
    getIceServers: vi.fn(async () => ({ iceServers: [{ urls: 'stun:stun.l.google.com:19302' }] })),
  };
});

// ── Fake WebRTC / WebSocket harness ──────────────────────────────────────────

class FakeDataChannel {
  binaryType = 'blob';
  readyState: 'connecting' | 'open' | 'closing' | 'closed' = 'connecting';
  bufferedAmount = 0;
  bufferedAmountLowThreshold = 0;
  onopen: (() => void) | null = null;
  onclose: (() => void) | null = null;
  onmessage: ((ev: { data: unknown }) => void) | null = null;
  onbufferedamountlow: (() => void) | null = null;
  sent: ArrayBuffer[] = [];

  constructor(readonly label: string) {}

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
  lastSent(): Uint8Array {
    return new Uint8Array(this.sent[this.sent.length - 1]);
  }
}

class FakePeerConnection {
  onicecandidate: ((ev: { candidate: { toJSON: () => RTCIceCandidateInit } | null }) => void) | null =
    null;
  onconnectionstatechange: (() => void) | null = null;
  ondatachannel: ((ev: { channel: FakeDataChannel }) => void) | null = null;
  connectionState: RTCPeerConnectionState = 'new';
  localDescription: RTCSessionDescriptionInit | null = null;
  remoteDescription: RTCSessionDescriptionInit | null = null;

  constructor(readonly config?: RTCConfiguration) {
    FakePeerConnection.instances.push(this);
  }
  static instances: FakePeerConnection[] = [];

  async createAnswer(): Promise<RTCSessionDescriptionInit> {
    return { type: 'answer', sdp: 'fake-answer-sdp' };
  }
  async setLocalDescription(d: RTCSessionDescriptionInit): Promise<void> {
    this.localDescription = d;
  }
  async setRemoteDescription(d: RTCSessionDescriptionInit): Promise<void> {
    this.remoteDescription = d;
  }
  async addIceCandidate(): Promise<void> {}
  close(): void {
    this.connectionState = 'closed';
  }

  /** 模拟 controller 创建 DataChannel(label="ridge") → host ondatachannel。 */
  attachControllerChannel(): FakeDataChannel {
    const dc = new FakeDataChannel('ridge');
    this.ondatachannel?.({ channel: dc });
    return dc;
  }
}

class FakeWebSocket {
  static OPEN = 1;
  static instances: FakeWebSocket[] = [];
  readyState = FakeWebSocket.OPEN;
  onmessage: ((ev: { data: unknown }) => void) | null = null;
  onerror: (() => void) | null = null;
  onclose: (() => void) | null = null;
  sent: string[] = [];

  constructor(readonly url: string) {
    FakeWebSocket.instances.push(this);
  }
  send(data: string): void {
    this.sent.push(data);
  }
  close(): void {
    this.readyState = 3;
  }
  deliver(msg: unknown): void {
    this.onmessage?.({ data: JSON.stringify(msg) });
  }
  sentParsed(): Array<Record<string, unknown>> {
    return this.sent.map((s) => JSON.parse(s) as Record<string, unknown>);
  }
}

function installGlobals(): void {
  (globalThis as unknown as { RTCPeerConnection: unknown }).RTCPeerConnection =
    FakePeerConnection as unknown as typeof RTCPeerConnection;
  (globalThis as unknown as { WebSocket: unknown }).WebSocket =
    FakeWebSocket as unknown as typeof WebSocket;
}

const flush = () => new Promise<void>((r) => setTimeout(r, 0));

async function loadHost() {
  return await import('./ridgeCloudProvider');
}

const CONFIG = { deviceToken: 'device-jwt-abc', username: 'alice' };
const DEVICE = 'my-laptop';
const CID = 'c1';

// ── host 设备身份（Ed25519）测试夹具：固定种子，便于断言 0x02 签名 ─────────────────
const ID_PRIV = new Uint8Array(32).fill(7);
const ID_PUB = ed25519.getPublicKey(ID_PRIV);
/** 注入的 signContext：对 `ID_BIND_DOMAIN || context` 做 Ed25519 签名（= 桌面 sign_device_identity）。 */
function signContext(context: Uint8Array): Promise<Uint8Array> {
  const domain = new TextEncoder().encode(ID_BIND_DOMAIN);
  const msg = new Uint8Array(domain.length + context.length);
  msg.set(domain, 0);
  msg.set(context, domain.length);
  return Promise.resolve(ed25519.sign(msg, ID_PRIV));
}

interface BridgeRecord {
  cid: string;
  bindTranscript: Uint8Array | null;
  send: (p: Uint8Array) => void;
}

/** 建一个最小 host：捕获 createBridge 调用（cid + bindTranscript），返回 host + 记录。 */
async function makeHost(opts: {
  signContext?: (c: Uint8Array) => Promise<Uint8Array>;
  identityPub?: Uint8Array;
}) {
  const { RidgeCloudHost } = await loadHost();
  const bridges: BridgeRecord[] = [];
  const errors: string[] = [];
  const host = new RidgeCloudHost(
    { ...CONFIG, signContext: opts.signContext, identityPub: opts.identityPub },
    {
      onError: (m) => errors.push(m),
      createBridge: (cid, send, bindTranscript): CloudHostBridgeLike => {
        bridges.push({ cid, bindTranscript, send });
        return {
          handleFrame: () => {},
          reset: () => {},
        };
      },
    },
  );
  return { host, bridges, errors };
}

/**
 * 驱动 host 到「DataChannel 打开 + 已上线」状态，返回 dc + ws。
 * 流程：goOnline → welcome → peer-join(controller,cid) → offer(cid) → ondatachannel → dc.open。
 */
async function driveToOpenChannel(host: InstanceType<Awaited<ReturnType<typeof loadHost>>['RidgeCloudHost']>) {
  await host.goOnline(DEVICE);
  await flush();
  const ws = FakeWebSocket.instances[0];
  ws.deliver({ t: 'welcome', room: `${DEVICE}-alice`, role: 'host', peerPresent: false });
  await flush();
  ws.deliver({ t: 'peer-join', role: 'controller', cid: CID });
  await flush();
  ws.deliver({ t: 'offer', sdp: 'controller-offer', cid: CID });
  await flush();
  const pc = FakePeerConnection.instances[0];
  const dc = pc.attachControllerChannel();
  dc.fireOpen();
  return { ws, pc, dc };
}

describe('RidgeCloudHost 概念 4-桌面：握手时序反转（先收后发 0x02）', () => {
  beforeEach(() => {
    FakePeerConnection.instances = [];
    FakeWebSocket.instances = [];
    installGlobals();
  });
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('dc.onopen 时 host 不发 DataChannel 握手帧，仅经信令上报 e2ee-pubkey', async () => {
    const { host } = await makeHost({ signContext, identityPub: ID_PUB });
    const { ws, dc } = await driveToOpenChannel(host);

    // 关键：先收后发 —— 收到 controller 帧前，DataChannel 上零字节。
    expect(dc.sent.length).toBe(0);
    // 但 B3 旁路 e2ee-pubkey 仍经信令发出（带 cid）。
    const pk = ws.sentParsed().filter((m) => m.t === 'e2ee-pubkey' && m.cid === CID);
    expect(pk).toHaveLength(1);

    host.goOffline();
  });

  it('收到 controller 0x01 → 发 0x02 签名帧，sig 对 controller 验证通过', async () => {
    const { host, bridges } = await makeHost({ signContext, identityPub: ID_PUB });
    const { ws, dc } = await driveToOpenChannel(host);

    // controller 侧：生成临时密钥对，旁路上报 e2ee-pubkey（B3，使 host 绑定判定 accept），
    // 再经 DataChannel 发 0x01 握手帧。
    const ctrlEph = generateEphemeralKeyPair();
    ws.deliver({ t: 'e2ee-pubkey', pubkey: bytesToBase64(ctrlEph.publicKey), cid: CID });
    await flush();
    dc.deliver(encodeHandshakeFrame(ctrlEph.publicKey));
    await flush(); // 等异步签名完成

    // host 在 DataChannel 上发出的应是 0x02 签名帧。
    const frame = dc.lastSent();
    expect(frame[0]).toBe(DEVICE_BOUND_TAG);
    expect(frame.length).toBe(129);
    const signed = decodeSignedHandshakeFrame(frame);

    // id_pub == 注入的设备身份公钥。
    expect(Array.from(signed.idPub)).toEqual(Array.from(ID_PUB));
    // sig 覆盖 context = buildIdBindContext(host_eph, ctrl_eph, device, username)，controller 验证通过。
    const context = buildIdBindContext(signed.ephPub, ctrlEph.publicKey, DEVICE, 'alice');
    expect(verifyIdBindSignature(signed.idPub, context, signed.sig)).toBe(true);

    // 派生同一会话密钥：controller(dir=1) 能解 host 发来的业务帧（不死锁、会话可用）。
    const ctrlKey = deriveSessionKey(ctrlEph.privateKey, ctrlEph.publicKey, signed.ephPub);
    const ctrlSession = new E2eeSession(ctrlKey, DIR_CONTROLLER_TO_HOST);
    bridges[0].send(new TextEncoder().encode('hello-from-host'));
    const wire = dc.lastSent();
    expect(new TextDecoder().decode(ctrlSession.open(wire))).toBe('hello-from-host');

    host.goOffline();
  });

  it('建桥时把本会话 bindTranscript 透传给 createBridge（供概念 5 totp-bind）', async () => {
    const { host, bridges } = await makeHost({ signContext, identityPub: ID_PUB });
    const { ws, dc } = await driveToOpenChannel(host);

    const ctrlEph = generateEphemeralKeyPair();
    ws.deliver({ t: 'e2ee-pubkey', pubkey: bytesToBase64(ctrlEph.publicKey), cid: CID });
    await flush();
    dc.deliver(encodeHandshakeFrame(ctrlEph.publicKey));
    await flush();

    expect(bridges).toHaveLength(1);
    const signed = decodeSignedHandshakeFrame(dc.lastSent());
    const expected = buildBindTranscript(signed.ephPub, ctrlEph.publicKey);
    expect(bridges[0].bindTranscript).not.toBeNull();
    expect(Array.from(bridges[0].bindTranscript!)).toEqual(Array.from(expected));

    host.goOffline();
  });

  it('未注入 signContext/identityPub → 收到 controller 0x01 后回落发 0x01（向后兼容）', async () => {
    const { host } = await makeHost({}); // 不注入签名能力
    const { ws, dc } = await driveToOpenChannel(host);

    const ctrlEph = generateEphemeralKeyPair();
    ws.deliver({ t: 'e2ee-pubkey', pubkey: bytesToBase64(ctrlEph.publicKey), cid: CID });
    await flush();
    dc.deliver(encodeHandshakeFrame(ctrlEph.publicKey));
    await flush();

    const frame = dc.lastSent();
    expect(frame[0]).toBe(HANDSHAKE_TAG); // 0x01 裸公钥
    expect(frame.length).toBe(33);

    host.goOffline();
  });

  it('异步签名期间 host 下线 → 不崩、teardown 后不再发帧', async () => {
    // 用一个永不 resolve 的 signContext，模拟签名在途时连接被拆。
    let resolveSig: ((s: Uint8Array) => void) | null = null;
    const pendingSign = (): Promise<Uint8Array> =>
      new Promise<Uint8Array>((r) => {
        resolveSig = r;
      });
    const { host } = await makeHost({ signContext: pendingSign, identityPub: ID_PUB });
    const { ws, dc } = await driveToOpenChannel(host);

    const ctrlEph = generateEphemeralKeyPair();
    ws.deliver({ t: 'e2ee-pubkey', pubkey: bytesToBase64(ctrlEph.publicKey), cid: CID });
    await flush();
    dc.deliver(encodeHandshakeFrame(ctrlEph.publicKey)); // 触发签名（pending）
    await flush();
    const sentBefore = dc.sent.length;

    // 签名仍在途时下线 → 拆连接。
    host.goOffline();
    // 现在签名「迟到」resolve：不应在已 teardown 的连接上发 0x02。
    resolveSig?.(new Uint8Array(64).fill(9));
    await flush();
    expect(dc.sent.length).toBe(sentBefore); // 无新增帧
  });
});
