// Ridge Cloud — 桌面 host 端帧桥（host=answerer 的应用层）。
//
// 角色与定位（契约 §0/§5.1）：桌面 ridge app 是 **host = answerer**。
// `RidgeCloudProvider`（ridgeCloudProvider.ts）负责信令/ICE/DTLS + §7.1 E2EE
// 握手 + §7.2 解密，把**已解密的明文帧**经 `onFrame(plaintext)` 上抛。本桥就是
// 那个 `onFrame` 的消费者：它把明文帧 **demux**，对 controller（浏览器）发来的
// JSON-RPC 控制/invoke 请求在**本地真实执行**（host 是 Tauri 桌面 app），回结果，
// 并把 pane 的 PTY 裸字节推回 controller。
//
// 这是 cloud 路径"未端到端打通"的根因修复（计划 R5）：CloudPanel 的 onFrame 原本
// 是空 stub，本桥把它接成一个与 LAN host（server.rs `dispatch_invoke_jsonrpc` /
// `negotiate_hello`）**同形**的 host 行为：同样的 JSON-RPC 2.0 信封、$/hello 能力
// 协商、能力白名单、error 透传（{code,message,data}）。
//
// 线协议字节格式（契约 §7 / D-GM-7）：本桥**直接复用** `cloudMux.ts` 的
// `demuxFrame`/`encodeJsonFrame`/`encodePaneFrame` —— 即 controller 端 mux/demux
// 用的同一套编解码器。复用同一模块是"逐字一致"最强的保证：双方共享 `0x11`=JSON、
// `0x10 || paneIdLen(u8) || paneId(UTF-8) || raw` 的实现，不存在两套实现漂移的可能。
//
// 依赖注入（保持本桥纯、可单测、不硬绑 Tauri）：
//   - `invoke`：执行本地命令（生产环境由 CloudPanel 注入 `@tauri-apps/api/core`
//     的 `invoke`；测试注入 mock）。
//   - `sendFrame`：把一帧明文经 provider 加密发回 controller（注入
//     `provider.sendFrame`）。
//
// 不在本桥职责内（归属别处）：E2EE 加解密（provider 内部，§7.2）、JWT/设备配对
// 鉴权（auth.ts + provider），以及把 host WebRTC 迁到 Rust(webrtc-rs)（契约 §8 终态）。

import {
  CHANNEL,
  demuxFrame,
  encodeControlFrame,
  encodeJsonFrame,
  encodePaneFrame,
} from '../../transport/remote/cloudMux';
import { isRemoteAllowed } from './remoteAllowlist';

/** §7.3 D9：本 host 实现的协议版本（与 server.rs `REMOTE_PROTOCOL_VERSION` 对齐）。 */
export const HOST_PROTOCOL_VERSION = 1;

/**
 * §7.3 D9：本 host 公告的能力集（与 server.rs `HOST_CAPABILITIES`、客户端
 * `rpcClient.CLIENT_CAPABILITIES` 逐字一致）。controller 取交集后灰掉缺失面板。
 */
export const HOST_CAPABILITIES: readonly string[] = [
  'pane',
  'invoke',
  'fs',
  'git',
  'search',
  'workspace',
  'theme',
] as const;

/** D9 握手方法名（契约 §7.3）。 */
const HELLO_METHOD = '$/hello';
/** D9 版本不匹配 teardown 方法名（契约 §7.3）。 */
const BYE_METHOD = '$/bye';
/** 取消长任务方法名（契约 §7.0）。 */
const CANCEL_METHOD = '$/cancel';

/** JSON-RPC 2.0 标准保留错误码（host 腿用到的子集）。 */
const JSON_RPC_INVALID_REQUEST = -32600;
const JSON_RPC_METHOD_NOT_FOUND = -32601;
const JSON_RPC_INTERNAL_ERROR = -32603;

/**
 * SECURITY (audit #3): 单桥 `totp-verify` 失败上限。达到后锁死本连接的 TOTP 通道
 * （后续 `totp-verify` 一律回 `{ok:false,locked:true}` 不再调校验器），杜绝经永开的
 * CONTROL 通道对 6 位码（±90s 窗口）爆破。重连建新桥 / `reset()` 后才清零（硬上限，
 * 无指数退避——爆破面已封死，无需更复杂的退避）。
 */
const MAX_TOTP_ATTEMPTS = 5;

