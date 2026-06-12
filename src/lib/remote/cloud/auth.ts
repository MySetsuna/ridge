// Ridge Cloud — 登录态管理 + 桌面自助设备激活（契约 §3 JWT、§4.1/§4.2/§4.4）。
//
// 持久化（localStorage）：
//   - user JWT（scope=user，§3，30 天）→ ridge.cloud.userToken
//   - 缓存的 user 对象 → ridge.cloud.user
//   - device JWT（scope=device，§3，180 天）→ ridge.cloud.deviceToken
//   - 已激活的 device_name → ridge.cloud.deviceName
//
// 注意：JWT 仅做客户端展示用途的浅解码（读 plan/username/exp），真正的校验在
// 后端。不信任本地 token 的真实性，仅用于 UI 状态判断。

import { writable, get, type Writable } from 'svelte/store';
import * as api from './apiClient';
import type { CheckinResult, UserDto } from './apiClient';

const LS_USER_TOKEN = 'ridge.cloud.userToken';
const LS_USER = 'ridge.cloud.user';
const LS_DEVICE_TOKEN = 'ridge.cloud.deviceToken';
const LS_DEVICE_NAME = 'ridge.cloud.deviceName';

/** 配对轮询间隔（契约 §4.4 建议每 2s）。 */
const POLL_INTERVAL_MS = 2000;
/** 配对轮询总超时上限（配对码 TTL 600s，留一点余量）。 */
const POLL_TIMEOUT_MS = 600_000;

export interface CloudAuthState {
  userToken: string | null;
  user: UserDto | null;
  deviceToken: string | null;
  deviceName: string | null;
}

/** SSR/Node（测试、vite build prerender）下无 localStorage 的安全访问。 */
function ls(): Storage | null {
  try {
    if (typeof localStorage === 'undefined') return null;
    // 某些运行时（如 Node 实验性 localStorage、未带文件路径）提供「半成品」全局：
    // 对象存在但 getItem/setItem 非函数。校验方法可用，否则视为不可用退化为内存态。
    if (typeof localStorage.getItem !== 'function') return null;
    return localStorage;
  } catch {
    return null;
  }
}

function readInitialState(): CloudAuthState {
  const store = ls();
  if (!store) return { userToken: null, user: null, deviceToken: null, deviceName: null };
  let user: UserDto | null = null;
  const rawUser = store.getItem(LS_USER);
  if (rawUser) {
    try {
      user = JSON.parse(rawUser) as UserDto;
    } catch {
      user = null;
    }
  }
  return {
    userToken: store.getItem(LS_USER_TOKEN),
    user,
    deviceToken: store.getItem(LS_DEVICE_TOKEN),
    deviceName: store.getItem(LS_DEVICE_NAME),
  };
}

/** 全局云端登录态（响应式）。 */
export const cloudAuth: Writable<CloudAuthState> = writable<CloudAuthState>(readInitialState());

function persist(state: CloudAuthState): void {
  const store = ls();
  if (!store) return;
  if (state.userToken) store.setItem(LS_USER_TOKEN, state.userToken);
  else store.removeItem(LS_USER_TOKEN);
  if (state.user) store.setItem(LS_USER, JSON.stringify(state.user));
  else store.removeItem(LS_USER);
  if (state.deviceToken) store.setItem(LS_DEVICE_TOKEN, state.deviceToken);
  else store.removeItem(LS_DEVICE_TOKEN);
  if (state.deviceName) store.setItem(LS_DEVICE_NAME, state.deviceName);
  else store.removeItem(LS_DEVICE_NAME);
}

function update(mut: (s: CloudAuthState) => CloudAuthState): CloudAuthState {
  let next!: CloudAuthState;
  cloudAuth.update((cur) => {
    next = mut(cur);
    persist(next);
    return next;
  });
  return next;
}

/** 读取当前快照（非响应式）。 */
export function snapshot(): CloudAuthState {
  return readInitialState();
}

/** 是否已登录（有 user token）。 */
export function isLoggedIn(state: CloudAuthState): boolean {
  return !!state.userToken;
}

/** 是否 premium（按缓存的 user.plan）。 */
export function isPremium(state: CloudAuthState): boolean {
  return state.user?.plan === 'premium';
}

