// Ridge Cloud — 浏览器 controller 端 WebRTC + E2EE provider（契约 §5 信令、§7 E2EE）。
//
// 角色：浏览器 controller 是 **offerer**（契约 §5.1）。它是 `RidgeCloudProvider`
// （桌面 host = answerer）的**镜像**，差异仅在 WebRTC 协商方向与 E2EE 方向：
//   - 信令 role=controller（user JWT，scope=user，§3）；host 用 device JWT、role=host。
//   - **主动创建 DataChannel(label="ridge", ordered) + createOffer/setLocalDescription**，
//     发 offer、收 host 的 answer → setRemoteDescription（host 是被动 answerer）。
//   - E2EE 发出方向为 controller→host(**dir=1**)，收 host→controller(dir=0)；与 host
//     provider 的 dir 严格相反（见 §7.2）。
//
// 连接流程（契约 §5.1）：
//   1. GET /api/v1/ice-servers(Bearer user) 取 iceServers（§5.2，不硬编码 STUN）。
//   2. 连 wss://{hostDevice}-{username}.{BASE_DOMAIN}/ws?token=<userJWT>&role=controller（§1/§3）。
//   3. 收 welcome(peerPresent:true) 或 peer-join（host 已在房）→ controller 创建 offer：
//      createDataChannel("ridge") → createOffer → setLocalDescription → 发 offer。
//   4. 收 host answer → setRemoteDescription；双向 trickle ICE（§5.1）。
//   5. DataChannel open → 跑 §7.1 E2EE 握手（首两条二进制消息：0x01||pub32）。
//   6. 握手完成后，对业务帧加密收发（seal/open）。明文帧 = §7 的 1 字节前缀 mux 帧
//      （由 cloudWebrtcAdapter 处理；本 provider 只做 opaque 加解密）。
//
// ⚠️ 安全现状（GM D-GM-10 / 契约 §5.5）：§7.1 握手只验"首帧是 0x01||pub32"，**不**
// 校验对端临时公钥与配对设备/账户身份的绑定 —— 仅凭信令 relay 把双方撮合到同一
// room 即建会话（relay-trust）。relay 可信时够用；cloud 后端被攻陷理论可 MITM。
// 完整绑定是跨仓库的协议级变更，本期不做（与 host 侧 KeyBindingVerifier 接入点对称）。

import {
  type RemoteConnectionProvider,
  type CloudConnectionState,
  type CloudConnectionCallbacks,
} from './connectionProvider';
import {
  E2eeSession,
  DIR_CONTROLLER_TO_HOST,
  generateEphemeralKeyPair,
  encodeHandshakeFrame,
  decodeAnyHandshakeFrame,
  buildIdBindContext,
  verifyIdBindSignature,
  deriveSessionKey,
  bytesToBase64,
  base64ToBytes,
  type EphemeralKeyPair,
} from './e2ee';
import { checkOrPinDeviceIdentity } from './deviceTrust';
import { decideKeyBinding, type KeyBindingMode } from './keyBinding';
import { getIceServers, type IceServer } from './apiClient';
import { BASE_DOMAIN, cloudWsScheme } from './apiClient';
import { MAX_PANE_FRAME_BYTES } from '../../transport/remote/cloudMux';

/** B3：等待信令旁路公钥到达的宽限期（ms）。过期仍未到则回落 relay-trust。 */
const KEY_BIND_GRACE_MS = 3000;

/** DataChannel 标签（契约 §1.1 / §7：label="ridge"）。 */
const DC_LABEL = 'ridge';

// ── 断线自动重连参数（与 LAN src/remote/lib/wsRemote.ts 同名同值）──
/** 退避基数（ms）。 */
const RECONNECT_BASE_MS = 1_000;
/** 退避上限（ms）。 */
const RECONNECT_MAX_MS = 15_000;
/** 'disconnected'（ICE 抖动）自愈宽限（ms）：超时仍未回 connected 才重连。 */
const DISCONNECTED_WATCHDOG_MS = 8_000;
/** ICE restart 后判定未恢复、升级整体重建的期限（ms）。 */
const ICE_RESTART_DEADLINE_MS = 6_000;

