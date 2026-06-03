// Ridge Cloud — 桌面 host 端 WebRTC + E2EE provider（契约 §5 信令、§7 E2EE）。
//
// 角色：桌面端是 **host = answerer**（契约 §5.1）。controller（浏览器）才是
// offer 发起方。host 连上信令 WS 后等待 controller 的 offer，回 answer。
//
// 连接流程：
//   1. GET /api/v1/ice-servers(Bearer device) 取 iceServers（§5.2，不硬编码 STUN）。
//   2. 连 wss://{device}-{username}.{BASE_DOMAIN}/ws?token=<deviceJWT>&role=host（§1/§3）。
//   3. 收 welcome / peer-join；controller 发 offer → host setRemoteDescription →
//      createAnswer → 发 answer；双向 trickle ICE（§5.1）。
//   4. controller 创建 DataChannel(label="ridge", ordered) → host 端 ondatachannel。
//   5. DataChannel open 后跑 §7 E2EE 握手（首两条二进制消息：0x01||pub32）。
//   6. 握手完成后，对现有 postcard 帧加密收发（seal/open）。
//
// ⚠️ v1 scaffold（契约 §8）：本 provider 运行在 WebView/TS。Deep Root Mode 用
// window.hide()（隐藏不销毁），连接活在隐藏 WebView 里。终态需把 host WebRTC
// 迁到 Rust(webrtc-rs) 才能 destroy WebView 仍保活——那是 Agent 3 的后续工作。

import {
  type RemoteConnectionProvider,
  type CloudConnectionState,
  type CloudConnectionCallbacks,
} from './connectionProvider';
import {
  E2eeSession,
  DIR_HOST_TO_CONTROLLER,
  generateEphemeralKeyPair,
  encodeHandshakeFrame,
  decodeHandshakeFrame,
  deriveSessionKey,
  type EphemeralKeyPair,
} from './e2ee';
import { getIceServers, type IceServer } from './apiClient';
import { BASE_DOMAIN } from './apiClient';

/** DataChannel 标签与参数（契约 §7）。 */
const DC_LABEL = 'ridge';

/** 信令消息（契约 §5.1，tag 字段 `t`）。 */
type SignalIn =
  | { t: 'welcome'; room: string; role: 'host' | 'controller'; peerPresent: boolean }
  | { t: 'peer-join'; role: 'host' | 'controller' }
  | { t: 'peer-leave'; role: 'host' | 'controller' }
  | { t: 'error'; code: string; message: string }
  | { t: 'offer'; sdp: string }
  | { t: 'answer'; sdp: string }
  | { t: 'ice'; candidate: RTCIceCandidateInit | null };

export interface RidgeCloudProviderConfig {
  /** device JWT（scope=device），WS 与 ice-servers 鉴权用。 */
  deviceToken: string;
  /** username（host label 拼接用，契约 §1）。 */
  username: string;
  /** Base zone，默认 BASE_DOMAIN，集中可改。 */
  baseDomain?: string;
}

/**
 * 桌面 host 端云端连接 provider。
 *
 * 单连接对象；一次 connect 对应一个 controller 会话。disconnect 幂等清理。
 */
export class RidgeCloudProvider implements RemoteConnectionProvider {
  private readonly config: Required<RidgeCloudProviderConfig>;
  private readonly cb: CloudConnectionCallbacks;

  private ws: WebSocket | null = null;
  private pc: RTCPeerConnection | null = null;
  private dc: RTCDataChannel | null = null;

  private ephemeral: EphemeralKeyPair | null = null;
  private session: E2eeSession | null = null;
  private handshakeDone = false;

  private state: CloudConnectionState = 'disconnected';
  private closed = false;
  private deviceId = '';

  constructor(config: RidgeCloudProviderConfig, callbacks: CloudConnectionCallbacks = {}) {
    this.config = {
      deviceToken: config.deviceToken,
      username: config.username,
      baseDomain: config.baseDomain ?? BASE_DOMAIN,
    };
    this.cb = callbacks;
  }

  getState(): CloudConnectionState {
    return this.state;
  }

  private setState(s: CloudConnectionState): void {
    if (this.state === s) return;
    this.state = s;
    this.cb.onState?.(s);
  }

  private fail(message: string, code?: string): void {
    this.cb.onError?.(message, code);
    this.setState('error');
  }