/**
 * DataChannel 背压流控（弱网 P1）：由 provider 在 acceptConn 后经 {@link CloudHostBridge.attachChannelControl}
 * 注入；未注入则不做背压（行为不变，向后兼容既有构造点 / 测试）。
 */
export interface ChannelBackpressure {
  /** 当前 DataChannel 发送缓冲字节数（provider 读 `conn.dc.bufferedAmount`）。 */
  bufferedAmount(): number;
  /** 订阅「缓冲已回落到低水位」（`bufferedamountlow`）；返回退订函数。 */
  onDrained(cb: () => void): () => void;
}

/**
 * DataChannel 背压上水位：`bufferedAmount` 超过即丢 pane 帧（防 SCTP 发送缓冲无界增长
 * → OOM/卡死）。8 MiB 远低于 libwebrtc ~16 MiB 硬上限，留余量给在途帧。低水位（1 MiB）
 * 在 provider 侧设 `bufferedAmountLowThreshold`，回落经 onDrained 通知。
 */
const BUFFERED_HIGH_WATERMARK = 8 * 1024 * 1024; // 8 MiB

/**
 * 执行本地命令的注入点。生产环境为 `@tauri-apps/api/core` 的 `invoke`。
 * 返回任意 result；抛错则映射成 JSON-RPC error。
 */
export type InvokeFn = (method: string, params?: Record<string, unknown>) => Promise<unknown>;

/** 把一帧明文发回 controller 的注入点（包装 `provider.sendFrame`，内部加密）。 */
export type SendFrameFn = (plaintext: Uint8Array) => void;

/**
 * 订阅一个 pane 的 PTY 输出源 —— **pane 流接入点**（见类文档「pane 流」一节）。
 * 实现方在收到 `subscribe-pane` 时被调用：应开始把该 pane 的裸字节经
 * `onOutput(raw)` 回调推出，并返回一个取消订阅函数。
 *
 * v1 未接入真实 PTY 源时为 `undefined`（桥仅登记意图，不推流）。
 */
export type PaneOutputSource = (
  paneId: string,
  onOutput: (raw: Uint8Array) => void,
) => Unsubscribe;

export type Unsubscribe = () => void;

/**
 * E2EE 公钥 ↔ 设备身份绑定校验钩子（契约 §5.5，安全硬项）。
 *
 * 现状（见报告）：§7.1 握手只验"首帧是 0x01||pub32"，**不**校验对端临时公钥与
 * 配对设备/账户身份的绑定 —— 仅凭信令 relay 能把双方撮合到同一 room 即建会话。
 * relay 是可信撮合方时这够用，但若 relay/cloud 后端被攻陷，理论上可 MITM。
 *
 * 本钩子是 host 侧的最小加固挂载点：握手完成后，桥用对端公钥 + 本端已知的设备
 * 身份上下文（deviceToken 内的 device/username/sub）调用它；返回 false 则桥拒绝
 * 该会话（不处理任何业务帧、回 $/bye 并请上层断开）。
 *
 * v1 默认 `undefined` = 不额外绑定（保持 relay-trust 现状，向后兼容）；当 cloud
 * 后端提供"握手公钥需经 deviceJWT 签名/HMAC 绑定"的带外校验通道后接入。
 */
export type KeyBindingVerifier = (peerPublicKey: Uint8Array) => boolean;

/**
 * 云端 TOTP 二次验证钩子（契约 §4）。controller 经 CONTROL 通道发来 6 位 code，
 * 桥用本机 `RemoteAuth::verify(code)`（±1 窗口，与 LAN 同源）判定。生产环境注入
 * `(code) => invoke('verify_remote_totp', { code })`；测试注入 mock。
 *
 * 未注入时桥**默认放行**（verified=true，向后兼容已有 cloud 连接路径，不回归）。
 */
export type TotpVerifier = (code: string) => Promise<boolean>;

