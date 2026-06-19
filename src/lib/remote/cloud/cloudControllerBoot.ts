// Ridge Cloud — 浏览器 controller 端启动接线（cloud-controller boot）。
//
// 把缺失的最后一块拼图接起来：让浏览器加载的完整桌面 SPA 以 **cloud controller**
// 形态，经云中转（signaling relay + WebRTC E2EE）远控某台桌面 host。
//
// 拓扑（契约 §0）：
//   浏览器(controller, user JWT, offerer)
//     └─ ControllerCloudProvider（信令 role=controller + offer + E2EE dir=1）
//        └─ CloudWebrtcAdapter（L1：1 字节前缀 mux，已就绪，复用）
//           └─ bridge.attach(...) → 内部建 L2 RpcClient（已就绪，复用）
//   + setTransport(new TauriDataProvider())  // FS/git/search 走同一 shimmed invoke
//
// 这是 host 侧 `cloudHostBridge.ts`（answerer）的对端：controller 经此发 invoke /
// pane 订阅 / $/hello，host 桥本地真实执行后回结果（端到端打通）。
//
// 触发方式（优先级从高到低）：
//   1. URL query: `?cloudHost=<device>&u=<username>`（显式指定）
//   2. 租户域名: `{device}-{username}.9527127.xyz`（从 hostname 自动解析）
//   解析规则与 ridge-cloud 后端 validation.rs §1.1/§1.2 逐字一致。
//
// 设计要点（与 LAN boot 在 +layout.svelte 对称）：
//   - 本模块只做"已鉴权传输 → bridge"接线，不解析 mux、不碰 E2EE（都在 provider/adapter）。
//   - user JWT / hostDevice / username 由调用方提供；URL 入口从 localStorage（auth.ts
//     的 cloudAuth）取 user token，从 URL query 或 hostname 取 hostDevice/username 覆盖默认。
//   - 幂等：重复 boot 返回同一句柄（避免重复 attach / 多条 WebRTC）。

import { bridge } from '$lib/transport/tauriShim/bridge';
import { setTransport } from '$lib/transport';
import { TauriDataProvider } from '$lib/transport/tauri';
import {
  createCloudWebrtcTransportWith,
  type CloudWebrtcAdapter,
} from '$lib/transport/remote/cloudWebrtcAdapter';
import { ControllerCloudProvider } from './controllerCloudProvider';
import type { CloudConnectionCallbacks, CloudConnectionState } from './connectionProvider';
import { snapshot as authSnapshot, cloudAuth, refreshAccess } from './auth';
import { get } from 'svelte/store';
import { computeBindTag, bytesToBase64 } from './e2ee';

/** URL query 参数名（cloud-controller 模式触发 + 目标）。 */
export const CLOUD_HOST_PARAM = 'cloudHost'; // 目标 host 的 device_name
export const CLOUD_USER_PARAM = 'u'; // username（host label 拼接 + 同账户校验）

export interface CloudControllerBootParams {
  /** user JWT（scope=user）。省略时从 auth.ts 持久化的 cloudAuth 取。 */
  userToken?: string;
  /** 目标 host 的 device_name（房间 label 的 device 段，§1.1）。 */
  hostDevice: string;
  /** username（房间 label 的 username 段，§1.1）。省略时从 cloudAuth.user.username 取。 */
  username?: string;
  /** 可选：UI 回调（状态/错误透传），与 provider 的 demux/state 回调组合。 */
  onState?: (state: CloudConnectionState) => void;
  onError?: (message: string, code?: string) => void;
}

export interface CloudControllerHandle {
  /** 包好的 L1 适配器（其内已接 provider 的 demux/state 回调）。 */
  readonly adapter: CloudWebrtcAdapter;
  /** 目标 host 的 device_name。 */
  readonly hostDevice: string;
  /**
   * §4 云端 TOTP 二次验证：经 CONTROL 通道（0x12）发 `{ t:'totp-verify', code }`，
   * 等 host 回 `{ t:'totp-result', ok }`。resolve(ok)。超时（默认 10s）→ reject。
   * 连上（'connected'）后、标记 ready 前由 gate 调用。
   */
  verifyTotp(code: string, timeoutMs?: number): Promise<boolean>;
  /** 断开并释放（幂等）：close 适配器（→ provider.disconnect）。 */
  disconnect(): void;
}