  /** host label：{device}-{username}（契约 §1.1）。 */
  private hostLabel(deviceId: string): string {
    return `${deviceId}-${this.config.username}`;
  }

  async connect(deviceId: string): Promise<void> {
    if (this.state === 'connecting' || this.state === 'handshaking' || this.state === 'connected') {
      return; // 已在连接中，幂等
    }
    this.closed = false;
    this.deviceId = deviceId;
    this.setState('connecting');

    // 1. 取 ICE servers（契约 §5.2：必须调接口，不硬编码 STUN）。
    let iceServers: IceServer[];
    try {
      const res = await getIceServers(this.config.deviceToken);
      iceServers = res.iceServers ?? [];
    } catch (e: unknown) {
      this.fail(e instanceof Error ? e.message : '获取 ICE 服务器失败', 'NETWORK');
      return;
    }
    if (this.closed) return;

    this.setupPeerConnection(iceServers);
    this.openSignaling(deviceId);
  }

  private setupPeerConnection(iceServers: IceServer[]): void {
    const pc = new RTCPeerConnection({ iceServers: iceServers as RTCIceServer[] });
    this.pc = pc;

    // 本端 ICE candidate → 经信令转发给对端（§5.1 trickle）。
    pc.onicecandidate = (ev) => {
      this.sendSignal({ t: 'ice', candidate: ev.candidate ? ev.candidate.toJSON() : null });
    };

    pc.onconnectionstatechange = () => {
      const cs = pc.connectionState;
      if (cs === 'failed' || cs === 'closed') {
        if (!this.closed) this.fail('WebRTC 连接中断', 'NETWORK');
      } else if (cs === 'disconnected') {
        // 短暂抖动可能自愈，不立刻判失败。
      }
    };

    // host 是 answerer，DataChannel 由 controller 创建 → 这里被动接收。
    pc.ondatachannel = (ev) => {
      if (ev.channel.label !== DC_LABEL) return;
      this.attachDataChannel(ev.channel);
    };
  }

  private attachDataChannel(dc: RTCDataChannel): void {
    this.dc = dc;
    dc.binaryType = 'arraybuffer';

    dc.onopen = () => {
      this.setState('handshaking');
      this.startE2eeHandshake();
    };
    dc.onclose = () => {
      if (!this.closed) this.fail('数据通道已关闭', 'NETWORK');
    };
    dc.onmessage = (ev) => {
      this.onDataChannelMessage(ev.data);
    };
  }

  /** 发起 E2EE 握手：本端生成临时密钥对并发送 0x01||pub32（契约 §7.1）。 */
  private startE2eeHandshake(): void {
    this.ephemeral = generateEphemeralKeyPair();
    this.handshakeDone = false;
    this.session = null;
    this.rawSend(encodeHandshakeFrame(this.ephemeral.publicKey));
  }

  private onDataChannelMessage(data: unknown): void {
    const bytes = toBytes(data);
    if (!bytes) return;

    // 握手完成前，首帧必须是对端握手帧；否则断开（契约 §7.1）。
    if (!this.handshakeDone) {
      try {
        const peerPub = decodeHandshakeFrame(bytes);
        if (!this.ephemeral) throw new Error('本端临时密钥缺失');
        const key = deriveSessionKey(this.ephemeral.privateKey, this.ephemeral.publicKey, peerPub);
        // host 端发出方向为 host→controller(dir=0)。
        this.session = new E2eeSession(key, DIR_HOST_TO_CONTROLLER);
        this.handshakeDone = true;
        // 握手用完即焚临时私钥引用。
        this.ephemeral = null;
        this.setState('connected');
      } catch (e: unknown) {
        this.fail(e instanceof Error ? e.message : 'E2EE 握手失败，已断开', 'FORBIDDEN');
        this.disconnect();
      }
      return;
    }

    // 业务帧：解密后上抛明文 postcard 字节。
    if (!this.session) return;
    try {
      const plaintext = this.session.open(bytes);
      this.cb.onFrame?.(plaintext);
    } catch (e: unknown) {
      // 解密/重放失败：丢弃该帧但不一定断连（契约要求拒绝该帧）。
      this.cb.onError?.(e instanceof Error ? e.message : '收到无法解密的帧（已丢弃）', 'FORBIDDEN');
    }
  }

