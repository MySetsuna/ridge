// Ridge Cloud — 桌面 host 端 WebRTC + E2EE provider（契约 §5 信令、§5.3 多控制方、§7 E2EE）。
//
// 角色：桌面端是 **host = answerer**（契约 §5.1）。controller（浏览器）是 offerer。
//
// 多控制方（契约 §5.3）：host **一条**信令 WS（role=host）即可被 **N 个 controller**
// 同时接入。relay 给每条 controller 连接分配房间内唯一 `cid`，并：
//   - 把 controller→host 的 offer/ice **加盖 cid** 转发给 host；
//   - 把 host→controller 的 answer/ice 按 host 指定的 `cid` 定向投递。
// 因此 host 端按 cid 各自维护一个 RTCPeerConnection + 独立 E2EE 会话 + 独立应用层桥
// （CloudHostBridge，一连接一实例；pane 输出各自订阅，等价于 LAN 的 N 个独立会话）。
//
// 连接流程（每个 cid）：
//   1. 收 peer-join{role:controller,cid}（或直接收 offer{cid}）→ 准备该 cid 的会话槽。
//   2. 收 offer{sdp,cid} → setRemoteDescription → createAnswer → 发 answer{sdp,cid}；双向 trickle ICE。
//   3. controller 创建 DataChannel(label="ridge") → host ondatachannel。
//   4. DataChannel open 后跑 §7 E2EE 握手（首两条二进制消息 0x01||pub32）。
//   5. 握手完成 → 该 cid 标记 connected，向上层取一个 bridge 接管明文帧。
//
// ⚠️ v1 scaffold（契约 §8）：host WebRTC 跑在 WebView/TS。终态需迁到 Rust(webrtc-rs)。

