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
  encodeSignedHandshakeFrame,
  decodeHandshakeFrame,
  deriveSessionKey,
  buildIdBindContext,
  buildBindTranscript,
  bytesToBase64,
  base64ToBytes,
  PUBKEY_LEN,
  type EphemeralKeyPair,
} from './e2ee';
import { decideKeyBinding, type KeyBindingMode } from './keyBinding';
import { getIceServers, type IceServer } from './apiClient';
import { BASE_DOMAIN, cloudWsScheme } from './apiClient';
import { MAX_PANE_FRAME_BYTES } from '../../transport/remote/cloudMux';
import { encodeChunks, ChunkReassembler } from '../../transport/remote/cloudChunk';
import type { ChannelBackpressure } from './cloudHostBridge';

/** B3：等待信令旁路公钥到达的宽限期（ms）。过期仍未到则回落 relay-trust。 */
const KEY_BIND_GRACE_MS = 3000;

/** DataChannel 标签与参数（契约 §7）。 */
const DC_LABEL = 'ridge';

// ── 信令断线自动重连参数（与 LAN wsRemote.ts / controller provider 同名同值）──
/** 退避基数（ms）。 */
const RECONNECT_BASE_MS = 1_000;
/** 退避上限（ms）。 */
const RECONNECT_MAX_MS = 15_000;

/**
 * DataChannel 背压下水位（弱网 P1）：设为每条 conn DataChannel 的 `bufferedAmountLowThreshold`，
 * 缓冲回落到此即触发 `bufferedamountlow` → 通知 bridge 重同步。与 cloudHostBridge 的
 * 上水位（8 MiB）配对。
 */
const BUFFERED_LOW_WATERMARK = 1 * 1024 * 1024; // 1 MiB

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
  /** 弱网 P1：注入 DataChannel 背压流控（可选；未实现则不背压）。 */
  attachChannelControl?(ctrl: ChannelBackpressure): void;
}

/** 信令消息（契约 §5.1 + §5.3，tag 字段 `t`）。host 端视角。 */
type SignalIn =
  | { t: 'welcome'; room: string; role: 'host' | 'controller'; peerPresent: boolean; cid?: string }
  | { t: 'peer-join'; role: 'host' | 'controller'; cid?: string }
  | { t: 'peer-leave'; role: 'host' | 'controller'; cid?: string }
  | { t: 'error'; code: string; message: string }
  | { t: 'offer'; sdp: string; cid?: string }
  | { t: 'answer'; sdp: string; cid?: string }
  | { t: 'ice'; candidate: RTCIceCandidateInit | null; cid?: string }
  // B3（D-GM-10）：cloud 经已认证信令把对端(controller)临时公钥旁路转发回来（带 cid）。
  | { t: 'e2ee-pubkey'; pubkey: string; cid?: string };

export interface RidgeCloudHostConfig {
  /** device JWT（scope=device），WS 与 ice-servers 鉴权用。 */
  deviceToken: string;
  /** username（host label 拼接用，契约 §1）。 */
  username: string;
  /** Base zone，默认 BASE_DOMAIN，集中可改。 */
  baseDomain?: string;
  /**
   * 零信任 #2（概念 4-桌面）：对 id-bind context 做 Ed25519 设备身份签名
   * （= invoke `sign_device_identity`，私钥在 Rust/DPAPI，relay 无法伪造）。
   * 与 {@link identityPub} **配对注入**：两者俱在 → 握手发 0x02 签名帧；缺一 → 回落 0x01
   * 裸公钥（向后兼容/签名能力不可用时降级，仍由 B3 旁路 + TOTP 兜底）。
   */
  signContext?: (context: Uint8Array) => Promise<Uint8Array>;
  /**
   * 零信任 #2（概念 4-桌面）：本机 Ed25519 设备身份公钥（= invoke
   * `get_device_identity_pub`，启动取一次缓存）。与 {@link signContext} 配对。
   */
  identityPub?: Uint8Array;
}