  sendFrame(plaintext: Uint8Array): void {
    if (this.state !== 'connected' || !this.session) return; // 握手前静默丢弃
    try {
      this.rawSend(this.session.seal(plaintext));
    } catch (e: unknown) {
      // counter 接近上限等 → 触发重建。
      this.fail(e instanceof Error ? e.message : '加密发送失败', 'INTERNAL');
    }
  }

  private rawSend(bytes: Uint8Array): void {
    if (this.dc && this.dc.readyState === 'open') {
      // 拷贝出独立 ArrayBuffer，避免发送共享底层 buffer 的视图。
      this.dc.send(bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength) as ArrayBuffer);
    }
  }

  // ─── 信令 WS（契约 §3：WS 用 query ?token=&role=）──────────────────────────

  private openSignaling(deviceId: string): void {
    const label = this.hostLabel(deviceId);
    const url =
      `wss://${label}.${this.config.baseDomain}/ws` +
      `?token=${encodeURIComponent(this.config.deviceToken)}&role=host`;

    let ws: WebSocket;
    try {
      ws = new WebSocket(url);
    } catch (e: unknown) {
      this.fail(e instanceof Error ? e.message : '信令连接失败', 'NETWORK');
      return;
    }
    this.ws = ws;

    ws.onmessage = (ev) => {
      void this.onSignal(ev.data);
    };
    ws.onerror = () => {
      if (!this.closed) this.fail('信令 WebSocket 错误', 'NETWORK');
    };
    ws.onclose = () => {
      // 信令断开不一定意味着 RTC 断（已连通后 relay 可下线），仅在尚未建立 RTC 时判失败。
      if (!this.closed && this.state === 'connecting') {
        this.fail('信令连接已关闭', 'NETWORK');
      }
    };
  }

  private sendSignal(msg: Record<string, unknown>): void {
    if (this.ws && this.ws.readyState === WebSocket.OPEN) {
      this.ws.send(JSON.stringify(msg));
    }
  }

  private async onSignal(raw: unknown): Promise<void> {
    let msg: SignalIn;
    try {
      msg = JSON.parse(typeof raw === 'string' ? raw : '') as SignalIn;
    } catch {
      return; // 非文本/非法 JSON 忽略
    }
    const pc = this.pc;
    if (!pc) return;

    switch (msg.t) {
      case 'welcome':
      case 'peer-join':
      case 'peer-leave':
        // host 是 answerer，被动等待 controller offer，无需在此动作。
        break;
      case 'offer':
        try {
          await pc.setRemoteDescription({ type: 'offer', sdp: msg.sdp });
          const answer = await pc.createAnswer();
          await pc.setLocalDescription(answer);
          this.sendSignal({ t: 'answer', sdp: answer.sdp });
        } catch (e: unknown) {
          this.fail(e instanceof Error ? e.message : '处理 offer 失败', 'INTERNAL');
        }
        break;
      case 'answer':
        // host 不应收到 answer（host 是 answerer）。忽略。
        break;
      case 'ice':
        if (msg.candidate) {
          try {
            await pc.addIceCandidate(msg.candidate);
          } catch {
            /* 无关键 candidate 失败可忽略 */
          }
        }
        break;
      case 'error':
        this.fail(msg.message || '信令错误', msg.code);
        break;
    }
  }

  disconnect(): void {
    this.closed = true;
    if (this.dc) {
      this.dc.onopen = this.dc.onclose = this.dc.onmessage = null;
      try { this.dc.close(); } catch { /* ignore */ }
      this.dc = null;
    }
    if (this.pc) {
      this.pc.onicecandidate = this.pc.ondatachannel = this.pc.onconnectionstatechange = null;
      try { this.pc.close(); } catch { /* ignore */ }
      this.pc = null;
    }
    if (this.ws) {
      this.ws.onmessage = this.ws.onerror = this.ws.onclose = null;
      try { this.ws.close(); } catch { /* ignore */ }
      this.ws = null;
    }
    this.ephemeral = null;
    this.session = null;
    this.handshakeDone = false;
    this.setState('disconnected');
  }
}

/** 把 DataChannel message data（ArrayBuffer / Blob 暂不支持）转为 Uint8Array。 */
function toBytes(data: unknown): Uint8Array | null {
  if (data instanceof ArrayBuffer) return new Uint8Array(data);
  if (ArrayBuffer.isView(data)) {
    const v = data as ArrayBufferView;
    return new Uint8Array(v.buffer, v.byteOffset, v.byteLength);
  }
  return null;
}