export interface CloudHostBridgeConfig {
  /** 执行本地命令（注入 Tauri `invoke` 或 mock）。 */
  invoke: InvokeFn;
  /** 把明文帧加密发回 controller（注入 `provider.sendFrame`）。 */
  sendFrame: SendFrameFn;
  /** 可选：pane PTY 输出源（pane 流接入点）。未提供时仅登记订阅意图。 */
  paneOutputSource?: PaneOutputSource;
  /**
   * 可选：E2EE 公钥↔设备身份绑定校验（§5.5）。提供则桥在 first-frame 路径校验，
   * 不过即拒会话。未提供保持 relay-trust 现状。
   */
  keyBindingVerifier?: KeyBindingVerifier;
  /**
   * 可选：云端 TOTP 二次验证（契约 §4）。提供则桥在验证通过前**门控业务帧**
   * （拒绝 invoke / pane 订阅），仅放行 CONTROL 通道的 totp 握手。未提供则默认
   * 放行（verified=true），保持既有 cloud 连接路径不回归。
   */
  totpVerifier?: TotpVerifier;
  /** 可选：诊断日志回调（默认 console）。 */
  log?: (level: 'warn' | 'error', message: string, detail?: unknown) => void;
}

/** 一个待执行/已发出请求的取消令牌（$/cancel 尽力中止用）。 */
interface InflightInvoke {
  readonly method: string;
  cancelled: boolean;
  readonly abort: AbortController;
}

/**
 * cloud host 端帧桥。一个连接一个实例；`reset()` 在重连/断开时清状态。
 *
 * 收发对称性（与 server.rs 一致）：controller 发 JSON-RPC request（带 id）→ host
 * 回同 id 的 result/error；controller 发 notification（无 id，如 $/hello、$/cancel、
 * subscribe-pane）→ host 按语义处理，必要时回 notification（$/hello / $/bye）。
 */
export class CloudHostBridge {
  private readonly invoke: InvokeFn;
  private readonly sendFrame: SendFrameFn;
  private readonly paneOutputSource?: PaneOutputSource;
  private readonly keyBindingVerifier?: KeyBindingVerifier;
  private readonly totpVerifier?: TotpVerifier;
  private readonly log: (level: 'warn' | 'error', message: string, detail?: unknown) => void;

  /** 在途 invoke（id → 令牌），供 $/cancel 尽力中止。 */
  private readonly inflight = new Map<string, InflightInvoke>();
  /** 已订阅 pane（paneId → 取消订阅）。host-global 多 pane：每 pane 登记一次。 */
  private readonly paneSubs = new Map<string, Unsubscribe>();
  /** 会话是否已被 §5.5 绑定校验拒绝（拒绝后丢弃一切业务帧）。 */
  private rejected = false;
  /**
   * §4 云端 TOTP 门控旗标：本连接是否已通过 TOTP 二次验证。
   *   - 注入了 `totpVerifier` ⇒ 默认 false，验证通过前拒绝业务帧（仅放行 CONTROL 握手）。
   *   - 未注入 `totpVerifier` ⇒ 默认 true（向后兼容，不门控）。
   */
  private verified: boolean;
  /**
   * SECURITY (audit #3): 本连接累计 TOTP 失败次数；≥ {@link MAX_TOTP_ATTEMPTS} 即锁死
   * TOTP 通道（防 CONTROL 通道爆破）。`reset()` 清零。
   */
  private totpFailures = 0;

  // ── DataChannel 背压流控（弱网 P1；未注入则不背压）──
  /** provider 注入的背压接口（bufferedAmount 读取 + drain 订阅）。 */
  private channel: ChannelBackpressure | null = null;
  /** drain 订阅的退订句柄（attach 替换 / reset 时调）。 */
  private channelUnsub: (() => void) | null = null;
  /** 背压期间丢帧的 pane：缓冲回落后请求 host 重放 RIS+scrollback。 */
  private readonly backpressuredPanes = new Set<string>();

  constructor(config: CloudHostBridgeConfig) {
    this.invoke = config.invoke;
    this.sendFrame = config.sendFrame;
    this.paneOutputSource = config.paneOutputSource;
    this.keyBindingVerifier = config.keyBindingVerifier;
    this.totpVerifier = config.totpVerifier;
    // 未注入 TOTP 校验器 ⇒ 不门控（向后兼容既有 cloud 路径）。
    this.verified = !config.totpVerifier;
    this.log =
      config.log ??
      ((level, message, detail) => {
        // eslint-disable-next-line no-console
        console[level](`[cloudHostBridge] ${message}`, detail ?? '');
      });
  }

  /**
   * provider 在 acceptConn 后注入 DataChannel 背压流控（弱网 P1）。可选——未注入则
   * {@link pushPaneOutput} 不做背压（向后兼容既有构造点 / 测试）。再次调用会替换并退订旧订阅。
   */
  attachChannelControl(ctrl: ChannelBackpressure): void {
    this.channelUnsub?.();
    this.channel = ctrl;
    this.channelUnsub = ctrl.onDrained(() => this.onChannelDrained());
  }