/** 信令消息（契约 §5.1，tag 字段 `t`）。与 host provider 同形。 */
type SignalIn =
  | { t: 'welcome'; room: string; role: 'host' | 'controller'; peerPresent: boolean }
  | { t: 'peer-join'; role: 'host' | 'controller' }
  | { t: 'peer-leave'; role: 'host' | 'controller' }
  | { t: 'error'; code: string; message: string }
  | { t: 'offer'; sdp: string }
  | { t: 'answer'; sdp: string }
  | { t: 'ice'; candidate: RTCIceCandidateInit | null }
  // B3（D-GM-10）：cloud 经已认证信令把对端(host)临时公钥旁路转发回来。
  | { t: 'e2ee-pubkey'; pubkey: string };

export interface ControllerCloudProviderConfig {
  /** user JWT（scope=user），WS 与 ice-servers 鉴权用（§3）。 */
  userToken: string;
  /** username（host label 拼接用，契约 §1；host 与 controller 必须同账户）。 */
  username: string;
  /** Base zone，默认 BASE_DOMAIN，集中可改。 */
  baseDomain?: string;
}

/**
 * 浏览器 controller 端云端连接 provider（offerer）。
 *
 * 单连接对象；一次 connect(hostDevice) 对应一个 host 会话。disconnect 幂等清理。
 * `connect(deviceId)` 的 deviceId = **目标 host 的 device_name**（房间 label 的
 * device 段）。
 */
export class ControllerCloudProvider implements RemoteConnectionProvider {
  private readonly config: Required<ControllerCloudProviderConfig>;
  private readonly cb: CloudConnectionCallbacks;

  private ws: WebSocket | null = null;
  private pc: RTCPeerConnection | null = null;
  private dc: RTCDataChannel | null = null;

  private ephemeral: EphemeralKeyPair | null = null;
  private session: E2eeSession | null = null;
  private handshakeDone = false;
  /** offer 是否已发起（防 welcome+peer-join 双触发重复 createOffer）。 */
  private offerStarted = false;

  private state: CloudConnectionState = 'disconnected';
  private closed = false;
  private hostDevice = '';

  // ── 断线自动重连状态（弱网 P1；口径与 LAN RemoteConnection 对齐）──
  /** 首连缓存的 ICE servers，供重连重建复用（弱网下不再发 API）。 */
  private iceServers: IceServer[] = [];
  private reconnectTimer: ReturnType<typeof setTimeout> | null = null;
  private reconnectAttempts = 0;
  /** 'disconnected'（ICE 抖动）自愈看门狗。 */
  private disconnectedTimer: ReturnType<typeof setTimeout> | null = null;
  /** ICE restart 升级看门狗（限期未恢复 → 整体重建）。 */
  private iceRestartTimer: ReturnType<typeof setTimeout> | null = null;
  /** 本次断线是否已尝试过 ICE restart（避免对同一次断线反复 restart 死循环）。 */
  private iceRestartTried = false;

  // ── B3（D-GM-10）E2EE 公钥↔身份绑定状态 ──
  /** host 经已认证信令旁路转发回来的临时公钥；尚未到达为 null。 */
  private peerSigKey: Uint8Array | null = null;
  /** DataChannel 握手解出的 host 公钥，待与信令公钥比对。 */
  private pendingHandshakePub: Uint8Array | null = null;
  private bindTimer: ReturnType<typeof setTimeout> | null = null;
  private bindingDecided = false;
  private bindingAccepted = false;
  private bindingMode: KeyBindingMode = 'pending';