/** 解析默认值后的内部 host 配置。 */
interface ResolvedHostConfig {
  deviceToken: string;
  username: string;
  baseDomain: string;
  signContext?: (context: Uint8Array) => Promise<Uint8Array>;
  identityPub?: Uint8Array;
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
   * @param bindTranscript 零信任 #1（概念 5）：本会话信道绑定 transcript
   *   （= `buildBindTranscript(host_eph, ctrl_eph)`，host 发 0x02 时非 null；回落 0x01 时为
   *   旁路绑定，仍可计算）。桥据此校验 controller 的 `totp-bind`（HMAC tag，明文码不上线）。
   */
  createBridge: (
    cid: string,
    send: (plaintext: Uint8Array) => void,
    bindTranscript: Uint8Array | null,
  ) => CloudHostBridgeLike;
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
  // ── B3（D-GM-10）E2EE 公钥↔身份绑定（按 cid 独立）──
  /** controller 经已认证信令旁路转发回来的临时公钥；尚未到达为 null。 */
  peerSigKey: Uint8Array | null;
  /** DataChannel 握手解出的 controller 公钥，待与信令公钥比对。 */
  pendingHandshakePub: Uint8Array | null;
  bindTimer: ReturnType<typeof setTimeout> | null;
  bindingDecided: boolean;
  bindingMode: KeyBindingMode;
  /**
   * 零信任 #1（概念 5）：本会话信道绑定 transcript（握手派生会话后算出，
   * `buildBindTranscript(host_eph, ctrl_eph)`），acceptConn 时透传给桥校验 totp-bind。
   * 握手完成前为 null。
   */
  bindTranscript: Uint8Array | null;
  /** 弱网 P1：DataChannel 缓冲回落（bufferedamountlow）时由 bridge 注册的 drain 回调。 */
  onDrained: (() => void) | null;
  // ── 传输层分片（修 RTCDataChannel max-message-size，见 cloudChunk.ts）──
  /** 发送帧计数器（每帧一个 msgId，供接收端重组）。 */
  sendMsgId: number;
  /** 入站分片重组器（按序拼回完整密文再 open）。 */
  reassembler: ChunkReassembler;
}

/**
 * 桌面 host 端云端连接管理器（多控制方）。
 *
 * 生命周期：`goOnline(deviceName)` 连信令 WS 上线 → controller 来去自如 →
 * `goOffline()` 幂等清理全部资源。`kick(cid)` / `blacklist(cid)` 管理单个 controller。
 */
export class RidgeCloudHost {
  private readonly config: ResolvedHostConfig;
  private readonly cb: RidgeCloudHostCallbacks;

  private ws: WebSocket | null = null;
  private hostState: HostSignalState = 'offline';
  private closed = true;
  private iceServers: IceServer[] = [];

  // ── 信令断线自动重连状态（弱网 P1；只重连信令，不拆已建 per-controller RTC）──
  /** 当前上线的 deviceId（信令重连复用）。 */
  private deviceId = '';
  private reconnectTimer: ReturnType<typeof setTimeout> | null = null;
  private reconnectAttempts = 0;

  /** cid → 连接。 */
  private readonly conns = new Map<string, ControllerConn>();
  /** 已拉黑的 cid（会话级；命中即拒绝其 offer 并 kick）。 */
  private readonly banned = new Set<string>();

