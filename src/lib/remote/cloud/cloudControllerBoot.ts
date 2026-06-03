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
// 设计要点（与 LAN boot 在 +layout.svelte 对称）：
//   - 本模块只做"已鉴权传输 → bridge"接线，不解析 mux、不碰 E2EE（都在 provider/adapter）。
//   - user JWT / hostDevice / username 由调用方提供；URL 入口从 localStorage（auth.ts
//     的 cloudAuth）取 user token，从 URL query 取 hostDevice/username 覆盖默认。
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
import { snapshot as authSnapshot } from './auth';

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
  /** 断开并释放（幂等）：close 适配器（→ provider.disconnect）。 */
  disconnect(): void;
}

/** 进程内单例句柄：保证幂等（重复 boot 不重复 attach / 不开多条 WebRTC）。 */
let active: CloudControllerHandle | null = null;

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
    return new ControllerCloudProvider({ userToken, username, baseDomain: undefined }, callbacks);
  });

  // bridge 内部建 L2 RpcClient + D9 $/hello + use-global-workspace（与 LAN boot 一致）。
  bridge.attach(adapter);
  // FS/git/search 等 DataProvider 消费者走同一 shimmed invoke（经 bridge → RpcClient）。
  setTransport(new TauriDataProvider());

  // 发起连接（信令 → offer → E2EE → connected）。失败经 provider onError/onState 透传。
  void adapter.connect();

  const handle: CloudControllerHandle = {
    adapter,
    hostDevice: params.hostDevice,
    disconnect() {
      adapter.close();
      adapter.dispose();
      if (active === handle) active = null;
    },
  };
  active = handle;
  return handle;
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

/**
 * URL 驱动入口：若当前 URL 带 `?cloudHost=...`，则以 cloud-controller 形态接线并连接。
 * 返回句柄；非 cloud-controller 模式或缺凭据返回 null（调用方据此回退到 LAN boot）。
 *
 * 由 +layout.svelte 的 web-remote boot 在浏览器环境调用（SSR 下 location 不可用，
 * 调用方需在 `browser` 守卫内调）。
 */
export function bootCloudControllerFromUrl(
  search: string,
  ui?: Pick<CloudControllerBootParams, 'onState' | 'onError'>,
): CloudControllerHandle | null {
  const parsed = parseCloudControllerUrl(search);
  if (!parsed) return null;
  try {
    return startCloudControllerBoot({
      hostDevice: parsed.hostDevice,
      username: parsed.username,
      onState: ui?.onState,
      onError: ui?.onError,
    });
  } catch {
    // 缺 token/username：交由调用方回退（不静默吞）。
    return null;
  }
}