/** 公网入口域名（契约 §1）：{device}-{username}.{BASE_DOMAIN}。 */
export function publicEntryDomain(state: CloudAuthState): string | null {
  const device = state.deviceName;
  const username = state.user?.username;
  if (!device || !username) return null;
  return `${device}-${username}.${api.BASE_DOMAIN}`;
}

// ─── 登录 / 退出 ───────────────────────────────────────────────────────────

export async function login(email: string, password: string): Promise<CloudAuthState> {
  const { token, user } = await api.login(email, password);
  return update((s) => ({ ...s, userToken: token, user }));
}

/**
 * 跨子域 fragment 交接落盘（方案 B）：主域登录后经 `#token=<jwt>` 回跳到租户子域，
 * 控制端 boot 在此把 user token 写入本子域 localStorage，使 cloud 远控接线可发起。
 *
 * 只落 token：user 对象按需由 refreshMe()/`/me` 补齐；租户域下 username 由 hostname 提供
 * （见 cloudControllerBoot 的 parseCloudControllerHostname），故此处无须 user 即可 boot。
 */
export function persistHandoffToken(token: string): void {
  update((s) => ({ ...s, userToken: token }));
}

/**
 * 父域 cookie bootstrap（设计 2026-06-12-cloud-domain-sso）：调 `GET /auth/session`
 * （带父域 `ridge_sso` cookie）换短时 access token。成功 → 写入 userToken+user（seed
 * 现有 Bearer 流程，apiClient 零改动）→ true；401/网络失败 → false（调用方跳主域登录）。
 * 这是替代 `#token=` 跨子域握手的免重登入口。
 */
export async function bootstrapFromCookie(): Promise<boolean> {
  try {
    const { token, user } = await api.session();
    update((s) => ({ ...s, userToken: token, user }));
    return true;
  } catch {
    return false;
  }
}

// ─── 401 静默刷新（设计 2026-06-12）：短 access 过期 → 用 refresh cookie 换新 ──────────
// 单飞去重：多个并发 401 共享同一个 in-flight 刷新，避免刷新风暴。
let refreshing: Promise<boolean> | null = null;
function refreshAccess(): Promise<boolean> {
  if (!refreshing) {
    refreshing = bootstrapFromCookie().finally(() => {
      refreshing = null;
    });
  }
  return refreshing;
}

// 模块初始化即把刷新钩子注册进 apiClient：带 token 的请求收 401 → 刷新换新 access 重试一次。
api.setUnauthorizedHandler(async () => {
  const ok = await refreshAccess();
  return ok ? get(cloudAuth).userToken : null;
});

// ─── §2.3 浏览器登录授权（host 轮询拿 user JWT，token 不进 URL）──────────────

export interface BrowserLoginProgress {
  /** 已打开的浏览器授权地址（UI 可展示「未自动打开？点此」回退链接）。 */
  authorizeUrl: string;
  /** 配对码（仅作展示/排障，token 永远走轮询拿）。 */
  requestCode: string;
}

export interface BrowserLoginOptions {
  /** 拿到 authorize_url 后回调（用于 UI 展示回退链接 / 配对码）。 */
  onProgress?: (p: BrowserLoginProgress) => void;
  /** 取消轮询。 */
  signal?: AbortSignal;
  /**
   * 「立即轮询」唤醒源。`ridge://auth/focus` 把桌面端拉回前台后，Rust 侧广播
   * `ridge://auth-focus` 事件；调用方把该事件桥接为 onWake(cb)，授权批准后免去
   * 等待下一个轮询间隔。返回取消订阅函数。
   */
  onWake?: (cb: () => void) => () => void;
}

/**
 * 浏览器登录授权（契约 §2.3）：
 *   1. POST /auth/request {client:'desktop'} → request_code + poll_token + authorize_url
 *   2. opener 打开 authorize_url（默认浏览器；不可用时退回 window.open）
 *   3. 每 interval s 轮询 POST /auth/poll {poll_token}，直到 approved / expired / 超时
 *   4. approved → 把 {token,user} 写入 cloudAuth（与 login() 一致）
 *
 * token 绝不经 `ridge://` URL 传递——URI 仅作「唤起回前台」信号（§1）。
 */