  /**
   * §5.5 安全钩子：在 provider E2EE 握手完成、业务帧开始前，由上层（CloudPanel）
   * 用对端临时公钥调用一次。若注入了 `keyBindingVerifier` 且校验不过：标记会话
   * 拒绝、回 $/bye、返回 false（上层据此 disconnect）。未注入校验器时恒为 true
   * （保持 relay-trust 现状，向后兼容）。
   */
  verifyPeerKey(peerPublicKey: Uint8Array): boolean {
    if (!this.keyBindingVerifier) return true;
    let ok = false;
    try {
      ok = this.keyBindingVerifier(peerPublicKey);
    } catch (e) {
      this.log('error', 'key-binding verifier threw; rejecting session', e);
      ok = false;
    }
    if (!ok) {
      this.rejected = true;
      // 明确拒绝（§7.3 风格）：回 $/bye，由上层断开。
      this.sendControl({
        jsonrpc: '2.0',
        method: BYE_METHOD,
        params: { reason: 'key-binding-failed' },
      });
    }
    return ok;
  }

  /**
   * provider 解密后的明文帧入口。CloudPanel 把 `provider.onFrame` 接到这里。
   *
   * 永不抛错：结构性坏帧/解析失败一律记日志后丢弃（与 provider 的"拒绝坏帧但不
   * 一定断连"立场一致）。
   */
  handleFrame(plaintext: Uint8Array): void {
    if (this.rejected) return; // §5.5 绑定校验已拒绝：丢弃一切业务帧

    let result;
    try {
      result = demuxFrame(plaintext);
    } catch (e) {
      // 0x11 JSON 体解析失败（demuxFrame 在 JSON 分支会抛）→ 丢弃该帧。
      this.log('error', 'failed to demux inbound frame; dropped', e);
      return;
    }

    switch (result.kind) {
      case 'control':
        // §4 CONTROL 通道（0x12）：TOTP 握手。**门控前唯一放行**的通道。
        if (result.json !== null && typeof result.json === 'object') {
          void this.handleSessionControl(result.json as Record<string, unknown>);
        } else {
          this.log('warn', 'non-object CONTROL frame ignored');
        }
        return;
      case 'json':
        // §4 门控：未通过 TOTP 验证前拒绝业务 JSON-RPC（带 id 的回错误帧，
        // 否则静默丢弃 notification）。
        if (!this.verified) {
          this.rejectUnverified(result.json);
          return;
        }
        if (result.json !== null && typeof result.json === 'object') {
          void this.handleControl(result.json as Record<string, unknown>);
        } else {
          this.log('warn', 'non-object JSON control frame ignored');
        }
        return;
      case 'pane':
        // host 一般不收 pane-bytes（controller 不发 PTY 裸字节）。忽略（契约语义）。
        this.log('warn', `unexpected inbound PANE_RAW frame for pane ${result.paneId}; ignored`);
        return;
      case 'unknown':
        // 前向兼容：未知通道 tag → 忽略。
        return;
    }
  }

  // ── §4 云端 TOTP 二次验证（CONTROL 通道 0x12）──────────────────────────────────
  /**
   * 处理一帧 CONTROL 信封。当前仅 `totp-verify`：
   *   controller → host: `{ t: 'totp-verify', code }`
   *   host → controller: `{ t: 'totp-result', ok }`
   * 校验经注入的 `totpVerifier`（生产 = `verify_remote_totp` 命令，本机 RemoteAuth）。
   * ok ⇒ 置 `verified=true`，放行后续业务帧。
   */
  private async handleSessionControl(frame: Record<string, unknown>): Promise<void> {
    if (frame.t !== 'totp-verify') {
      this.log('warn', `unknown CONTROL frame t=${String(frame.t)}; ignored`);
      return;
    }
    const code = typeof frame.code === 'string' ? frame.code : '';
    // 未注入校验器（不门控）：任何 code 都视为通过（与构造时 verified=true 一致）。
    if (!this.totpVerifier) {
      this.verified = true;
      this.sendSessionControl({ t: 'totp-result', ok: true });
      return;
    }
    // 已通过：不再消耗尝试次数（幂等放行）。
    if (this.verified) {
      this.sendSessionControl({ t: 'totp-result', ok: true });
      return;
    }
    // SECURITY (audit #3): 失败次数达上限 → 锁死，不再调校验器（防 CONTROL 通道爆破）。
    if (this.totpFailures >= MAX_TOTP_ATTEMPTS) {
      this.log('warn', 'TOTP locked out (too many failed attempts); rejecting');
      this.sendSessionControl({ t: 'totp-result', ok: false, locked: true });
      return;
    }
    let ok = false;
    try {
      ok = await this.totpVerifier(code);
    } catch (e) {
      this.log('error', 'TOTP verifier threw; treating as failed', e);
      ok = false;
    }
    if (ok) {
      this.verified = true;
    } else {
      // SECURITY (audit #3): 每次失败累加；达上限后本桥后续 totp-verify 直接锁死。
      this.totpFailures += 1;
    }
    const locked = !ok && this.totpFailures >= MAX_TOTP_ATTEMPTS;
    this.sendSessionControl({ t: 'totp-result', ok, ...(locked ? { locked: true } : {}) });
  }