  constructor(config: RidgeCloudHostConfig, callbacks: RidgeCloudHostCallbacks) {
    this.config = {
      deviceToken: config.deviceToken,
      username: config.username,
      baseDomain: config.baseDomain ?? BASE_DOMAIN,
      signContext: config.signContext,
      identityPub: config.identityPub,
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
    this.deviceId = deviceId;
    this.reconnectAttempts = 0;
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
    if (this.reconnectTimer) { clearTimeout(this.reconnectTimer); this.reconnectTimer = null; }
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
      peerSigKey: null,
      pendingHandshakePub: null,
      bindTimer: null,
      bindingDecided: false,
      bindingMode: 'pending',
      bindTranscript: null,
      onDrained: null,
      sendMsgId: 0,
      reassembler: new ChunkReassembler(),
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
    // §背压（弱网 P1）：缓冲回落到低水位即 bufferedamountlow → 通知 bridge 触发重同步。
    dc.bufferedAmountLowThreshold = BUFFERED_LOW_WATERMARK;
    dc.onbufferedamountlow = () => conn.onDrained?.();
    dc.onopen = () => {
      this.setConnState(conn, 'handshaking');
      this.prepareE2eeHandshake(conn);
    };
    dc.onclose = () => {
      if (!this.closed) this.teardownConn(conn.cid, false);
    };
    dc.onmessage = (ev) => {
      this.onDataChannelMessage(conn, ev.data);
    };
  }

  /**
   * 准备 E2EE 握手（零信任 #2，概念 4-桌面「握手时序反转：先收后发」）：
   * 仅生成本端临时密钥对 + 经信令旁路上报 e2ee-pubkey；**不**发 DataChannel 握手帧。
   *
   * 0x02 签名帧的 sig context 需要 controller 的临时公钥，故 host 不能先发——改为收到
   * controller 0x01 握手帧后（{@link onDataChannelMessage}）才派生会话 + 签名 + 发 0x02
   * （{@link sendHandshakeResponse}）。controller 作为 offerer 先发，故不会死锁（对照 FIX-1c
   * 的 cli 死锁教训：只要有一端先发即可）。
   */
  private prepareE2eeHandshake(conn: ControllerConn): void {
    conn.ephemeral = generateEphemeralKeyPair();
    conn.handshakeDone = false;
    conn.session = null;
    conn.reassembler.reset(); // 新会话：清掉上一会话遗留的在途分片
    // B3：把本端临时公钥经**已认证信令**旁路定向上报给该 cid 的 controller，供其比对
    // DataChannel 握手公钥（两条独立通道，网络 MITM 无法同时篡改）。host 回落 0x01 时该旁路
    // 生效；host 发 0x02 时 controller 走更强的设备签名验证（忽略旁路），此上报无害。
    //
    // 测试 seam（仅 dev e2e harness 置位此 global；**生产永不设置**）：篡改成全 0 的
    // 错误公钥，模拟 relay-MITM 在 E2EE 腿调包 → 对端比对应判 MITM 并拒绝。用于在真链路
    // 上验证 reject 路径（不仅是 decideKeyBinding 纯函数）。
    const tamper = (globalThis as { __RIDGE_DEBUG_TAMPER_E2EE_SIG?: boolean })
      .__RIDGE_DEBUG_TAMPER_E2EE_SIG === true;
    const sigPub = tamper ? new Uint8Array(PUBKEY_LEN) : conn.ephemeral.publicKey;
    this.sendSignal({ t: 'e2ee-pubkey', pubkey: bytesToBase64(sigPub), cid: conn.cid });
  }

  /**
   * 发本端握手响应帧（先收后发，概念 4-桌面）：
   *   - 注入了 signContext + identityPub → 发 **0x02** 设备签名帧（零信任 #2）：
   *     `context = buildIdBindContext(host_eph, ctrl_eph, device, username)`，
   *     `sig = signContext(context)`（= invoke `sign_device_identity`，私钥在 Rust/DPAPI）。
   *     **异步**：签名期间到达的其它帧落入业务分支，因 `conn.bridge` 仍 null 被丢弃；签名
   *     返回后校验连接仍在（未 closed / 未 teardown / dc 仍 open）才发，避免 teardown 后发帧。
   *   - 否则回落发 **0x01** 裸公钥（向后兼容；签名能力不可用时降级，仍由 B3 旁路 + TOTP 兜底）。
   */
  private sendHandshakeResponse(
    conn: ControllerConn,
    hostEph: EphemeralKeyPair,
    controllerPub: Uint8Array,
  ): void {
    const { signContext, identityPub } = this.config;
    if (!signContext || !identityPub) {
      // 回落 0x01（旧行为）。
      this.rawSend(conn, encodeHandshakeFrame(hostEph.publicKey));
      return;
    }
    const context = buildIdBindContext(
      hostEph.publicKey,
      controllerPub,
      this.deviceId,
      this.config.username,
    );
    void (async () => {
      let sig: Uint8Array;
      try {
        sig = await signContext(context);
      } catch (e: unknown) {
        // 签名失败：降级回落 0x01（P2 默认 fail-open；P3 翻闸再改硬拒）。
        this.fail(
          e instanceof Error ? `设备签名失败，回落明文握手：${e.message}` : '设备签名失败，回落明文握手',
          'INTERNAL',
        );
        if (this.isConnLive(conn)) this.rawSend(conn, encodeHandshakeFrame(hostEph.publicKey));
        return;
      }
      if (!this.isConnLive(conn)) return; // 异步签名期间断连/拆除：丢弃，不在死连接上发帧
      try {
        this.rawSend(conn, encodeSignedHandshakeFrame(hostEph.publicKey, identityPub, sig));
      } catch (e: unknown) {
        this.fail(e instanceof Error ? e.message : '发送签名握手帧失败', 'INTERNAL');
      }
    })();
  }

  /** 该连接是否仍存活（用于异步签名回调发帧前的 race 守卫）。 */
  private isConnLive(conn: ControllerConn): boolean {
    return !this.closed && this.conns.get(conn.cid) === conn && conn.dc?.readyState === 'open';
  }

  private onDataChannelMessage(conn: ControllerConn, data: unknown): void {
    const bytes = toBytes(data);
    if (!bytes) return;

    // 握手完成前，首帧必须是 controller 的 0x01 握手帧；否则断开（契约 §7.1）。
    // 先收后发（概念 4-桌面）：收到 controller 临时公钥后才派生会话 + 发本端握手响应（0x02
    // 签名 / 回落 0x01）。
    if (!conn.handshakeDone) {
      try {
        const controllerPub = decodeHandshakeFrame(bytes);
        if (!conn.ephemeral) throw new Error('本端临时密钥缺失');
        const hostEph = conn.ephemeral;
        const key = deriveSessionKey(hostEph.privateKey, hostEph.publicKey, controllerPub);
        // host 端发出方向为 host→controller(dir=0)。
        conn.session = new E2eeSession(key, DIR_HOST_TO_CONTROLLER);
        conn.handshakeDone = true;
        // 零信任 #1（概念 5）：本会话信道绑定 transcript（host 用本机 TOTP 种子验
        // controller 的 totp-bind）。两端独立计算（字典序排序）得同一值。
        conn.bindTranscript = buildBindTranscript(hostEph.publicKey, controllerPub);
        conn.ephemeral = null; // 私钥用完即焚（公钥已旁路上报；签名走 hostEph 闭包副本）
        // 先收后发：发本端握手响应（0x02 签名异步 / 回落 0x01 同步）。host 不验自身签名。
        this.sendHandshakeResponse(conn, hostEph, controllerPub);
        // B3：对 controller 公钥做旁路绑定判定（host 验 controller，与本端签名无关），
        // 通过(或宽限期回落)才 createBridge + 标记 connected。业务帧门控天然成立：连上前
        // conn.bridge 为 null，下方业务分支会丢弃（含异步签名在途期间到达的帧）。
        this.resolveBinding(conn, controllerPub);
      } catch (e: unknown) {
        this.fail(e instanceof Error ? e.message : 'E2EE 握手失败，已断开', 'FORBIDDEN');
        this.teardownConn(conn.cid, false);
      }
      return;
    }

    // 业务帧：先经传输层重组（分片→完整密文），再解密交给该 cid 的桥。
    if (!conn.session || !conn.bridge) return;
    const ciphertext = conn.reassembler.push(bytes);
    if (!ciphertext) return; // 半帧（继续等后续片）或坏帧（已丢弃）
    try {
      const plaintext = conn.session.open(ciphertext);
      // SECURITY (audit #4): drop oversized decrypted frames before demux/JSON.parse
      // so a connected peer can't OOM/stall the UI thread (match "drop bad frame").
      if (plaintext.length > MAX_PANE_FRAME_BYTES) return;
      conn.bridge.handleFrame(plaintext);
    } catch (e: unknown) {
      this.fail(e instanceof Error ? e.message : '收到无法解密的帧（已丢弃）', 'FORBIDDEN');
    }
  }

  // ── B3（D-GM-10）公钥绑定判定（按 cid 独立）──────────────────────────────────
  /** DataChannel 握手解出对端公钥后进入判定（信令公钥可能先到/后到）。 */
  private resolveBinding(conn: ControllerConn, peerPub: Uint8Array): void {
    conn.pendingHandshakePub = peerPub;
    this.decideBinding(conn);
  }

  /**
   * 据「握手公钥 + 信令公钥(可能未到) + 宽限期」三态判定（见 keyBinding.decideKeyBinding）：
   * accept → 建桥+connected；reject → 判 MITM 拆除；wait → 起宽限计时等信令公钥。
   */
  private decideBinding(conn: ControllerConn, graceExpired = false): void {
    if (conn.bindingDecided || this.closed || conn.pendingHandshakePub == null) return;
    if (!this.conns.has(conn.cid)) return; // 已被拆除
    const decision = decideKeyBinding(conn.pendingHandshakePub, conn.peerSigKey, graceExpired);
    if (decision === 'wait') {
      this.armBindGrace(conn);
      return;
    }
    conn.bindingDecided = true;
    if (conn.bindTimer) {
      clearTimeout(conn.bindTimer);
      conn.bindTimer = null;
    }
    if (decision === 'accept') {
      conn.bindingMode = conn.peerSigKey ? 'enforced' : 'relay-trust';
      this.acceptConn(conn, conn.pendingHandshakePub);
    } else {
      // reject：握手公钥 ≠ 信令旁路公钥 → 检测到 MITM。
      this.fail('E2EE 公钥绑定校验失败（疑似 MITM），已断开', 'FORBIDDEN');
      this.teardownConn(conn.cid, true);
    }
  }

  private armBindGrace(conn: ControllerConn): void {
    if (conn.bindTimer || conn.bindingDecided) return;
    conn.bindTimer = setTimeout(() => {
      conn.bindTimer = null;
      this.decideBinding(conn, true);
    }, KEY_BIND_GRACE_MS);
  }

  /** 绑定通过：建应用层桥（含既有 verifyPeerKey 钩子）+ 标记 connected。 */
  private acceptConn(conn: ControllerConn, peerPub: Uint8Array): void {
    // 取应用层桥接管该 cid 的明文帧；sendFrame 闭包加密经本连接发回。零信任 #1（概念 5）：
    // 把本会话 bindTranscript 透传给桥，供其校验 controller 的 totp-bind（HMAC tag）。
    const bridge = this.cb.createBridge(
      conn.cid,
      (plaintext) => this.sendFrame(conn, plaintext),
      conn.bindTranscript,
    );
    // 兼容既有 §5.5 verifyPeerKey 钩子（更一般的注入式机制，默认 relay-trust=true）。
    if (bridge.verifyPeerKey && !bridge.verifyPeerKey(peerPub)) {
      conn.bridge = bridge;
      this.teardownConn(conn.cid, true);
      return;
    }
    conn.bridge = bridge;
    // §背压（弱网 P1）：注入 DataChannel 流控（bufferedAmount 读取 + drain 订阅）。bridge 在
    // bufferedAmount 过高时丢 pane 帧、回落后请求 host 重放 RIS+scrollback。
    bridge.attachChannelControl?.({
      bufferedAmount: () => conn.dc?.bufferedAmount ?? 0,
      onDrained: (cb) => {
        conn.onDrained = cb;
        return () => {
          if (conn.onDrained === cb) conn.onDrained = null;
        };
      },
    });
    this.setConnState(conn, 'connected');
  }

  /** 把一帧明文加密经某连接的 DataChannel 发回 controller（传输层分片，修 max-message-size）。 */
  private sendFrame(conn: ControllerConn, plaintext: Uint8Array): void {
    if (conn.state !== 'connected' || !conn.session) return; // 握手前静默丢弃
    try {
      const sealed = conn.session.seal(plaintext);
      const msgId = conn.sendMsgId++;
      // 密文 ≤ 单条上限走单条 SINGLE；否则切成多条 ≤16KiB 的 CHUNK，接收端按序重组。
      for (const wire of encodeChunks(sealed, msgId)) this.rawSend(conn, wire);
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
    if (conn.bindTimer) { clearTimeout(conn.bindTimer); conn.bindTimer = null; } // B3：清宽限计时
    try { conn.bridge?.reset(); } catch { /* ignore */ }
    if (conn.dc) {
      conn.dc.onopen = conn.dc.onclose = conn.dc.onmessage = null;
      conn.dc.onbufferedamountlow = null;
      try { conn.dc.close(); } catch { /* ignore */ }
    }
    conn.onDrained = null;
    conn.pc.onicecandidate = conn.pc.ondatachannel = conn.pc.onconnectionstatechange = null;
    try { conn.pc.close(); } catch { /* ignore */ }
    conn.ephemeral = null;
    conn.session = null;
    if (notify) this.emitSessions();
  }

  // ─── 信令 WS（契约 §3：WS 用 query ?token=&role=）──────────────────────────

  /**
   * 信令断线退避重连（弱网 P1，与 LAN / controller provider 同口径：base 1s→cap 15s、
   * ±30% 抖动）。只重开信令 WS，**不拆**已建 per-controller RTC（relay 下线不影响 P2P）。
   */
  private scheduleSignalingReconnect(): void {
    if (this.closed || this.reconnectTimer) return;
    const n = this.reconnectAttempts++;
    const base = Math.min(RECONNECT_BASE_MS * 2 ** n, RECONNECT_MAX_MS);
    const delay = Math.round(base + base * 0.3 * Math.random()); // ±30% 抖动
    this.reconnectTimer = setTimeout(() => {
      this.reconnectTimer = null;
      if (this.closed) return;
      this.openSignaling(this.deviceId);
    }, delay);
  }

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
      // 仅上报；断开与重连由 onclose 驱动（避免误置 error 终态）。
      if (!this.closed) this.fail('信令 WebSocket 错误', 'NETWORK');
    };
    ws.onclose = () => {
      if (this.closed) return;
      // 信令断开：已建立的 per-controller RTC 不拆（relay 下线不影响 P2P），仅退避重连
      // 信令通道以恢复「接纳新 controller / 续 ICE / 重协商」。重连成功收 welcome 即回 online。
      this.setHostState('connecting');
      this.scheduleSignalingReconnect();
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
        this.reconnectAttempts = 0; // 信令恢复：重置退避曲线
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
      case 'e2ee-pubkey':
        // B3：controller 经已认证信令旁路转发回来的临时公钥（带 cid）→ 存入该 conn 触发判定。
        if (msg.cid) {
          const conn = this.conns.get(msg.cid);
          if (conn) {
            const pk = base64ToBytes(msg.pubkey);
            if (pk) {
              conn.peerSigKey = pk;
              this.decideBinding(conn);
            }
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