export async function loginViaBrowser(opts: BrowserLoginOptions = {}): Promise<CloudAuthState> {
  const { onProgress, signal, onWake } = opts;

  // 1. 发起授权请求。
  const req = await api.authRequest('desktop');
  onProgress?.({ authorizeUrl: req.authorize_url, requestCode: req.request_code });

  // 2. 用默认浏览器打开授权页。
  await openExternalUrl(req.authorize_url);

  // 3. 轮询直到批准 / 过期 / 超时。
  const intervalMs = Math.max(1, req.interval) * 1000;
  const deadline = Date.now() + Math.min(POLL_TIMEOUT_MS, req.expires_in * 1000);

  // `ridge://auth-focus` 事件 → 提前结束当前等待，立即再轮询一次。
  let wake: (() => void) | null = null;
  const unsubWake = onWake?.(() => wake?.());

  try {
    // eslint-disable-next-line no-constant-condition
    while (true) {
      if (signal?.aborted) throw new api.ApiError('INVALID_INPUT', '已取消');
      if (Date.now() > deadline) throw new api.ApiError('AUTH_REQUEST_EXPIRED', '登录授权超时');

      const poll = await api.authPoll(req.poll_token);
      if (poll.status === 'approved') {
        return update((s) => ({ ...s, userToken: poll.token, user: poll.user }));
      }
      if (poll.status === 'expired') {
        throw new api.ApiError('AUTH_REQUEST_EXPIRED', '登录授权已过期');
      }
      await waitable(intervalMs, signal, (resolve) => { wake = resolve; });
      wake = null;
    }
  } finally {
    unsubWake?.();
  }
}

/** opener 优先打开外链；不可用（如纯浏览器 web-remote）时退回 window.open。 */
async function openExternalUrl(url: string): Promise<void> {
  try {
    const m = await import('@tauri-apps/plugin-opener');
    await m.openUrl(url);
  } catch {
    try {
      window.open(url, '_blank', 'noopener');
    } catch {
      /* 无 window（测试/SSR），忽略——UI 仍展示回退链接 */
    }
  }
}

/**
 * 可被「唤醒」的延时：等满 ms 自动 resolve；signal abort 则 reject；register 暴露一个
 * 提前 resolve 的钩子（接到 ridge://auth-focus 时立即再轮询，免等下一个间隔）。
 */
function waitable(
  ms: number,
  signal: AbortSignal | undefined,
  register: (resolve: () => void) => void,
): Promise<void> {
  return new Promise((resolve, reject) => {
    let done = false;
    const finish = () => {
      if (done) return;
      done = true;
      clearTimeout(id);
      signal?.removeEventListener('abort', onAbort);
      resolve();
    };
    const onAbort = () => {
      if (done) return;
      done = true;
      clearTimeout(id);
      reject(new api.ApiError('INVALID_INPUT', '已取消'));
    };
    const id = setTimeout(finish, ms);
    signal?.addEventListener('abort', onAbort, { once: true });
    register(finish);
  });
}

export async function refreshMe(): Promise<CloudAuthState> {
  const state = readInitialState();
  if (!state.userToken) throw new api.ApiError('UNAUTHORIZED', '未登录');
  const { user } = await api.getMe(state.userToken);
  return update((s) => ({ ...s, user }));
}

// ─── 忘记密码 / 重置密码 ─────────────────────────────────────────────────────

/** 忘记密码：发送重置码到邮箱。始终成功（防枚举），UI 不应根据返回值判断邮箱是否存在。 */
export async function forgotPassword(email: string): Promise<void> {
  await api.forgotPassword(email);
}

/** 重置密码：验证重置码 + 设新密码 → 登录态自动写入 cloudAuth。 */
export async function resetPassword(email: string, code: string, password: string): Promise<CloudAuthState> {
  const { token, user } = await api.resetPassword(email, code, password);
  return update((s) => ({ ...s, userToken: token, user }));
}

// ─── §5 每日签到（free 用户每日 2h 免费公网远控）─────────────────────────────

/**
 * 每日签到（契约 §5）：调 POST /me/checkin 授予 2h 临时 premium，然后 refreshMe()
 * 重新拉取 /me 刷新 plan/premium 展示态。返回后端结果（含 premiumExpiresAt + reason），
 * 供 UI 展示「已授予至…」/「今日已签到」/「已是永久 premium」。
 *
 * 注意：成功后 user.plan 变为 'premium'，cloudAuth 据此联动隐藏升级/签到入口。
 */