  /**
   * 门控期收到业务 JSON-RPC：带 id 的 request 回一个 JSON-RPC error（让 controller
   * 端 promise reject，而非悬挂）；notification（无 id）静默丢弃。
   */
  private rejectUnverified(json: unknown): void {
    if (json !== null && typeof json === 'object') {
      const id = (json as { id?: unknown }).id;
      if (typeof id === 'number' || typeof id === 'string') {
        this.sendControl(
          jsonrpcError(id, {
            code: JSON_RPC_INVALID_REQUEST,
            message: 'TOTP verification required',
            data: { kind: 'totp-required' },
          }),
        );
        return;
      }
    }
    this.log('warn', 'business frame dropped before TOTP verification');
  }

  /** 把一帧 CONTROL 信封编码为 0x12 帧并发出。 */
  private sendSessionControl(frame: Record<string, unknown>): void {
    this.sendFrame(encodeControlFrame(frame));
  }

  /** 处理一帧 0x11 JSON 控制信封（JSON-RPC 2.0）。 */
  private async handleControl(frame: Record<string, unknown>): Promise<void> {
    if (frame.jsonrpc !== '2.0') {
      this.log('warn', 'control frame missing jsonrpc:"2.0"; ignored');
      return;
    }
    const method = typeof frame.method === 'string' ? frame.method : undefined;
    const id = frame.id;
    const hasId = typeof id === 'number' || typeof id === 'string';

    // 无 method 但有 id 的帧不是请求（host 不向 controller 发请求，故收到的
    // result/error 响应无意义）→ 忽略。
    if (!method) {
      if (!hasId) this.log('warn', 'control frame without method/id; ignored');
      return;
    }

    // ── notification（无 id）：$/hello / $/cancel / subscribe-pane / 其它事件 ──
    if (!hasId) {
      this.handleNotification(method, frame.params);
      return;
    }

    // ── request（有 id）：$/hello 也可能带 id（保守同时支持）；否则走 invoke ──
    if (method === HELLO_METHOD) {
      // 罕见：带 id 的 $/hello。回一帧 $/hello/$/bye notification（D9 语义），
      // 并对 id 回一个空 result 以免 controller 端 request 悬挂。
      this.replyHello(frame.params);
      this.sendControl(jsonrpcResult(id as number | string, null));
      return;
    }
    if (method === CANCEL_METHOD) {
      // $/cancel 规范是 notification；若误带 id，仍尽力中止并回 ack。
      this.cancelInvoke(frame.params);
      this.sendControl(jsonrpcResult(id as number | string, null));
      return;
    }

    await this.dispatchInvoke(id as number | string, method, frame.params);
  }

  /** notification 路由（无 id）。 */
  private handleNotification(method: string, params: unknown): void {
    switch (method) {
      case HELLO_METHOD:
        this.replyHello(params);
        return;
      case CANCEL_METHOD:
        this.cancelInvoke(params);
        return;
      case 'subscribe-pane':
        this.handleSubscribePane(params);
        return;
      default:
        // 其它 controller→host 事件（如 resize、switch-workspace）尚未在本桥落地：
        // 作为 invoke 转发给本地命令（与 LAN host 把控制消息也走 dispatch 一致），
        // 但 notification 无 id ⇒ fire-and-forget（不回响应）。
        void this.dispatchInvokeFireAndForget(method, params);
        return;
    }
  }