/** §4 controller→host TOTP 验证默认超时（ms）。蜂窝网络加 TURN relay 延迟高，从 10s 提至 20s。 */
const TOTP_VERIFY_TIMEOUT_MS = 20_000;

/** 进程内单例句柄：保证幂等（重复 boot 不重复 attach / 不开多条 WebRTC）。 */
let active: CloudControllerHandle | null = null;

/** access token 定时刷新间隔（ms）：10 分钟，短于 15 分钟过期窗口，保证 WS 重连始终用新 token。 */
const TOKEN_REFRESH_INTERVAL_MS = 10 * 60 * 1000;
/** 定时刷新 timer，disconnect 时清除。 */
let refreshTimer: ReturnType<typeof setInterval> | null = null;

/**
 * 以 cloud-controller 形态接线并发起连接。返回句柄（含 adapter）。
 *
 * 副作用：
 *   - `bridge.attach(adapter)`：bridge 内部建 L2 RpcClient + 跑 D9 $/hello（与 LAN 一致）。
 *   - `setTransport(new TauriDataProvider())`：FS/git/search 走同一 shimmed invoke。
 *   - `adapter.connect()`：provider 连信令 → offer → E2EE 握手 → connected。
 *
 * @throws 缺少 userToken / username（无法拼 room / 鉴权）时抛错。
 */
export function startCloudControllerBoot(params: CloudControllerBootParams): CloudControllerHandle {
  if (active) return active; // 幂等：已接线

  const auth = authSnapshot();
  const userToken = params.userToken ?? auth.userToken ?? undefined;
  const username = params.username ?? auth.user?.username ?? undefined;

  if (!userToken) {
    throw new Error('cloud-controller boot 缺少 user token（需先登录或显式传入）');
  }
  if (!username) {
    throw new Error('cloud-controller boot 缺少 username（需先在云端设置用户名或显式传入）');
  }
  if (!params.hostDevice) {
    throw new Error('cloud-controller boot 缺少 hostDevice（目标设备名）');
  }

  // 组合 UI 回调 + 适配器的 demux/state 回调（适配器在工厂内自接 onState/onFrame）。
  const adapter = createCloudWebrtcTransportWith(params.hostDevice, (adapterCallbacks) => {
    const callbacks: CloudConnectionCallbacks = {
      onState: (s) => {
        params.onState?.(s);
        adapterCallbacks.onState?.(s);
      },
      onFrame: (b) => adapterCallbacks.onFrame?.(b),
      onError: (message, code) => params.onError?.(message, code),
    };
    // 传 getter 而非固定字符串：每次 WS/WebRTC (重)连时动态读 cloudAuth store，
    // 保证使用的是最新 access token，防止 15 分钟过期后重连失败。
    return new ControllerCloudProvider({
      userToken: () => get(cloudAuth).userToken ?? userToken,
      username,
      baseDomain: undefined,
    }, callbacks);
  });

  // bridge 内部建 L2 RpcClient + D9 $/hello + use-global-workspace（与 LAN boot 一致）。
  bridge.attach(adapter);
  // FS/git/search 等 DataProvider 消费者走同一 shimmed invoke（经 bridge → RpcClient）。
  setTransport(new TauriDataProvider());

  // 发起连接（信令 → offer → E2EE → connected）。失败经 provider onError/onState 透传。
  void adapter.connect();

  // 定时刷新 access token：每 10 分钟主动刷新，保证 cloudAuth.userToken 始终在过期前更新，
  // 使上方 getter `() => get(cloudAuth).userToken` 在 WS 重连时总能拿到有效 token。
  if (refreshTimer) clearInterval(refreshTimer);
  refreshTimer = setInterval(() => { void refreshAccess(); }, TOKEN_REFRESH_INTERVAL_MS);

  const handle: CloudControllerHandle = {
    adapter,
    hostDevice: params.hostDevice,
    verifyTotp(code, timeoutMs = TOTP_VERIFY_TIMEOUT_MS) {
      return verifyTotpOverControl(adapter, code, timeoutMs);
    },
    disconnect() {
      if (refreshTimer) {
        clearInterval(refreshTimer);
        refreshTimer = null;
      }
      adapter.close();
      adapter.dispose();
      if (active === handle) active = null;
    },
  };
  active = handle;
  return handle;
}

