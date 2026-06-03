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

import { writable, type Writable } from 'svelte/store';
import * as api from './apiClient';
import type { UserDto } from './apiClient';

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
    return typeof localStorage !== 'undefined' ? localStorage : null;
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

export async function refreshMe(): Promise<CloudAuthState> {
  const state = readInitialState();
  if (!state.userToken) throw new api.ApiError('UNAUTHORIZED', '未登录');
  const { user } = await api.getMe(state.userToken);
  return update((s) => ({ ...s, user }));
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