  // ── §7.3 D9：$/hello 协商（与 server.rs negotiate_hello 同形）─────────────────
  /**
   * 收到 controller 的 $/hello → 回本 host 的 $/hello（取能力交集）或 $/bye
   * （无公共版本）。逻辑与 server.rs `negotiate_hello` 逐字对齐。
   */
  private replyHello(params: unknown): void {
    this.sendControl(negotiateHello(params));
  }

  // ── §7.0：$/cancel 尽力中止 ───────────────────────────────────────────────────
  private cancelInvoke(params: unknown): void {
    const targetId = (params as { id?: unknown } | null | undefined)?.id;
    if (typeof targetId !== 'number' && typeof targetId !== 'string') {
      this.log('warn', '$/cancel without a valid target id; ignored');
      return;
    }
    const key = String(targetId);
    const entry = this.inflight.get(key);
    if (entry) {
      entry.cancelled = true;
      entry.abort.abort();
      // 不在此删除：dispatchInvoke 的 finally 统一清理，避免竞态丢令牌。
    }
  }

  // ── invoke 路由（有 id 的 JSON-RPC request）──────────────────────────────────
  /**
   * 调本地 `invoke(method, params)` → 回 `0x11 || {jsonrpc,id,result|error}`。
   * 错误映射（与 server.rs `dispatch_invoke_jsonrpc` 对齐）：
   *   - invoke 抛 `{code,message,data}` 形错误 → 透传（保 D-GM-2/D-GM-8 结构）。
   *   - 其它抛错 → JSON-RPC INTERNAL_ERROR(-32603)，message 取错误文本。
   */
  private async dispatchInvoke(
    id: number | string,
    method: string,
    params: unknown,
  ): Promise<void> {
    // §5.4 D8 能力门控（审计 ①-1）：controller 只能调远程白名单内命令。非白名单
    // （尤其 host 特权命令如 get_remote_info → 泄露 LAN TOTP 密钥）一律拒，杜绝
    // 云控制端任意命令 RCE。白名单镜像 ridge_core capability::REMOTE_ALLOWLIST。
    if (!isRemoteAllowed(method)) {
      this.log('warn', `rejected non-allowlisted invoke "${method}"`);
      this.sendControl(
        jsonrpcError(id, {
          code: JSON_RPC_METHOD_NOT_FOUND,
          message: `method not permitted remotely: ${method}`,
          data: { kind: 'forbidden' },
        }),
      );
      return;
    }
    const callParams = normalizeParams(params);
    const key = String(id);
    const token: InflightInvoke = { method, cancelled: false, abort: new AbortController() };
    this.inflight.set(key, token);

    try {
      const result = await this.invoke(method, callParams);
      if (token.cancelled) return; // 已 $/cancel：不回响应（client 已 reject）
      this.sendControl(jsonrpcResult(id, result ?? null));
    } catch (e) {
      if (token.cancelled) return;
      this.sendControl(jsonrpcError(id, toJsonRpcError(e)));
    } finally {
      this.inflight.delete(key);
    }
  }

  /** notification 形态的命令转发（无 id，不回响应；中途出错仅记日志）。 */
  private async dispatchInvokeFireAndForget(method: string, params: unknown): Promise<void> {
    // §5.4 D8 能力门控（审计 ①-1）：notification 路径同样过白名单，丢弃非白名单方法。
    if (!isRemoteAllowed(method)) {
      this.log('warn', `dropped non-allowlisted notification "${method}"`);
      return;
    }
    try {
      await this.invoke(method, normalizeParams(params));
    } catch (e) {
      this.log('warn', `fire-and-forget notification "${method}" failed`, e);
    }
  }