/**
 * §4 一次性 TOTP 握手：在 CONTROL 通道发 `totp-verify`，等首个 `totp-result`。
 * 纯函数（注入 adapter 以便单测）：监听 → 发码 → resolve(ok) / 超时 reject，
 * 完成即退订（无悬挂监听）。
 */
export function verifyTotpOverControl(
  adapter: CloudWebrtcAdapter,
  code: string,
  timeoutMs = TOTP_VERIFY_TIMEOUT_MS,
): Promise<boolean> {
  return new Promise<boolean>((resolve, reject) => {
    let settled = false;
    const timers: ReturnType<typeof setTimeout>[] = [];
    const clearAll = () => {
      for (const t of timers) clearTimeout(t);
    };
    const unsub = adapter.onSessionControl((frame) => {
      if (frame.t !== 'totp-result') return; // 忽略其它 CONTROL 帧
      if (settled) return;
      settled = true;
      clearAll();
      unsub();
      resolve(frame.ok === true);
    });
    // 零信任 #1：host 0x02 后有信道绑定 transcript → 发 totp-bind（HMAC tag，码不明文上线）；
    // 否则（旧 host / 未收到 0x02）回退明文 totp-verify。host 对两者都回 totp-result。
    const send = () => {
      const transcript = adapter.getBindTranscript?.() ?? null;
      if (transcript) {
        const tag = bytesToBase64(computeBindTag(code, transcript));
        adapter.sendSessionControl({ t: 'totp-bind', tag });
      } else {
        adapter.sendSessionControl({ t: 'totp-verify', code });
      }
    };
    send();
    // 弱网兜底：移动蜂窝下 WebRTC 数据通道易在 connected 后劣化丢首帧（表现为 TOTP
    // 「网络错误」超时）。到半程仍无结果则重发一次——host 对重复帧幂等（已验证直接回 ok，
    // 未验证则按同一码重判），把「首帧丢失」与「真超时」区分开，显著降低弱网误超时。
    timers.push(
      setTimeout(() => {
        if (!settled) send();
      }, Math.floor(timeoutMs / 2)),
    );
    timers.push(
      setTimeout(() => {
        if (settled) return;
        settled = true;
        unsub();
        reject(new Error('TOTP 验证超时'));
      }, timeoutMs),
    );
  });
}

/** 当前活跃的 cloud-controller 句柄（无则 null）。 */
export function activeCloudController(): CloudControllerHandle | null {
  return active;
}

/**
 * 从 URL query 解析 cloud-controller 触发参数（`?cloudHost=<device>&u=<username>`）。
 * 缺 cloudHost ⇒ 返回 null（非 cloud-controller 模式）。username 缺省时由 boot 从
 * cloudAuth 兜底；故此处只要求 hostDevice。
 */
export function parseCloudControllerUrl(
  search: string,
): { hostDevice: string; username?: string } | null {
  let params: URLSearchParams;
  try {
    params = new URLSearchParams(search);
  } catch {
    return null;
  }
  const hostDevice = params.get(CLOUD_HOST_PARAM);
  if (!hostDevice) return null;
  const username = params.get(CLOUD_USER_PARAM) ?? undefined;
  return { hostDevice, username };
}

// ── 租户域名解析（与 ridge-cloud validation.rs §1.1/§1.2 逐字一致）─────────────

/**
 * 契约 §1.2 保留字：这些首段 label 不做租户解析（按系统路由）。
 * 与 ridge-cloud `validation.rs` 的 `RESERVED_LABELS` 逐字同步。
 */
