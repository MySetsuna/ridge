// controllerCloudProvider 单测（契约 §5 信令 / §7 E2EE）。
//
// 用 fake signaling WebSocket + fake RTCPeerConnection/RTCDataChannel 驱动整条
// offerer 流程，验证：
//   • offer 创建 + 发出（controller=offerer，契约 §5.1）；信令 URL/role=controller。
//   • DataChannel(label="ridge", ordered) 由 controller 主动创建。
//   • §7.1 E2EE 握手：本端发 0x01||pub32；用 dir=1（DIR_CONTROLLER_TO_HOST）的 session
//     与一个 dir=0 的 host session 互通（onFrame 解密上抛、sendFrame 加密发出）。
//   • 状态机：disconnected → connecting → handshaking → connected → disconnected。
//   • peer-leave / 信令 error / 非握手首帧的失败路径。

import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import {
  E2eeSession,
  DIR_HOST_TO_CONTROLLER,
  generateEphemeralKeyPair,
  encodeHandshakeFrame,
  encodeSignedHandshakeFrame,
  decodeHandshakeFrame,
  deriveSessionKey,
  bytesToBase64,
  BYE_REASON_SIGNATURE_INVALID,
  type EphemeralKeyPair,
} from './e2ee';
import { demuxFrame } from '../../transport/remote/cloudMux';
import { encodeChunks, ChunkReassembler } from '../../transport/remote/cloudChunk';
import type { CloudConnectionState } from './connectionProvider';

// ── 传输层分片测试帮手（握手后业务帧都经 cloudChunk 封装：单帧 = 0x00||ciphertext）──
/** 把单帧密文包成 provider 期望的 SINGLE 线消息（用于 dc.deliver 模拟入站业务帧）。 */
function wrapSingle(ciphertext: Uint8Array): Uint8Array {
  return encodeChunks(ciphertext, 0)[0];
}
/** 从 provider 发出的（单条）线消息还原出密文（用于断言出站业务帧）。 */
function unwrapSingle(wire: Uint8Array): Uint8Array {
  const ct = new ChunkReassembler().push(wire);
  if (!ct) throw new Error('test: failed to reassemble single wire frame');
  return ct;
}

// getIceServers 网络调用 mock 掉；BASE_DOMAIN 用真实常量。
vi.mock('./apiClient', async (importOriginal) => {
  const actual = await importOriginal<typeof import('./apiClient')>();
  return {
    ...actual,
    getIceServers: vi.fn(async () => ({ iceServers: [{ urls: 'stun:stun.l.google.com:19302' }] })),
  };
});

// ── Fake WebRTC / WebSocket harness ──────────────────────────────────────────

/** 测试可驱动的 fake DataChannel。 */
class FakeDataChannel {
  binaryType = 'blob';
  readyState: 'connecting' | 'open' | 'closing' | 'closed' = 'connecting';
  onopen: (() => void) | null = null;
  onclose: (() => void) | null = null;
  onmessage: ((ev: { data: unknown }) => void) | null = null;
  /** provider rawSend 出来的二进制帧（ArrayBuffer）。 */
  sent: ArrayBuffer[] = [];

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

  // ── 测试驱动 ──
  /** 模拟 DTLS/SCTP 建立完成，channel open。 */
  fireOpen(): void {
    this.readyState = 'open';
    this.onopen?.();
  }
  /** 模拟从对端（host）收到一帧二进制数据。 */
  deliver(bytes: Uint8Array): void {
    this.onmessage?.({
      data: bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength),
    });
  }
  /** 取最近一帧 sent，转回 Uint8Array。 */
  lastSent(): Uint8Array {
    return new Uint8Array(this.sent[this.sent.length - 1]);
  }
}

/** 测试可驱动的 fake RTCPeerConnection。 */
class FakePeerConnection {
  onicecandidate: ((ev: { candidate: { toJSON: () => RTCIceCandidateInit } | null }) => void) | null = null;
  onconnectionstatechange: (() => void) | null = null;
  ondatachannel: ((ev: { channel: FakeDataChannel }) => void) | null = null;
  connectionState: RTCPeerConnectionState = 'new';