  // ── pane 流（契约 §7.4 / D-GM-7）─────────────────────────────────────────────
  //
  // controller 发 `subscribe-pane`（notification，params.paneId）→ host 应把该
  // pane 的 PTY 裸字节经 `0x10 || paneIdLen || paneId || raw`（encodePaneFrame）
  // 推回。host-global 多 pane：每个 paneId 登记一次（幂等）。
  //
  // **pane 流接入点（v1 未硬塞真实 PTY 源）**：本桥通过注入的 `paneOutputSource`
  // 订阅一个 pane 的输出源并把 raw 字节经 `pushPaneOutput` 推出。真实接入需把
  // `paneOutputSource` 接到桌面 host 的 PTY fan-out（server.rs 侧 `register_remote_sub`
  // / `raw_tx` 等价物，或 AppState 的 per-pane scrollback + 增量流）。在 host WebRTC
  // 仍跑在 WebView/TS（契约 §8 v1 scaffold）期间，这条 PTY 源要么经一个 Tauri
  // event 通道（invoke 订阅 + onPaneRaw event）桥进来，要么等 host 迁到 Rust 后由
  // Rust 侧直接编码。S5 的 D10 屏幕快照（先发快照再续 raw）在此处之前注入。
  private handleSubscribePane(params: unknown): void {
    const paneId = (params as { paneId?: unknown } | null | undefined)?.paneId;
    if (typeof paneId !== 'string' || paneId.length === 0) {
      this.log('warn', 'subscribe-pane without a valid paneId; ignored');
      return;
    }
    if (this.paneSubs.has(paneId)) return; // 幂等：已订阅

    if (!this.paneOutputSource) {
      // 仅登记意图（占位），无真实流。记一个占位 unsub 以保持幂等语义。
      this.paneSubs.set(paneId, () => {});
      this.log(
        'warn',
        `subscribe-pane(${paneId}) registered but no paneOutputSource wired (pane stream TODO)`,
      );
      return;
    }

    const unsub = this.paneOutputSource(paneId, (raw) => {
      this.pushPaneOutput(paneId, raw);
    });
    this.paneSubs.set(paneId, unsub);
  }

  /**
   * 把一个 pane 的 PTY 裸字节推回 controller：`0x10 || paneIdLen || paneId || raw`。
   * 公开以便真实 PTY 源（或测试）直接驱动。
   */
  pushPaneOutput(paneId: string, raw: Uint8Array): void {
    if (this.rejected) return;
    // SECURITY (audit #3): never leak pane bytes before TOTP passes. handleSubscribePane
    // already gates subscription, but guard the push path too (defense in depth) so a
    // pre-verification race / direct caller can't emit PTY output ahead of the gate.
    if (!this.verified) return;
    // §背压（弱网 P1）：DataChannel 缓冲过高 → 丢帧（而非无界堆积撑爆 SCTP 缓冲 → OOM/卡死），
    // 记录待重同步；缓冲回落后 onChannelDrained 请求 host 重放 RIS+scrollback 修复空洞。
    if (this.channel && this.channel.bufferedAmount() > BUFFERED_HIGH_WATERMARK) {
      this.backpressuredPanes.add(paneId);
      return;
    }
    try {
      this.sendFrame(encodePaneFrame(paneId, raw));
    } catch (e) {
      // paneId 过长等编码错误：丢弃该帧但不断连。
      this.log('error', `failed to encode pane frame for ${paneId}; dropped`, e);
    }
  }

  /**
   * DataChannel 缓冲回落到低水位（provider 经 onDrained 通知）：对背压期间丢帧的 pane
   * 请求 host 重放 RIS+scrollback（`invoke('resync_pane_raw')`）。复用 cloud_pane.rs 的
   * desync→RIS+scrollback 恢复原语（与 LAN server.rs 同一套）。fire-and-forget。
   */
  private onChannelDrained(): void {
    if (this.backpressuredPanes.size === 0) return;
    const panes = [...this.backpressuredPanes];
    this.backpressuredPanes.clear();
    for (const paneId of panes) {
      void Promise.resolve(this.invoke('resync_pane_raw', { paneId })).catch((e) =>
        this.log('warn', `resync_pane_raw(${paneId}) failed`, e),
      );
    }
  }

  /** 取消一个 pane 的订阅（host 侧主动，或 pane 关闭时）。 */
  unsubscribePane(paneId: string): void {
    const unsub = this.paneSubs.get(paneId);
    if (unsub) {
      this.paneSubs.delete(paneId);
      try {
        unsub();
      } catch (e) {
        this.log('warn', `pane unsubscribe(${paneId}) threw`, e);
      }
    }
  }

  /** 把一帧 JSON 控制信封编码为 0x11 帧并发出。 */
  private sendControl(envelope: unknown): void {
    this.sendFrame(encodeJsonFrame(envelope));
  }