  constructor(config: ControllerCloudProviderConfig, callbacks: CloudConnectionCallbacks = {}) {
    this.config = {
      userToken: config.userToken,
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

  /** 房间 label：{hostDevice}-{username}（契约 §1.1）。 */
  private roomLabel(hostDevice: string): string {
    return `${hostDevice}-${this.config.username}`;
  }

  async connect(deviceId: string): Promise<void> {
    if (this.state === 'connecting' || this.state === 'handshaking' || this.state === 'connected') {
      return; // 已在连接中，幂等
    }
    this.closed = false;
    this.reconnectAttempts = 0;
    this.offerStarted = false;
    this.hostDevice = deviceId;
    this.resetBinding();
    this.setState('connecting');

    // 1. 取 ICE servers（契约 §5.2：必须调接口，不硬编码 STUN）。
    let iceServers: IceServer[];
    try {
      const res = await getIceServers(this.config.userToken);
      iceServers = res.iceServers ?? [];
    } catch (e: unknown) {
      this.fail(e instanceof Error ? e.message : '获取 ICE 服务器失败', 'NETWORK');
      return;
    }
    if (this.closed) return;

    this.iceServers = iceServers; // 缓存供重连重建复用（STUN/TURN 稳定，弱网下不再发 API）。
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
      if (cs === 'connected') {
        // 连接恢复：重置退避 + 清看门狗。ICE restart 恢复时 E2EE 会话存活、无需重握手，
        // 直接恢复业务态（驱动 L2 'connected' 边沿 → 重订阅 pane / 重发 $/hello）。
        this.reconnectAttempts = 0;
        this.iceRestartTried = false;
        this.clearDisconnectedWatchdog();
        this.clearIceRestartDeadline();
        if (this.handshakeDone && this.bindingAccepted) this.setState('connected');
      } else if (cs === 'failed' || cs === 'closed') {
        if (!this.closed) this.scheduleReconnect('WebRTC 连接中断');
      } else if (cs === 'disconnected') {
        // 短暂抖动可能自愈；起看门狗，超时仍未回 connected 才重连。
        this.armDisconnectedWatchdog();
      }
    };

    // controller 是 offerer：**主动创建** DataChannel（host 端 ondatachannel 被动接收）。
    const dc = pc.createDataChannel(DC_LABEL, { ordered: true });
    this.attachDataChannel(dc);
  }

  private attachDataChannel(dc: RTCDataChannel): void {
    this.dc = dc;
    dc.binaryType = 'arraybuffer';

    dc.onopen = () => {
      this.setState('handshaking');
      this.startE2eeHandshake();
    };
    dc.onclose = () => {
      if (!this.closed) this.scheduleReconnect('数据通道已关闭');
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
    // B3：把本端临时公钥经**已认证信令**旁路上报，供 host 比对 DataChannel 握手公钥
    // （走与 DataChannel 不同的 TLS 信令通道，网络 MITM 无法同时篡改两者）。
    this.sendSignal({ t: 'e2ee-pubkey', pubkey: bytesToBase64(this.ephemeral.publicKey) });
  }

  private onDataChannelMessage(data: unknown): void {
    const bytes = toBytes(data);
    if (!bytes) return;

    // 握手完成前，首帧必须是对端握手帧；否则断开（契约 §7.1）。
    if (!this.handshakeDone) {
      try {
        // 按首字节分派：0x01 旧裸公钥 / 0x02 带设备身份签名（方案 X，零信任 #2）。
        const hs = decodeAnyHandshakeFrame(bytes);
        if (!this.ephemeral) throw new Error('本端临时密钥缺失');
        // 焚毁前留存本端临时公钥：0x02 验签的 context 需要双方临时公钥。
        const myPub = this.ephemeral.publicKey;
        const key = deriveSessionKey(this.ephemeral.privateKey, myPub, hs.ephPub);
        // controller 端发出方向为 controller→host(dir=1)；与 host provider 严格镜像。
        this.session = new E2eeSession(key, DIR_CONTROLLER_TO_HOST);
        this.handshakeDone = true;
        // 握手用完即焚临时私钥引用（公钥已在 startE2eeHandshake 经信令上报）。
        this.ephemeral = null;
        if (hs.kind === 'signed') {
          // 方案 X：host 设备身份签名 + TOFU（设备私钥在 host、relay 无法伪造，强于 B3 旁路）。
          this.resolveSignedBinding(hs.ephPub, myPub, hs.idPub, hs.sig);
        } else {
          // 旧 host（0x01 裸公钥）：回退 B3 e2ee-pubkey 旁路三态判定（向后兼容）。
          this.resolveBindingFromHandshake(hs.ephPub);
        }
      } catch (e: unknown) {
        this.fail(e instanceof Error ? e.message : 'E2EE 握手失败，已断开', 'FORBIDDEN');
        this.disconnect();
      }
      return;
    }

    // 业务帧：解密后上抛明文 mux 帧字节。
    if (!this.session) return;
    // B3：绑定判定通过前不放行任何业务帧（防绑定未决期处理对端数据）。
    if (!this.bindingAccepted) return;
    try {
      const plaintext = this.session.open(bytes);
      // SECURITY (audit #4): drop oversized decrypted frames before they reach the
      // adapter's demux/JSON.parse so a peer can't OOM/stall the UI thread.
      if (plaintext.length > MAX_PANE_FRAME_BYTES) return;
      this.cb.onFrame?.(plaintext);
    } catch (e: unknown) {
      // 解密/重放失败：丢弃该帧但不一定断连（契约要求拒绝该帧）。
      this.cb.onError?.(e instanceof Error ? e.message : '收到无法解密的帧（已丢弃）', 'FORBIDDEN');
    }
  }

  // ── 方案 X（零信任 #2）：0x02 设备身份签名 + TOFU ────────────────────────────
  /**
   * host 发来 0x02 签名握手帧：用 host 设备身份公钥验签本次临时公钥绑定
   * （context = 双方临时公钥‖device‖username），并 TOFU 固定 host 指纹。
   *   - 验签失败 → 判 MITM，断开（设备私钥在 host，relay 无私钥无法伪造）。
   *   - TOFU 指纹变化 → 告警（本期默认**不强拒**；fail-closed 翻闸是 P3）。
   *   - 通过 → 标记 connected（enforced）。设备签名比 B3 旁路更强，故不再走 e2ee-pubkey 比对。
   */
  private resolveSignedBinding(
    hostEphPub: Uint8Array,
    controllerEphPub: Uint8Array,
    hostIdPub: Uint8Array,
    sig: Uint8Array,
  ): void {
    if (this.bindingDecided || this.closed) return;
    const context = buildIdBindContext(
      hostEphPub,
      controllerEphPub,
      this.hostDevice,
      this.config.username,
    );
    if (!verifyIdBindSignature(hostIdPub, context, sig)) {
      this.bindingDecided = true;
      this.fail('E2EE 设备身份签名验证失败（疑似 MITM），已断开', 'FORBIDDEN');
      this.disconnect();
      return;
    }
    // TOFU 固定（host 标识 = {device}-{username}）。指纹变化本期仅告警（P3 翻闸再强拒）。
    const tofu = checkOrPinDeviceIdentity(this.roomLabel(this.hostDevice), hostIdPub);
    if (tofu.status === 'changed') {
      this.cb.onError?.(
        `host 设备指纹变化（原 ${tofu.pinned} → 现 ${tofu.actual}），疑似 MITM 或换机`,
        'FORBIDDEN',
      );
    }
    this.bindingDecided = true;
    this.bindingMode = 'enforced';
    this.bindingAccepted = true;
    this.reconnectAttempts = 0; // 全量握手成功：重置退避曲线
    this.setState('connected');
  }

  // ── B3（D-GM-10）公钥绑定判定 ──────────────────────────────────────────────
  /** DataChannel 握手解出对端公钥后进入判定（信令公钥可能先到/后到）。 */
  private resolveBindingFromHandshake(peerPub: Uint8Array): void {
    this.pendingHandshakePub = peerPub;
    this.decideBinding();
  }

  /**
   * 据「握手公钥 + 信令公钥(可能未到) + 宽限期」三态判定（见 keyBinding.decideKeyBinding）：
   * accept → 标记 connected；reject → 判 MITM 断开；wait → 起宽限计时等信令公钥。
   */
  private decideBinding(graceExpired = false): void {
    if (this.bindingDecided || this.closed || this.pendingHandshakePub == null) return;
    const decision = decideKeyBinding(this.pendingHandshakePub, this.peerSigKey, graceExpired);
    if (decision === 'wait') {
      this.armBindGrace();
      return;
    }
    this.bindingDecided = true;
    if (this.bindTimer) {
      clearTimeout(this.bindTimer);
      this.bindTimer = null;
    }
    if (decision === 'accept') {
      this.bindingMode = this.peerSigKey ? 'enforced' : 'relay-trust';
      this.bindingAccepted = true;
      this.reconnectAttempts = 0; // 全量握手成功：重置退避曲线
      this.setState('connected');
    } else {
      // reject：握手公钥 ≠ 信令旁路公钥 → 检测到 MITM。
      this.fail('E2EE 公钥绑定校验失败（疑似 MITM），已断开', 'FORBIDDEN');
      this.disconnect();
    }
  }

  /** 信令公钥未到时起一次性宽限计时；到期回落 relay-trust（兼容旧端）。 */
  private armBindGrace(): void {
    if (this.bindTimer || this.bindingDecided) return;
    this.bindTimer = setTimeout(() => {
      this.bindTimer = null;
      this.decideBinding(true);
    }, KEY_BIND_GRACE_MS);
  }

  /** 重置绑定状态（connect/disconnect）。 */
  private resetBinding(): void {
    if (this.bindTimer) {
      clearTimeout(this.bindTimer);
      this.bindTimer = null;
    }
    this.peerSigKey = null;
    this.pendingHandshakePub = null;
    this.bindingDecided = false;
    this.bindingAccepted = false;
    this.bindingMode = 'pending';
  }

  /** B3 绑定模式（诊断/测试可读）：enforced=已比对一致；relay-trust=回落；pending=未决。 */
  getKeyBindingMode(): KeyBindingMode {
    return this.bindingMode;
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

  private openSignaling(hostDevice: string): void {
    const label = this.roomLabel(hostDevice);
    const url =
      `${cloudWsScheme(this.config.baseDomain)}://${label}.${this.config.baseDomain}/ws` +
      `?token=${encodeURIComponent(this.config.userToken)}&role=controller`;

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
      // 仅上报；真正的断开与重连由 onclose 驱动（避免误置 error 终态）。
      if (!this.closed) this.cb.onError?.('信令 WebSocket 错误', 'NETWORK');
    };
    ws.onclose = () => {
      if (this.closed) return;
      // RTC 已连通：信令断开无害，且重开信令会在 relay 拿到新 cid → host 视作新 controller
      //（幽灵会话），故不主动重连信令；待 RTC 真断时再整体重建。
      if (this.pc?.connectionState === 'connected') return;
      // 尚未连通 / RTC 也不健康 → 退避重连（整体重建会重开信令）。
      this.scheduleReconnect('信令连接已关闭');
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
        // controller 是 offerer：host 已在房（peerPresent:true）则立即发起 offer。
        if (msg.peerPresent) await this.startOffer();
        break;
      case 'peer-join':
        // host 随后进房 → 此时发起 offer（契约 §5.1：controller 收 peer-join 后建 offer）。
        if (msg.role === 'host') await this.startOffer();
        break;
      case 'peer-leave':
        // host 离开：尚未建立 RTC 时判失败（已连通后交给 connectionstatechange）。
        if (msg.role === 'host' && !this.closed && this.state === 'connecting') {
          this.fail('对端（host）已离开', 'NETWORK');
        }
        break;
      case 'offer':
        // controller 是 offerer，不应收到 offer。忽略。
        break;
      case 'answer':
        try {
          await pc.setRemoteDescription({ type: 'answer', sdp: msg.sdp });
        } catch (e: unknown) {
          this.fail(e instanceof Error ? e.message : '处理 answer 失败', 'INTERNAL');
        }
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
      case 'e2ee-pubkey': {
        // B3：host 经已认证信令旁路转发回来的临时公钥 → 存下并触发绑定判定。
        const pk = base64ToBytes(msg.pubkey);
        if (pk) {
          this.peerSigKey = pk;
          this.decideBinding();
        }
        break;
      }
      case 'error':
        this.fail(msg.message || '信令错误', msg.code);
        break;
    }
  }

  /** controller=offerer：创建 offer 并发出（幂等，防 welcome+peer-join 双触发）。 */
  private async startOffer(): Promise<void> {
    if (this.offerStarted || this.closed) return;
    const pc = this.pc;
    if (!pc) return;
    this.offerStarted = true;
    try {
      const offer = await pc.createOffer();
      await pc.setLocalDescription(offer);
      this.sendSignal({ t: 'offer', sdp: offer.sdp });
    } catch (e: unknown) {
      this.offerStarted = false; // 允许后续重试
      this.fail(e instanceof Error ? e.message : '创建 offer 失败', 'INTERNAL');
    }
  }

  // ─── 断线自动重连（弱网 P1）─────────────────────────────────────────────────
  // 让 provider 断线后真正重连并重新 setState('connected')，自动激活 L2 RpcClient 的
  // onReconnected 重订阅 + 重连 reject/重握手（rpcClient.handleStateChange）。复用既有
  // setupPeerConnection/openSignaling，不另写一套传输逻辑；退避口径与 LAN 完全一致。

  /** 排一次退避重连。connected→disconnected 边沿驱动 L2 reject 在途请求。幂等（已排则跳过）。 */
  private scheduleReconnect(reason?: string): void {
    if (this.closed || this.reconnectTimer) return;
    this.clearDisconnectedWatchdog();
    this.clearIceRestartDeadline();
    if (reason) this.cb.onError?.(reason, 'NETWORK');
    // connected → disconnected 边沿：L2 据此 reject 在途请求（rpcClient.handleStateChange）。
    this.setState('disconnected');
    const n = this.reconnectAttempts++;
    const base = Math.min(RECONNECT_BASE_MS * 2 ** n, RECONNECT_MAX_MS);
    const delay = Math.round(base + base * 0.3 * Math.random()); // ±30% 抖动
    this.reconnectTimer = setTimeout(() => {
      this.reconnectTimer = null;
      void this.reconnect();
    }, delay);
  }

  /** 一次重连尝试：信令仍在 + pc 未关 → ICE restart（同 cid 重协商，最省）；否则整体重建。 */
  private async reconnect(): Promise<void> {
    if (this.closed) return;
    this.setState('connecting');
    const canIceRestart =
      this.ws?.readyState === WebSocket.OPEN &&
      !!this.pc &&
      this.pc.connectionState !== 'closed' &&
      !this.iceRestartTried;
    if (canIceRestart && this.pc) {
      this.iceRestartTried = true;
      try {
        this.offerStarted = false;
        const offer = await this.pc.createOffer({ iceRestart: true });
        await this.pc.setLocalDescription(offer);
        this.sendSignal({ t: 'offer', sdp: offer.sdp });
        this.offerStarted = true;
        this.armIceRestartDeadline(); // 限期内未恢复 → 升级为整体重建
        return;
      } catch {
        // ICE restart 失败 → 落到整体重建。
      }
    }
    this.rebuildAll();
  }

  /** 整体重建：拆旧 pc/dc/ws，复用缓存 iceServers 重建（弱网下不再发 API）。 */
  private rebuildAll(): void {
    this.clearIceRestartDeadline();
    this.teardownTransport();
    if (this.closed) return;
    this.iceRestartTried = false; // 新 pc：下一次抖动可再尝试 ICE restart
    this.offerStarted = false;
    this.resetBinding();
    if (this.iceServers.length === 0) {
      void this.refetchIceAndBuild();
      return;
    }
    this.setupPeerConnection(this.iceServers);
    this.openSignaling(this.hostDevice);
  }

  /** 缓存为空（极端）时取一次 ICE 再重建；失败则继续退避重连。 */
  private async refetchIceAndBuild(): Promise<void> {
    try {
      const res = await getIceServers(this.config.userToken);
      this.iceServers = res.iceServers ?? [];
    } catch {
      if (!this.closed) this.scheduleReconnect('获取 ICE 服务器失败');
      return;
    }
    if (this.closed) return;
    this.setupPeerConnection(this.iceServers);
    this.openSignaling(this.hostDevice);
  }

  /** 关闭 pc/dc/ws 并解绑回调（不置 closed、不 setState）——供重连重建复用。 */
  private teardownTransport(): void {
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
  }

  /** 'disconnected'（ICE 抖动）看门狗：超时仍未自愈回 connected → 重连。 */
  private armDisconnectedWatchdog(): void {
    if (this.disconnectedTimer || this.closed) return;
    this.disconnectedTimer = setTimeout(() => {
      this.disconnectedTimer = null;
      if (this.closed) return;
      if (this.pc?.connectionState === 'connected') return; // 自愈
      this.scheduleReconnect('WebRTC 连接中断');
    }, DISCONNECTED_WATCHDOG_MS);
  }

  private clearDisconnectedWatchdog(): void {
    if (this.disconnectedTimer) { clearTimeout(this.disconnectedTimer); this.disconnectedTimer = null; }
  }

  /** ICE restart 升级看门狗：限期内未回 connected → 整体重建。 */
  private armIceRestartDeadline(): void {
    this.clearIceRestartDeadline();
    this.iceRestartTimer = setTimeout(() => {
      this.iceRestartTimer = null;
      if (this.closed) return;
      if (this.pc?.connectionState === 'connected') return; // restart 成功
      this.rebuildAll();
    }, ICE_RESTART_DEADLINE_MS);
  }

  private clearIceRestartDeadline(): void {
    if (this.iceRestartTimer) { clearTimeout(this.iceRestartTimer); this.iceRestartTimer = null; }
  }

  disconnect(): void {
    this.closed = true;
    if (this.reconnectTimer) { clearTimeout(this.reconnectTimer); this.reconnectTimer = null; }
    this.clearDisconnectedWatchdog();
    this.clearIceRestartDeadline();
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
    this.offerStarted = false;
    // B3：清宽限计时防泄漏（bindingMode 保留供断开后诊断读取，下次 connect 再整体重置）。
    if (this.bindTimer) {
      clearTimeout(this.bindTimer);
      this.bindTimer = null;
    }
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