  localDescription: RTCSessionDescriptionInit | null = null;
  remoteDescription: RTCSessionDescriptionInit | null = null;
  addedCandidates: RTCIceCandidateInit[] = [];
  channel: FakeDataChannel | null = null;

  constructor(readonly config?: RTCConfiguration) {
    FakePeerConnection.instances.push(this);
  }
  static instances: FakePeerConnection[] = [];

  createDataChannel(label: string, init?: RTCDataChannelInit): FakeDataChannel {
    this.channel = new FakeDataChannel(label, init);
    return this.channel;
  }
  async createOffer(): Promise<RTCSessionDescriptionInit> {
    return { type: 'offer', sdp: 'fake-offer-sdp' };
  }
  async createAnswer(): Promise<RTCSessionDescriptionInit> {
    return { type: 'answer', sdp: 'fake-answer-sdp' };
  }
  async setLocalDescription(d: RTCSessionDescriptionInit): Promise<void> {
    this.localDescription = d;
  }
  async setRemoteDescription(d: RTCSessionDescriptionInit): Promise<void> {
    this.remoteDescription = d;
  }
  async addIceCandidate(c: RTCIceCandidateInit): Promise<void> {
    this.addedCandidates.push(c);
  }
  close(): void {
    this.connectionState = 'closed';
  }
}

/** 测试可驱动的 fake signaling WebSocket。 */
class FakeWebSocket {
  static OPEN = 1;
  static instances: FakeWebSocket[] = [];
  readyState = FakeWebSocket.OPEN;
  onmessage: ((ev: { data: unknown }) => void) | null = null;
  onerror: (() => void) | null = null;
  onclose: (() => void) | null = null;
  /** 客户端经信令发出的 JSON 文本。 */
  sent: string[] = [];

  constructor(readonly url: string) {
    FakeWebSocket.instances.push(this);
  }
  send(data: string): void {
    this.sent.push(data);
  }
  close(): void {
    this.readyState = 3; // CLOSED
  }

  // ── 测试驱动 ──
  deliver(msg: unknown): void {
    this.onmessage?.({ data: JSON.stringify(msg) });
  }
  sentParsed(): Array<Record<string, unknown>> {
    return this.sent.map((s) => JSON.parse(s) as Record<string, unknown>);
  }
}

// 安装全局 fake（jsdom 无 WebRTC；node 无 WebSocket/RTCPeerConnection）。
function installGlobals(): void {
  (globalThis as unknown as { RTCPeerConnection: unknown }).RTCPeerConnection =
    FakePeerConnection as unknown as typeof RTCPeerConnection;
  (globalThis as unknown as { WebSocket: unknown }).WebSocket =
    FakeWebSocket as unknown as typeof WebSocket;
}

/** 等待所有挂起的 microtask（provider 内部 async 信令处理）。 */
const flush = () => new Promise<void>((r) => setTimeout(r, 0));

// 延迟导入：确保 vi.mock + 全局 fake 在 provider 求值前生效。
async function loadProvider() {
  return await import('./controllerCloudProvider');
}

const CONFIG = { userToken: 'user-jwt-abc', username: 'alice' };
const HOST_DEVICE = 'my-laptop';