  /**
   * 重置桥状态（重连/断开时调）：中止所有在途 invoke、退订所有 pane、清拒绝标记。
   * 幂等。
   */
  reset(): void {
    for (const token of this.inflight.values()) {
      token.cancelled = true;
      token.abort.abort();
    }
    this.inflight.clear();
    for (const [paneId] of this.paneSubs) this.unsubscribePane(paneId);
    this.paneSubs.clear();
    this.backpressuredPanes.clear(); // 弱网 P1：清背压待重同步集
    this.rejected = false;
    // §4：重连须重新 TOTP 验证（注入了校验器时 re-arm 门控）。
    this.verified = !this.totpVerifier;
    // SECURITY (audit #3): 重连清零失败计数（新桥/新连接重新获得满额尝试次数）。
    this.totpFailures = 0;
  }
}

// ── 纯函数：与 server.rs 对齐的协议帮手（导出供单测）─────────────────────────────

/** 构造 JSON-RPC 成功响应。 */
export function jsonrpcResult(id: number | string, result: unknown): Record<string, unknown> {
  return { jsonrpc: '2.0', id, result };
}

/** 从 {code,message,data} 错误对象构造 JSON-RPC 错误响应。 */
export function jsonrpcError(
  id: number | string,
  error: { code: number; message: string; data?: unknown },
): Record<string, unknown> {
  return { jsonrpc: '2.0', id, error };
}

/**
 * 把 invoke 抛出的任意错误映射成 JSON-RPC `{code,message,data}`（与 server.rs
 * `dispatch_invoke_jsonrpc` 的错误透传/降级语义对齐）：
 *   - 已是 `{code:number,message:string,data?}` 形（ridge-core `CoreError.to_json_rpc()`
 *     经 Tauri invoke 抛回）→ 原样透传，保 code/data（D-GM-2/D-GM-8）。
 *   - 其它（Error / 字符串 / 未知）→ INTERNAL_ERROR(-32603)，data.kind="internal"。
 */
export function toJsonRpcError(e: unknown): { code: number; message: string; data?: unknown } {
  // ridge-core 结构化错误：{ code, message, data:{kind} }。
  if (
    e !== null &&
    typeof e === 'object' &&
    'code' in e &&
    typeof (e as { code: unknown }).code === 'number' &&
    'message' in e &&
    typeof (e as { message: unknown }).message === 'string'
  ) {
    const obj = e as { code: number; message: string; data?: unknown };
    const out: { code: number; message: string; data?: unknown } = {
      code: obj.code,
      message: obj.message,
    };
    if ('data' in obj && obj.data !== undefined) out.data = obj.data;
    return out;
  }
  const message =
    e instanceof Error
      ? e.message
      : typeof e === 'string'
        ? e
        : 'command failed';
  return { code: JSON_RPC_INTERNAL_ERROR, message, data: { kind: 'internal' } };
}

/**
 * §7.3 D9：根据 controller 的 $/hello params 计算 host 回复。返回 `$/hello`
 * notification（兼容版本，能力取交集）或 `$/bye` notification（无公共版本）。
 * 与 server.rs `negotiate_hello` 逐字对齐。
 */
export function negotiateHello(params: unknown): Record<string, unknown> {
  const p = (params ?? {}) as { protocolVersion?: unknown; capabilities?: unknown };
  const peerVersion = typeof p.protocolVersion === 'number' ? p.protocolVersion : 0;
  if (peerVersion < HOST_PROTOCOL_VERSION) {
    return {
      jsonrpc: '2.0',
      method: BYE_METHOD,
      params: { reason: 'protocol-version-mismatch' },
    };
  }
  const peerCaps = Array.isArray(p.capabilities)
    ? new Set(p.capabilities.filter((c): c is string => typeof c === 'string'))
    : new Set<string>();
  // peerCaps 为空 ⇒ 不约束（与 server.rs `peer_caps.is_empty()` 分支一致）。
  const agreed = HOST_CAPABILITIES.filter((c) => peerCaps.size === 0 || peerCaps.has(c));
  return {
    jsonrpc: '2.0',
    method: HELLO_METHOD,
    params: { protocolVersion: HOST_PROTOCOL_VERSION, capabilities: agreed },
  };
}

/** 把 JSON-RPC `params` 规整成 `invoke` 期望的对象（非对象 → 空对象）。 */
function normalizeParams(params: unknown): Record<string, unknown> {
  if (params !== null && typeof params === 'object' && !Array.isArray(params)) {
    return params as Record<string, unknown>;
  }
  return {};
}

// 重新导出通道常量，便于消费者/测试断言字节布局与 cloudMux 同源。
export { CHANNEL, JSON_RPC_INVALID_REQUEST };