import {
  type CloudConnectionState,
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
import { BASE_DOMAIN, cloudWsScheme } from './apiClient';

/** DataChannel 标签与参数（契约 §7）。 */
const DC_LABEL = 'ridge';

/** host 端信令 WS 在线状态（与 per-controller 的 CloudConnectionState 区分）。 */
export type HostSignalState = 'offline' | 'connecting' | 'online' | 'error';

/** 一个 controller 会话对上层（CloudPanel）暴露的只读视图。 */
export interface CloudControllerSession {
  /** relay 分配的房间内唯一 controller id。 */
  cid: string;
  /** 该 controller 的 RTC/E2EE 阶段状态。 */
  state: CloudConnectionState;
  /** 首次出现（peer-join / offer）的本地时间戳（ms）。 */
  connectedAt: number;
}

/** 上层注入的应用层桥（CloudHostBridge 结构子集，便于解耦/测试）。 */
export interface CloudHostBridgeLike {
  handleFrame(plaintext: Uint8Array): void;
  verifyPeerKey?(peerPublicKey: Uint8Array): boolean;
  reset(): void;
}

/** 信令消息（契约 §5.1 + §5.3，tag 字段 `t`）。host 端视角。 */
type SignalIn =
  | { t: 'welcome'; room: string; role: 'host' | 'controller'; peerPresent: boolean; cid?: string }
  | { t: 'peer-join'; role: 'host' | 'controller'; cid?: string }
  | { t: 'peer-leave'; role: 'host' | 'controller'; cid?: string }
  | { t: 'error'; code: string; message: string }
  | { t: 'offer'; sdp: string; cid?: string }
  | { t: 'answer'; sdp: string; cid?: string }
  | { t: 'ice'; candidate: RTCIceCandidateInit | null; cid?: string };

export interface RidgeCloudHostConfig {
  /** device JWT（scope=device），WS 与 ice-servers 鉴权用。 */
  deviceToken: string;
  /** username（host label 拼接用，契约 §1）。 */
  username: string;
  /** Base zone，默认 BASE_DOMAIN，集中可改。 */
  baseDomain?: string;
}

export interface RidgeCloudHostCallbacks {
  /** 信令 WS 在线状态变化（驱动「公网远控已启用/启用中」展示）。 */
  onHostState?: (state: HostSignalState) => void;
  /** 已接入 controller 列表变化（驱动控制方列表 UI）。 */
  onSessions?: (sessions: CloudControllerSession[]) => void;
  /** 出错（结构化 code + 人类可读信息）。 */
  onError?: (message: string, code?: string) => void;
  /**
   * 为某个 controller 会话创建应用层桥。E2EE 握手完成后由 provider 调用一次。
   * @param cid  该 controller 的 id。
   * @param send 把一帧明文加密经**该 controller** 的 DataChannel 发回（provider 提供）。
   */
  createBridge: (cid: string, send: (plaintext: Uint8Array) => void) => CloudHostBridgeLike;
}

/** 单个 controller 连接的内部状态。 */
interface ControllerConn {
  cid: string;
  pc: RTCPeerConnection;
  dc: RTCDataChannel | null;
  ephemeral: EphemeralKeyPair | null;
  session: E2eeSession | null;
  handshakeDone: boolean;
  bridge: CloudHostBridgeLike | null;
  state: CloudConnectionState;
  connectedAt: number;
}

/**
 * 桌面 host 端云端连接管理器（多控制方）。
 *
 * 生命周期：`goOnline(deviceName)` 连信令 WS 上线 → controller 来去自如 →
 * `goOffline()` 幂等清理全部资源。`kick(cid)` / `blacklist(cid)` 管理单个 controller。
 */
export class RidgeCloudHost {
  private readonly config: Required<RidgeCloudHostConfig>;
  private readonly cb: RidgeCloudHostCallbacks;

  private ws: WebSocket | null = null;
  private hostState: HostSignalState = 'offline';
  private closed = true;
  private iceServers: IceServer[] = [];

  /** cid → 连接。 */
  private readonly conns = new Map<string, ControllerConn>();
  /** 已拉黑的 cid（会话级；命中即拒绝其 offer 并 kick）。 */
  private readonly banned = new Set<string>();

  constructor(config: RidgeCloudHostConfig, callbacks: RidgeCloudHostCallbacks) {
    this.config = {
      deviceToken: config.deviceToken,
      username: config.username,
      baseDomain: config.baseDomain ?? BASE_DOMAIN,
    };
    this.cb = callbacks;
  }

  getHostState(): HostSignalState {
    return this.hostState;
  }

  getSessions(): CloudControllerSession[] {
    return [...this.conns.values()].map((c) => ({
      cid: c.cid,
      state: c.state,
      connectedAt: c.connectedAt,
    }));
  }

  private setHostState(s: HostSignalState): void {
    if (this.hostState === s) return;
    this.hostState = s;
    this.cb.onHostState?.(s);
  }

  private emitSessions(): void {
    this.cb.onSessions?.(this.getSessions());
  }

  private fail(message: string, code?: string): void {
    this.cb.onError?.(message, code);
  }

  /** host label：{device}-{username}（契约 §1.1）。 */
  private hostLabel(deviceId: string): string {
    return `${deviceId}-${this.config.username}`;
  }

  // ─── 上线 / 下线 ────────────────────────────────────────────────────────────

  async goOnline(deviceId: string): Promise<void> {
    if (this.hostState === 'connecting' || this.hostState === 'online') return; // 幂等
    this.closed = false;
    this.setHostState('connecting');

    // 取 ICE servers（契约 §5.2：必须调接口，不硬编码 STUN）。建 PC 时复用。
    try {
      const res = await getIceServers(this.config.deviceToken);
      this.iceServers = res.iceServers ?? [];
    } catch (e: unknown) {
      this.fail(e instanceof Error ? e.message : '获取 ICE 服务器失败', 'NETWORK');
      this.setHostState('error');
      return;
    }
    if (this.closed) return;

    this.openSignaling(deviceId);
  }

  goOffline(): void {
    this.closed = true;
    for (const cid of [...this.conns.keys()]) this.teardownConn(cid, false);
    this.conns.clear();
    if (this.ws) {
      this.ws.onmessage = this.ws.onerror = this.ws.onclose = null;
      try { this.ws.close(); } catch { /* ignore */ }
      this.ws = null;
    }
    this.setHostState('offline');
    this.emitSessions();
  }

  /** 主动断开某 controller：通知 relay kick + 本地拆除。 */
  kick(cid: string): void {
    this.sendSignal({ t: 'kick', cid });
    this.teardownConn(cid, true);
  }

  /** 拉黑某 controller：加入会话级黑名单并 kick（后续该 cid 的 offer 一律拒绝）。 */
  blacklist(cid: string): void {
    this.banned.add(cid);
    this.kick(cid);
  }

  isBanned(cid: string): boolean {
    return this.banned.has(cid);
  }

  // ─── 单连接管理 ────────────────────────────────────────────────────────────

  /** 取/建某 cid 的连接槽（建时创建 RTCPeerConnection 并装好回调）。 */
  private ensureConn(cid: string): ControllerConn {
    let conn = this.conns.get(cid);
    if (conn) return conn;

    const pc = new RTCPeerConnection({ iceServers: this.iceServers as RTCIceServer[] });
    conn = {
      cid,
      pc,
      dc: null,
      ephemeral: null,
      session: null,
      handshakeDone: false,
      bridge: null,
      state: 'connecting',
      connectedAt: Date.now(),
    };
    this.conns.set(cid, conn);

    // 本端 ICE candidate → 经信令**定向**转发给该 controller（§5.3 带 cid）。
    pc.onicecandidate = (ev) => {
      this.sendSignal({ t: 'ice', candidate: ev.candidate ? ev.candidate.toJSON() : null, cid });
    };
    pc.onconnectionstatechange = () => {
      const cs = pc.connectionState;
      if (cs === 'failed' || cs === 'closed') {
        if (!this.closed) this.teardownConn(cid, false);
      }
      // 'disconnected' 短暂抖动可能自愈，不立刻拆。
    };
    // host 是 answerer：DataChannel 由 controller 创建 → 被动接收。
    pc.ondatachannel = (ev) => {
      if (ev.channel.label !== DC_LABEL) return;
      this.attachDataChannel(conn!, ev.channel);
    };
    this.emitSessions();
    return conn;
  }

  private setConnState(conn: ControllerConn, s: CloudConnectionState): void {
    if (conn.state === s) return;
    conn.state = s;
    this.emitSessions();
  }

  private attachDataChannel(conn: ControllerConn, dc: RTCDataChannel): void {
    conn.dc = dc;
    dc.binaryType = 'arraybuffer';
    dc.onopen = () => {
      this.setConnState(conn, 'handshaking');
      this.startE2eeHandshake(conn);
    };
    dc.onclose = () => {
      if (!this.closed) this.teardownConn(conn.cid, false);
    };
    dc.onmessage = (ev) => {
      this.onDataChannelMessage(conn, ev.data);
    };
  }

  /** 发起 E2EE 握手：生成临时密钥对并发送 0x01||pub32（契约 §7.1）。 */
  private startE2eeHandshake(conn: ControllerConn): void {
    conn.ephemeral = generateEphemeralKeyPair();
    conn.handshakeDone = false;
    conn.session = null;
    this.rawSend(conn, encodeHandshakeFrame(conn.ephemeral.publicKey));
  }

  private onDataChannelMessage(conn: ControllerConn, data: unknown): void {
    const bytes = toBytes(data);
    if (!bytes) return;

    // 握手完成前，首帧必须是对端握手帧；否则断开（契约 §7.1）。
    if (!conn.handshakeDone) {
      try {
        const peerPub = decodeHandshakeFrame(bytes);
        if (!conn.ephemeral) throw new Error('本端临时密钥缺失');
        const key = deriveSessionKey(conn.ephemeral.privateKey, conn.ephemeral.publicKey, peerPub);
        // host 端发出方向为 host→controller(dir=0)。
        conn.session = new E2eeSession(key, DIR_HOST_TO_CONTROLLER);
        conn.handshakeDone = true;
        conn.ephemeral = null; // 用完即焚
        // 取应用层桥接管该 cid 的明文帧；sendFrame 闭包加密经本连接发回。
        const bridge = this.cb.createBridge(conn.cid, (plaintext) => this.sendFrame(conn, plaintext));
        // §5.5 公钥↔身份绑定（桥若注入了校验器）。不过即拒会话。
        if (bridge.verifyPeerKey && !bridge.verifyPeerKey(peerPub)) {
          conn.bridge = bridge;
          this.teardownConn(conn.cid, true);
          return;
        }
        conn.bridge = bridge;
        this.setConnState(conn, 'connected');
      } catch (e: unknown) {
        this.fail(e instanceof Error ? e.message : 'E2EE 握手失败，已断开', 'FORBIDDEN');
        this.teardownConn(conn.cid, false);
      }
      return;
    }

    // 业务帧：解密后交给该 cid 的桥。
    if (!conn.session || !conn.bridge) return;
    try {
      const plaintext = conn.session.open(bytes);
      conn.bridge.handleFrame(plaintext);
    } catch (e: unknown) {
      this.fail(e instanceof Error ? e.message : '收到无法解密的帧（已丢弃）', 'FORBIDDEN');
    }
  }

  /** 把一帧明文加密经某连接的 DataChannel 发回 controller。 */
  private sendFrame(conn: ControllerConn, plaintext: Uint8Array): void {
    if (conn.state !== 'connected' || !conn.session) return; // 握手前静默丢弃
    try {
      this.rawSend(conn, conn.session.seal(plaintext));
    } catch (e: unknown) {
      this.fail(e instanceof Error ? e.message : '加密发送失败', 'INTERNAL');
    }
  }

  private rawSend(conn: ControllerConn, bytes: Uint8Array): void {
    if (conn.dc && conn.dc.readyState === 'open') {
      conn.dc.send(
        bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength) as ArrayBuffer,
      );
    }
  }

  /** 拆除某 cid 的连接并释放资源。`notify` 仅控制是否触发 onSessions（批量清理时关）。 */
  private teardownConn(cid: string, notify: boolean): void {
    const conn = this.conns.get(cid);
    if (!conn) return;
    this.conns.delete(cid);
    try { conn.bridge?.reset(); } catch { /* ignore */ }
    if (conn.dc) {
      conn.dc.onopen = conn.dc.onclose = conn.dc.onmessage = null;
      try { conn.dc.close(); } catch { /* ignore */ }
    }
    conn.pc.onicecandidate = conn.pc.ondatachannel = conn.pc.onconnectionstatechange = null;
    try { conn.pc.close(); } catch { /* ignore */ }
    conn.ephemeral = null;
    conn.session = null;
    if (notify) this.emitSessions();
  }

  // ─── 信令 WS（契约 §3：WS 用 query ?token=&role=）──────────────────────────

  private openSignaling(deviceId: string): void {
    const label = this.hostLabel(deviceId);
    const url =
      `${cloudWsScheme(this.config.baseDomain)}://${label}.${this.config.baseDomain}/ws` +
      `?token=${encodeURIComponent(this.config.deviceToken)}&role=host`;

    let ws: WebSocket;
    try {
      ws = new WebSocket(url);
    } catch (e: unknown) {
      this.fail(e instanceof Error ? e.message : '信令连接失败', 'NETWORK');
      this.setHostState('error');
      return;
    }
    this.ws = ws;

    ws.onmessage = (ev) => { void this.onSignal(ev.data); };
    ws.onerror = () => {
      if (!this.closed) { this.fail('信令 WebSocket 错误', 'NETWORK'); this.setHostState('error'); }
    };
    ws.onclose = () => {
      if (!this.closed) {
        // 信令断开：host 视为离线（已连通的 RTC 在 relay 下线后仍可短暂存活，但
        // 无信令则无法接入新 controller / 续 ICE，统一标记 error 让 UI 提示重连）。
        this.setHostState('error');
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
      return;
    }

    switch (msg.t) {
      case 'welcome':
        // host welcome：标记上线。已有 controller 由后续 peer-join 各自补发枚举。
        this.setHostState('online');
        break;
      case 'peer-join':
        // controller 入房：登记会话槽（offer 到达前先展示「连接中」）。
        if (msg.role === 'controller' && msg.cid) {
          if (this.banned.has(msg.cid)) { this.sendSignal({ t: 'kick', cid: msg.cid }); break; }
          this.ensureConn(msg.cid);
        }
        break;
      case 'peer-leave':
        if (msg.role === 'controller' && msg.cid) this.teardownConn(msg.cid, true);
        break;
      case 'offer':
        if (msg.cid) await this.onOffer(msg.cid, msg.sdp);
        break;
      case 'answer':
        // host 是 answerer，不应收到 answer。忽略。
        break;
      case 'ice':
        if (msg.cid && msg.candidate) {
          const conn = this.conns.get(msg.cid);
          if (conn) {
            try { await conn.pc.addIceCandidate(msg.candidate); } catch { /* 非关键 candidate 失败可忽略 */ }
          }
        }
        break;
      case 'error':
        this.fail(msg.message || '信令错误', msg.code);
        break;
    }
  }

  private async onOffer(cid: string, sdp: string): Promise<void> {
    if (this.banned.has(cid)) { this.sendSignal({ t: 'kick', cid }); return; }
    const conn = this.ensureConn(cid);
    try {
      await conn.pc.setRemoteDescription({ type: 'offer', sdp });
      const answer = await conn.pc.createAnswer();
      await conn.pc.setLocalDescription(answer);
      this.sendSignal({ t: 'answer', sdp: answer.sdp, cid });
    } catch (e: unknown) {
      this.fail(e instanceof Error ? e.message : '处理 offer 失败', 'INTERNAL');
      this.teardownConn(cid, true);
    }
  }
}

/** 把 DataChannel message data（ArrayBuffer / ArrayBufferView）转为 Uint8Array。 */
function toBytes(data: unknown): Uint8Array | null {
  if (data instanceof ArrayBuffer) return new Uint8Array(data);
  if (ArrayBuffer.isView(data)) {
    const v = data as ArrayBufferView;
    return new Uint8Array(v.buffer, v.byteOffset, v.byteLength);
  }
  return null;
}