describe('ControllerCloudProvider', () => {
  beforeEach(() => {
    FakePeerConnection.instances = [];
    FakeWebSocket.instances = [];
    installGlobals();
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('连信令时 URL 用 role=controller + room={hostDevice}-{username} + user token', async () => {
    const { ControllerCloudProvider } = await loadProvider();
    const provider = new ControllerCloudProvider(CONFIG);
    await provider.connect(HOST_DEVICE);
    await flush();

    expect(FakeWebSocket.instances).toHaveLength(1);
    const url = FakeWebSocket.instances[0].url;
    expect(url).toContain(`wss://my-laptop-alice.`);
    expect(url).toContain('role=controller');
    expect(url).toContain('token=user-jwt-abc');
  });

  it('信令 URL 携带 cli（契约 §5.3 同实例顶替）', async () => {
    const { ControllerCloudProvider } = await loadProvider();
    const provider = new ControllerCloudProvider(CONFIG);
    await provider.connect(HOST_DEVICE);
    await flush();
    const url = FakeWebSocket.instances[0].url;
    expect(url).toMatch(/[?&]role=controller/);
    expect(url).toMatch(/[?&]cli=[A-Za-z0-9._%-]+/);
  });

  it('controller=offerer：收 welcome(peerPresent) 后创建 ridge ordered DataChannel + 发 offer', async () => {
    const { ControllerCloudProvider } = await loadProvider();
    const provider = new ControllerCloudProvider(CONFIG);
    await provider.connect(HOST_DEVICE);
    await flush();

    const pc = FakePeerConnection.instances[0];
    // DataChannel 在 setupPeerConnection 时即主动创建（offerer）。
    expect(pc.channel).not.toBeNull();
    expect(pc.channel!.label).toBe('ridge');
    expect(pc.channel!.init?.ordered).toBe(true);

    // host 已在房 → welcome(peerPresent:true) → controller 发 offer。
    const ws = FakeWebSocket.instances[0];
    ws.deliver({ t: 'welcome', room: 'my-laptop-alice', role: 'controller', peerPresent: true });
    await flush();

    expect(pc.localDescription).toEqual({ type: 'offer', sdp: 'fake-offer-sdp' });
    const offerMsgs = ws.sentParsed().filter((m) => m.t === 'offer');
    expect(offerMsgs).toHaveLength(1);
    expect(offerMsgs[0].sdp).toBe('fake-offer-sdp');
  });

  it('收 peer-join(host) 后才发 offer；welcome(peerPresent:false) 不发；双触发只发一次', async () => {
    const { ControllerCloudProvider } = await loadProvider();
    const provider = new ControllerCloudProvider(CONFIG);
    await provider.connect(HOST_DEVICE);
    await flush();
    const ws = FakeWebSocket.instances[0];

    // host 未在房：不发 offer。
    ws.deliver({ t: 'welcome', room: 'my-laptop-alice', role: 'controller', peerPresent: false });
    await flush();
    expect(ws.sentParsed().filter((m) => m.t === 'offer')).toHaveLength(0);

    // host 进房 → 发 offer。
    ws.deliver({ t: 'peer-join', role: 'host' });
    await flush();
    // 再来一条（如重复/race）→ 幂等，仍只一条 offer。
    ws.deliver({ t: 'peer-join', role: 'host' });
    await flush();
    expect(ws.sentParsed().filter((m) => m.t === 'offer')).toHaveLength(1);
  });

  it('收 host answer → setRemoteDescription(answer)；trickle ICE 双向转发', async () => {
    const { ControllerCloudProvider } = await loadProvider();
    const provider = new ControllerCloudProvider(CONFIG);
    await provider.connect(HOST_DEVICE);
    await flush();
    const pc = FakePeerConnection.instances[0];
    const ws = FakeWebSocket.instances[0];

    ws.deliver({ t: 'peer-join', role: 'host' });
    await flush();

    // host 回 answer。
    ws.deliver({ t: 'answer', sdp: 'host-answer-sdp' });
    await flush();
    expect(pc.remoteDescription).toEqual({ type: 'answer', sdp: 'host-answer-sdp' });

    // 本端 ICE candidate → 经信令以 t:'ice' 转发。
    pc.onicecandidate?.({ candidate: { toJSON: () => ({ candidate: 'cand-1', sdpMid: '0' }) } });
    const iceOut = ws.sentParsed().filter((m) => m.t === 'ice');
    expect(iceOut).toHaveLength(1);
    expect((iceOut[0].candidate as RTCIceCandidateInit).candidate).toBe('cand-1');

    // 收对端 ICE → addIceCandidate。
    ws.deliver({ t: 'ice', candidate: { candidate: 'host-cand', sdpMid: '0' } });
    await flush();
    expect(pc.addedCandidates).toHaveLength(1);
    expect(pc.addedCandidates[0].candidate).toBe('host-cand');

    // ICE 收尾（candidate:null）不应入队（仅非空才 addIceCandidate）。
    ws.deliver({ t: 'ice', candidate: null });
    await flush();
    expect(pc.addedCandidates).toHaveLength(1);
  });

  it('状态机：disconnected → connecting → handshaking → connected', async () => {
    const { ControllerCloudProvider } = await loadProvider();
    const states: CloudConnectionState[] = [];
    const provider = new ControllerCloudProvider(CONFIG, { onState: (s) => states.push(s) });

    expect(provider.getState()).toBe('disconnected');
    await provider.connect(HOST_DEVICE);
    await flush();
    expect(provider.getState()).toBe('connecting');

    const pc = FakePeerConnection.instances[0];
    const dc = pc.channel!;
    // DataChannel open → handshaking + 发握手帧。
    dc.fireOpen();
    expect(provider.getState()).toBe('handshaking');

    // 完成握手（见专门的 E2EE 测试）后到 connected。
    completeHandshakeFromHost(dc);
    expect(provider.getState()).toBe('connected');

    expect(states).toEqual(['connecting', 'handshaking', 'connected']);
  });

  it('§7.1 E2EE：握手发 0x01||pub32；session 用 dir=1，与 host(dir=0) 互通', async () => {
    const { ControllerCloudProvider } = await loadProvider();
    const received: Uint8Array[] = [];
    const provider = new ControllerCloudProvider(CONFIG, { onFrame: (b) => received.push(b) });
    await provider.connect(HOST_DEVICE);
    await flush();

    const dc = FakePeerConnection.instances[0].channel!;
    dc.fireOpen();

    // 1) controller 首帧 = 0x01 || ephemeral_pub(32)。
    const handshakeFrame = dc.lastSent();
    expect(handshakeFrame[0]).toBe(0x01);
    expect(handshakeFrame.length).toBe(1 + 32);
    const ctrlPub = decodeHandshakeFrame(handshakeFrame);

    // 2) 模拟 host：用 dir=0 建 session，回自己的握手帧。
    const { hostSession } = handshakeAsHost(ctrlPub, dc);
    expect(provider.getState()).toBe('connected');

    // 3) host → controller（dir=0）：provider 先重组分片再 open，经 onFrame 上抛明文。
    const hostPlain = new TextEncoder().encode('\x11{"jsonrpc":"2.0","method":"$/hello"}');
    dc.deliver(wrapSingle(hostSession.seal(hostPlain)));
    expect(received).toHaveLength(1);
    expect(received[0]).toEqual(hostPlain);

    // 4) controller → host（dir=1）：sendFrame 加密+分片，host 重组后用同 key/dir 能解。
    const ctrlPlain = new TextEncoder().encode('\x11{"jsonrpc":"2.0","id":1,"method":"read_file"}');
    provider.sendFrame(ctrlPlain);
    const onWire = unwrapSingle(dc.lastSent()); // 剥掉传输层 SINGLE tag → 还原密文
    // 密文首字节 = nonce[0] = dir，controller 发出方向必须是 dir=1（DIR_CONTROLLER_TO_HOST）。
    expect(onWire[0]).toBe(1);
    expect(hostSession.open(onWire)).toEqual(ctrlPlain);
  });

  it('B3：信令公钥与握手公钥一致 → 绑定模式 enforced + connected', async () => {
    const { ControllerCloudProvider } = await loadProvider();
    const provider = new ControllerCloudProvider(CONFIG);
    await provider.connect(HOST_DEVICE);
    await flush();
    const dc = FakePeerConnection.instances[0].channel!;
    dc.fireOpen();
    const ctrlPub = decodeHandshakeFrame(dc.lastSent());
    handshakeAsHost(ctrlPub, dc); // 内部已发匹配的 e2ee-pubkey 信令
    expect(provider.getState()).toBe('connected');
    expect(provider.getKeyBindingMode()).toBe('enforced');
  });

  it('B3：信令公钥 ≠ 握手公钥（relay-MITM 调包）→ 判 MITM 拒绝断开', async () => {
    const { ControllerCloudProvider } = await loadProvider();
    let errCode: string | undefined;
    const provider = new ControllerCloudProvider(CONFIG, { onError: (_m, code) => (errCode = code) });
    await provider.connect(HOST_DEVICE);
    await flush();
    const dc = FakePeerConnection.instances[0].channel!;
    const ws = FakeWebSocket.instances[0];
    dc.fireOpen();
    const ctrlPub = decodeHandshakeFrame(dc.lastSent());

    // host 的真实 DataChannel 握手用 hostKp；但信令旁路上报一个【不同】的公钥
    // （模拟 relay 在 E2EE 腿给 controller 调包了攻击者公钥）。
    const hostKp = generateEphemeralKeyPair();
    const attackerPub = generateEphemeralKeyPair().publicKey;
    ws.deliver({ t: 'e2ee-pubkey', pubkey: bytesToBase64(attackerPub) });
    // controller 仍能用 hostKp 派生 session，但握手公钥(hostKp) ≠ 信令公钥(attacker) → 拒绝。
    void deriveSessionKey(hostKp.privateKey, hostKp.publicKey, ctrlPub);
    dc.deliver(encodeHandshakeFrame(hostKp.publicKey));

    expect(errCode).toBe('FORBIDDEN');
    expect(provider.getState()).toBe('disconnected');
    expect(provider.getKeyBindingMode()).not.toBe('enforced');
  });

  it('0x02 验签失败 → 经 E2EE 0x11 通道发 $/bye{signature-invalid}（不经 relay）+ 断开', async () => {
    const { ControllerCloudProvider } = await loadProvider();
    let errCode: string | undefined;
    const provider = new ControllerCloudProvider(CONFIG, { onError: (_m, code) => (errCode = code) });
    await provider.connect(HOST_DEVICE);
    await flush();
    const dc = FakePeerConnection.instances[0].channel!;
    dc.fireOpen();
    const ctrlPub = decodeHandshakeFrame(dc.lastSent());

    // 模拟 host：用真实临时密钥派生同一会话密钥（dir=0），但发一个**签名无效**的 0x02 帧
    //（id_pub 随意、sig 全 0）。controller 验签失败 → 判 MITM。
    const hostEph = generateEphemeralKeyPair();
    const hostKey = deriveSessionKey(hostEph.privateKey, hostEph.publicKey, ctrlPub);
    const hostSession = new E2eeSession(hostKey, DIR_HOST_TO_CONTROLLER);
    const badIdPub = new Uint8Array(32).fill(0x55);
    const badSig = new Uint8Array(64).fill(0x00);
    const sentBefore = dc.sent.length;
    dc.deliver(encodeSignedHandshakeFrame(hostEph.publicKey, badIdPub, badSig));

    // 断开 + FORBIDDEN。
    expect(errCode).toBe('FORBIDDEN');
    expect(provider.getState()).toBe('disconnected');

    // 断开前 controller 在 DataChannel 上多发了一帧：重组+解密 → 0x11 $/bye{signature-invalid}。
    expect(dc.sent.length).toBe(sentBefore + 1);
    const byePlain = hostSession.open(unwrapSingle(dc.lastSent()));
    const demuxed = demuxFrame(byePlain);
    expect(demuxed.kind).toBe('json');
    const bye = (demuxed as { kind: 'json'; json: Record<string, unknown> }).json;
    expect(bye.method).toBe('$/bye');
    expect((bye.params as { reason?: string }).reason).toBe(BYE_REASON_SIGNATURE_INVALID);
  });

  it('握手前 sendFrame 静默丢弃（不抛、不发）', async () => {
    const { ControllerCloudProvider } = await loadProvider();
    const provider = new ControllerCloudProvider(CONFIG);
    await provider.connect(HOST_DEVICE);
    await flush();
    const dc = FakePeerConnection.instances[0].channel!;
    dc.fireOpen(); // handshaking，但握手未完成
    const before = dc.sent.length; // 仅握手帧
    expect(() => provider.sendFrame(new Uint8Array([1, 2, 3]))).not.toThrow();
    expect(dc.sent.length).toBe(before); // 未额外发出
  });

  it('非握手首帧（首字节非 0x01）→ 握手失败 + error + 断开', async () => {
    const { ControllerCloudProvider } = await loadProvider();
    let errCode: string | undefined;
    const provider = new ControllerCloudProvider(CONFIG, {
      onError: (_m, code) => (errCode = code),
    });
    await provider.connect(HOST_DEVICE);
    await flush();
    const dc = FakePeerConnection.instances[0].channel!;
    dc.fireOpen();

    // host 发了一个非法首帧（首字节 0x10，不是握手 0x01）。
    dc.deliver(new Uint8Array([0x10, 0x00, 0x01]));
    expect(errCode).toBe('FORBIDDEN');
    expect(provider.getState()).toBe('disconnected'); // disconnect() 收尾
  });

  it('收到非法解密帧（握手后）→ 丢弃该帧但不断连', async () => {
    const { ControllerCloudProvider } = await loadProvider();
    const received: Uint8Array[] = [];
    let errCode: string | undefined;
    const provider = new ControllerCloudProvider(CONFIG, {
      onFrame: (b) => received.push(b),
      onError: (_m, code) => (errCode = code),
    });
    await provider.connect(HOST_DEVICE);
    await flush();
    const dc = FakePeerConnection.instances[0].channel!;
    dc.fireOpen();
    const ctrlPub = decodeHandshakeFrame(dc.lastSent());
    handshakeAsHost(ctrlPub, dc);
    expect(provider.getState()).toBe('connected');

    // 投递一个无法解密的"业务帧"（随机字节，tag/poly1305 不符）——须带传输层 SINGLE tag
    // 才会进到 open 失败路径（裸坏帧会在重组器层被丢弃，不触发 onError）。
    dc.deliver(wrapSingle(new Uint8Array(40).fill(7)));
    expect(errCode).toBe('FORBIDDEN');
    expect(received).toHaveLength(0);
    expect(provider.getState()).toBe('connected'); // 仍连着
  });

  it('信令 error 帧 → onError(code) + state=error', async () => {
    const { ControllerCloudProvider } = await loadProvider();
    let errCode: string | undefined;
    const provider = new ControllerCloudProvider(CONFIG, {
      onError: (_m, code) => (errCode = code),
    });
    await provider.connect(HOST_DEVICE);
    await flush();
    FakeWebSocket.instances[0].deliver({ t: 'error', code: 'REPLACED', message: '被新 controller 顶替' });
    await flush();
    expect(errCode).toBe('REPLACED');
    expect(provider.getState()).toBe('error');
  });

  it('connect 幂等：已连接中再次 connect 不新建 PeerConnection', async () => {
    const { ControllerCloudProvider } = await loadProvider();
    const provider = new ControllerCloudProvider(CONFIG);
    await provider.connect(HOST_DEVICE);
    await flush();
    expect(FakePeerConnection.instances).toHaveLength(1);
    await provider.connect(HOST_DEVICE); // connecting 中再次调用
    await flush();
    expect(FakePeerConnection.instances).toHaveLength(1);
  });

  it('disconnect 幂等清理：state=disconnected，关闭 pc/dc/ws', async () => {
    const { ControllerCloudProvider } = await loadProvider();
    const provider = new ControllerCloudProvider(CONFIG);
    await provider.connect(HOST_DEVICE);
    await flush();
    const pc = FakePeerConnection.instances[0];
    const ws = FakeWebSocket.instances[0];

    provider.disconnect();
    expect(provider.getState()).toBe('disconnected');
    expect(pc.connectionState).toBe('closed');
    expect(ws.readyState).toBe(3);
    expect(() => provider.disconnect()).not.toThrow(); // 幂等
  });
});

// ── 断线自动重连（弱网 P1）──────────────────────────────────────────────────────
//
// 验证 provider 断线后真正重连并重新 setState('connected')（=激活 L2 RpcClient 的
// onReconnected 重订阅 + 重连 reject 死代码路径），退避口径与 LAN 一致：
//   • RTC failed 且信令仍在 → ICE restart（同一 pc 重协商，不重建）。
//   • 信令断 + RTC 断 → 整体重建（新 pc + 新 ws）后恢复 connected。
//   • disconnect() 取消待定重连。
//   • 退避延迟落在 base(1s)~+30% 抖动之间。
describe('ControllerCloudProvider 断线自动重连 (P1)', () => {
  beforeEach(() => {
    FakePeerConnection.instances = [];
    FakeWebSocket.instances = [];
    installGlobals();
    vi.useFakeTimers();
  });
  afterEach(() => {
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  /** host（dir 无关）：经指定 ws 上报匹配公钥 + 经 dc 回握手帧，推进到 connected。 */
  function hostHandshake(dc: FakeDataChannel, ws: FakeWebSocket): void {
    const hostKp = generateEphemeralKeyPair();
    ws.deliver({ t: 'e2ee-pubkey', pubkey: bytesToBase64(hostKp.publicKey) });
    dc.deliver(encodeHandshakeFrame(hostKp.publicKey));
  }

  /** 连上并完成握手到 connected（直接 fireOpen + 握手，跳过 SDP 往返，与既有测试一致）。 */
  async function connectToConnected(onState?: (s: CloudConnectionState) => void) {
    const { ControllerCloudProvider } = await loadProvider();
    const provider = new ControllerCloudProvider(CONFIG, onState ? { onState } : {});
    await provider.connect(HOST_DEVICE);
    await vi.advanceTimersByTimeAsync(0);
    const pc = FakePeerConnection.instances.at(-1)!;
    const dc = pc.channel!;
    dc.fireOpen();
    hostHandshake(dc, FakeWebSocket.instances.at(-1)!);
    pc.connectionState = 'connected';
    pc.onconnectionstatechange?.();
    expect(provider.getState()).toBe('connected');
    return { provider, pc, dc };
  }

  it('RTC failed 且信令仍在 → ICE restart 同 pc 重协商，恢复 connected（不重建 pc）', async () => {
    const { provider, pc } = await connectToConnected();
    const ws = FakeWebSocket.instances[0];
    const offersBefore = ws.sentParsed().filter((m) => m.t === 'offer').length;

    // RTC 失败，信令 WS 仍 OPEN。
    pc.connectionState = 'failed';
    pc.onconnectionstatechange?.();
    expect(provider.getState()).toBe('disconnected'); // connected→disconnected：L2 reject 边沿

    // 退避（≤1.3s）后重连 → ICE restart：同一 pc 发新 offer，未重建 PeerConnection。
    await vi.advanceTimersByTimeAsync(1400);
    expect(FakePeerConnection.instances).toHaveLength(1);
    expect(ws.sentParsed().filter((m) => m.t === 'offer').length).toBe(offersBefore + 1);
    expect(provider.getState()).toBe('connecting');

    // ICE 恢复 → connected（E2EE 会话存活，无需重握手）。
    pc.connectionState = 'connected';
    pc.onconnectionstatechange?.();
    expect(provider.getState()).toBe('connected');
  });

  it('信令断 + RTC 失败 → 整体重建（新 pc + 新 ws）后恢复 connected', async () => {
    const { provider, pc } = await connectToConnected();
    // 信令先断（readyState=CLOSED），再 RTC 失败。
    FakeWebSocket.instances[0].close();
    pc.connectionState = 'failed';
    pc.onconnectionstatechange?.();
    expect(provider.getState()).toBe('disconnected');

    await vi.advanceTimersByTimeAsync(1400);
    // 整体重建：新 pc + 新 ws（复用缓存 iceServers，无需再取）。
    expect(FakePeerConnection.instances).toHaveLength(2);
    expect(FakeWebSocket.instances).toHaveLength(2);

    // 驱动新连接握手 → 恢复 connected。
    const pc2 = FakePeerConnection.instances[1];
    const ws2 = FakeWebSocket.instances[1];
    const dc2 = pc2.channel!;
    dc2.fireOpen();
    hostHandshake(dc2, ws2);
    pc2.connectionState = 'connected';
    pc2.onconnectionstatechange?.();
    expect(provider.getState()).toBe('connected');
  });

  it('disconnect() 取消待定重连，不再重建', async () => {
    const { provider, pc } = await connectToConnected();
    pc.connectionState = 'failed';
    pc.onconnectionstatechange?.();
    expect(provider.getState()).toBe('disconnected');

    provider.disconnect();
    await vi.advanceTimersByTimeAsync(5000);
    expect(FakePeerConnection.instances).toHaveLength(1); // 未重建
    expect(provider.getState()).toBe('disconnected');
  });

  it('退避首次重连延迟落在 base(1s)~+30% 抖动之间', async () => {
    const { provider, pc } = await connectToConnected();
    pc.connectionState = 'failed';
    pc.onconnectionstatechange?.();

    // < base：尚未重连。
    await vi.advanceTimersByTimeAsync(900);
    expect(provider.getState()).toBe('disconnected');

    // 到 1350ms（base + 30% 抖动上界）必已重连（ICE restart → connecting）。
    await vi.advanceTimersByTimeAsync(450);
    expect(provider.getState()).not.toBe('disconnected');
  });
});

// ── 测试辅助：模拟 host（dir=0）完成握手 + 派生同一 key ──────────────────────────

/**
 * 给定 controller 发出的临时公钥与 DataChannel，模拟 host：
 *   - 生成自己的临时密钥对，回 0x01||hostPub 握手帧给 controller；
 *   - 用 dir=0 派生 hostSession（与 controller 的 dir=1 session 同 key、相反方向）。
 */
function handshakeAsHost(
  ctrlPub: Uint8Array,
  dc: FakeDataChannel,
): { hostSession: E2eeSession; hostKp: EphemeralKeyPair } {
  const hostKp = generateEphemeralKeyPair();
  const hostKey = deriveSessionKey(hostKp.privateKey, hostKp.publicKey, ctrlPub);
  const hostSession = new E2eeSession(hostKey, DIR_HOST_TO_CONTROLLER);
  // B3：host 先经**信令旁路**上报与其 DataChannel 握手公钥一致的临时公钥；controller
  // 比对一致 → enforced → connected。（不发此信令则 controller 会等到宽限期才回落。）
  FakeWebSocket.instances[0]?.deliver({ t: 'e2ee-pubkey', pubkey: bytesToBase64(hostKp.publicKey) });
  // host 把自己的握手帧投递给 controller → controller 派生 key、比对绑定、置 connected。
  dc.deliver(encodeHandshakeFrame(hostKp.publicKey));
  return { hostSession, hostKp };
}

/** 便捷：从 DataChannel 的 controller 握手帧推进到 connected（丢弃 hostSession）。 */
function completeHandshakeFromHost(dc: FakeDataChannel): void {
  const ctrlPub = decodeHandshakeFrame(dc.lastSent());
  handshakeAsHost(ctrlPub, dc);
}