const RESERVED_LABELS = new Set([
  'www', 'api', 'ws', 'app', 'admin', 'static', 'cdn', 'mail',
]);

/**
 * 契约 §1.1 username 校验：`^[a-z0-9]{3,20}$`（小写字母+数字，不含连字符，3–20 位）。
 */
const USERNAME_RE = /^[a-z0-9]{3,20}$/;

/**
 * 契约 §1.1 device_name 校验：`^[a-z0-9]([a-z0-9-]*[a-z0-9])?$`，3–30 位，
 * 禁止 `--`，禁止首尾连字符。
 */
const DEVICE_NAME_RE = /^[a-z0-9]([a-z0-9-]*[a-z0-9])?$/;

function isValidUsername(s: string): boolean {
  return USERNAME_RE.test(s);
}

function isValidDeviceName(s: string): boolean {
  return s.length >= 3 && s.length <= 30 && !s.includes('--') && DEVICE_NAME_RE.test(s);
}

/**
 * 从 hostname 解析租户域名，提取 device_name 和 username。
 *
 * 例：`my-laptop-alice.9527127.xyz` → `{ hostDevice: "my-laptop", username: "alice" }`。
 *
 * 解析算法（契约 §1.2，与 ridge-cloud `validation.rs` 的 `parse_tenant_label` 一致）：
 * 1. 取 hostname 首段 label（第一个 `.` 之前，已小写）。
 * 2. 若 label 为保留字 → 返回 null。
 * 3. 按 label 中**最后一个 `-`** 切分为 device 和 username。
 * 4. 分别用 §1.1 正则校验；任一不过 → 返回 null。
 */
export function parseCloudControllerHostname(
  hostname: string,
): { hostDevice: string; username: string } | null {
  // 去端口、取首段 label、规范化小写。
  const withoutPort = hostname.split(':')[0];
  const label = withoutPort.split('.')[0].toLowerCase();
  if (!label) return null;

  // 保留字 → 非租户。
  if (RESERVED_LABELS.has(label)) return null;

  // 按最后一个 '-' 切分。
  const lastDash = label.lastIndexOf('-');
  if (lastDash < 0) return null;

  const device = label.slice(0, lastDash);
  const username = label.slice(lastDash + 1);

  if (!isValidDeviceName(device) || !isValidUsername(username)) return null;

  return { hostDevice: device, username };
}

/**
 * URL 驱动入口：按优先级从 URL query / hostname 解析目标设备，以 cloud-controller
 * 形态接线并连接。
 *
 * 解析顺序：
 *   1. URL query: `?cloudHost=<device>&u=<username>`（显式指定，最高优先级）
 *   2. hostname: `{device}-{username}.9527127.xyz`（租户域名自动解析）
 *
 * 返回句柄；非 cloud-controller 模式或缺凭据返回 null（调用方据此回退到 LAN boot）。
 *
 * 由 +layout.svelte 的 web-remote boot 在浏览器环境调用（SSR 下 location 不可用，
 * 调用方需在 `browser` 守卫内调）。
 */
export function bootCloudControllerFromUrl(
  search: string,
  ui?: Pick<CloudControllerBootParams, 'onState' | 'onError'>,
  hostname?: string,
): CloudControllerHandle | null {
  // 1. 先尝试 URL query 参数（显式指定）。
  const fromQuery = parseCloudControllerUrl(search);
  if (fromQuery) {
    try {
      return startCloudControllerBoot({
        hostDevice: fromQuery.hostDevice,
        username: fromQuery.username,
        onState: ui?.onState,
        onError: ui?.onError,
      });
    } catch {
      return null;
    }
  }

  // 2. 再尝试 hostname 解析（租户域名自动发现）。
  if (hostname) {
    const fromHost = parseCloudControllerHostname(hostname);
    if (fromHost) {
      try {
        return startCloudControllerBoot({
          hostDevice: fromHost.hostDevice,
          username: fromHost.username,
          onState: ui?.onState,
          onError: ui?.onError,
        });
      } catch {
        return null;
      }
    }
  }

  return null;
}