export async function checkin(): Promise<CheckinResult> {
  const state = readInitialState();
  if (!state.userToken) throw new api.ApiError('UNAUTHORIZED', '未登录');
  const result = await api.checkin(state.userToken);
  // 签到成功（或已签到/永久）后重新拉取 /me 以同步 plan/premium 展示态；
  // 刷新失败不影响签到结果回报（UI 仍按 result 展示）。
  try {
    await refreshMe();
  } catch {
    /* /me 刷新失败容错：不阻断签到结果回报 */
  }
  return result;
}

export function logout(): void {
  update(() => ({ userToken: null, user: null, deviceToken: null, deviceName: null }));
}

/** 卡密激活（§4.2）。成功后回写 premium user + token。 */
export async function activateKey(key: string, username?: string): Promise<CloudAuthState> {
  const state = readInitialState();
  if (!state.userToken) throw new api.ApiError('UNAUTHORIZED', '需先登录再激活');
  const { token, user } = await api.activateKey(state.userToken, key, username);
  return update((s) => ({ ...s, userToken: token, user }));
}

// ─── §4.4 桌面自助设备激活（Device Code Flow）───────────────────────────────

export interface DeviceActivationProgress {
  pairingCode: string;
  expiresIn: number;
}

/**
 * 桌面自助激活本机为一台云端设备：
 *   1. POST /device/code   → 拿 pairing_code + poll_token
 *   2. POST /device/activate(Bearer user) 传本机选定 device_name 绑定
 *   3. POST /device/poll   轮询直到 status==='bound'，取回 device JWT 持久化
 *
 * 说明：桌面端同时持有 user token，可在本进程内一气呵成（不像无头 CLI 需要
 * 用户跨设备输码）。先 activate 立即绑定，再 poll 取回 device JWT。
 *
 * @param deviceName 本机选定的设备名（须满足契约 §1.1 正则；后端再校验）。
 * @param onProgress 可选：拿到配对码后回调（用于 UI 展示）。
 * @param signal     可选：AbortSignal 取消轮询。
 */
export async function activateThisDevice(
  deviceName: string,
  onProgress?: (p: DeviceActivationProgress) => void,
  signal?: AbortSignal,
): Promise<CloudAuthState> {
  const state = readInitialState();
  if (!state.userToken) throw new api.ApiError('UNAUTHORIZED', '需先登录');

  // 1. 取配对码
  const code = await api.deviceCode();
  onProgress?.({ pairingCode: code.pairing_code, expiresIn: code.expires_in });

  // 2. 用 user token 绑定（传本机 device_name）
  await api.deviceActivate(state.userToken, code.pairing_code, deviceName);

  // 3. 轮询取回 device JWT
  const deadline = Date.now() + Math.min(POLL_TIMEOUT_MS, code.expires_in * 1000);
  // eslint-disable-next-line no-constant-condition
  while (true) {
    if (signal?.aborted) throw new api.ApiError('INVALID_INPUT', '已取消');
    if (Date.now() > deadline) throw new api.ApiError('PAIRING_EXPIRED', '配对超时');

    const poll = await api.devicePoll(code.poll_token);
    if (poll.status === 'bound') {
      return update((s) => ({
        ...s,
        deviceToken: poll.token,
        deviceName: poll.device_name,
        // username 若后端在 bound 里回带，且本地 user 尚无，则补齐展示。
        user: s.user
          ? { ...s.user, username: s.user.username ?? poll.username }
          : s.user,
      }));
    }
    if (poll.status === 'expired') {
      throw new api.ApiError('PAIRING_EXPIRED', '配对码已过期');
    }
    await delay(POLL_INTERVAL_MS, signal);
  }
}

function delay(ms: number, signal?: AbortSignal): Promise<void> {
  return new Promise((resolve, reject) => {
    const id = setTimeout(resolve, ms);
    signal?.addEventListener(
      'abort',
      () => {
        clearTimeout(id);
        reject(new api.ApiError('INVALID_INPUT', '已取消'));
      },
      { once: true },
    );
  });
}
