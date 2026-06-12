// Ridge Cloud — HTTP API 客户端（契约 §2 信封、§4 API、§1 域名）。
//
// 所有请求/响应走统一信封（§2）：
//   成功：{ ok: true,  data: <T> }
//   失败：{ ok: false, error: { code, message } }
// 客户端按 error.code 返回结构化错误，UI 据 code 映射本地化文案（不直接拼接
// 后端 message）。

/**
 * Base zone（契约 §1）。集中为一个常量便于改 —— 所有 cloud 消费者（API、信令 WS、
 * controller 入口域名、ridgeCloudProvider/controllerCloudProvider/cloudControllerBoot）
 * 都从这里取，单点改即全量生效。
 *
 * 默认指向生产 base。**debug 包**通过构建期 `RIDGE_CLOUD_BASE_DOMAIN`（vite define，
 * 见 vite.config.js + scripts/tauri-build-debug.mjs）注入，例如 `localhost:5173`，
 * 把整个 ridge-cloud 客户端指向本地 cloud。子域信令 `{device}-{username}.localhost`
 * 在 Chromium/WebView2 会自动解析到 127.0.0.1，故子域模型在 localhost 同样可用。
 */
const ENV_BASE_DOMAIN = (import.meta.env.RIDGE_CLOUD_BASE_DOMAIN as string | undefined) || '';
export const BASE_DOMAIN = ENV_BASE_DOMAIN || '9527127.xyz';

/**
 * 判定一个 cloud base 域是否为**不安全本机回环**（→ 明文 http/ws，而非 TLS）。
 *
 * 生产 base（`9527127.xyz` 等真实域名）恒为 false，继续走 https/wss。
 * 仅当 base 指向本机回环（`localhost` / `*.localhost` / `127.0.0.0/8` / `0.0.0.0` /
 * `[::1]`，可带端口）时为 true —— 用于自托管 / 本地 ridge-cloud（无 TLS 反代）调试。
 * 这是 apiClient.ts 顶部注释所述「`RIDGE_CLOUD_BASE_DOMAIN=localhost:xxxx` 把客户端
 * 指向本地 cloud」的配套：本地 cloud 是单机 HTTP，必须用 http/ws 而非 https/wss。
 *
 * 纯函数（不读模块状态）以便单测。
 */
export function isInsecureCloudDomain(domain: string): boolean {
  // 去路径，取主机名小写。
  const host = domain.split('/')[0].trim().toLowerCase();
  // 括号 IPv6（[::1] / [::1]:port）：取括号内地址，端口在括号外可忽略。
  if (host.startsWith('[')) {
    const end = host.indexOf(']');
    return host.slice(1, end < 0 ? undefined : end) === '::1';
  }
  // 裸 IPv6（含 >1 个冒号，无 host:port 语义）原样判断，否则去掉尾部 :port。
  // 直接对 `::1` 套 `:\d+$` 会把它误删成 `::`，故先识别裸 IPv6。
  const hostname = (host.match(/:/g)?.length ?? 0) > 1 ? host : host.replace(/:\d+$/, '');
  if (hostname === 'localhost' || hostname.endsWith('.localhost')) return true;
  if (hostname === '0.0.0.0' || hostname === '::1') return true;
  // 127.0.0.0/8 回环段。
  if (/^127\.\d{1,3}\.\d{1,3}\.\d{1,3}$/.test(hostname)) return true;
  return false;
}

/** 构建期逃生开关：dev 默认全链路 TLS；置 RIDGE_CLOUD_DEV_PLAINTEXT=1 时回环 cloud
 *  回退明文 http/ws（mkcert 故障时临时调试用）。经 vite define 注入（见 vite.config.js）。 */
const DEV_PLAINTEXT = (import.meta.env.RIDGE_CLOUD_DEV_PLAINTEXT as string | undefined) === '1';

/** 某 cloud base 域应使用的 HTTP scheme。仅「回环 + 逃生明文」→ http，否则 https。 */
export function cloudHttpScheme(domain: string, plaintext: boolean = DEV_PLAINTEXT): 'http' | 'https' {
  return isInsecureCloudDomain(domain) && plaintext ? 'http' : 'https';
}

/** 某 cloud base 域应使用的 WebSocket scheme。仅「回环 + 逃生明文」→ ws，否则 wss。 */
export function cloudWsScheme(domain: string, plaintext: boolean = DEV_PLAINTEXT): 'ws' | 'wss' {
  return isInsecureCloudDomain(domain) && plaintext ? 'ws' : 'wss';
}

/** 主域名 API 根（契约 §4：全部挂在主域名 /api/v1）。本机回环用 http。 */
export const API_BASE = `${cloudHttpScheme(BASE_DOMAIN)}://${BASE_DOMAIN}/api/v1`;

/** 错误码枚举（契约 §2，前后端共用字符串常量）。 */
export type ApiErrorCode =
  | 'UNAUTHORIZED'
  | 'FORBIDDEN'
  | 'NOT_FOUND'
  | 'INVALID_INPUT'
  | 'INVALID_KEY'
  | 'KEY_ALREADY_USED'
  | 'USERNAME_TAKEN'
  | 'USERNAME_REQUIRED'
  | 'NOT_PREMIUM'
  | 'PAIRING_EXPIRED'
  | 'PAIRING_NOT_FOUND'
  | 'DEVICE_NAME_TAKEN'
  | 'SIGNATURE_INVALID'
  | 'RATE_LIMITED'
  | 'INVALID_RESET_CODE'
  | 'INTERNAL'
  // 浏览器登录授权（契约 §2.1）
  | 'AUTH_REQUEST_NOT_FOUND'
  | 'AUTH_REQUEST_EXPIRED'
  // 传输层兜底（非后端枚举，仅前端用于网络/解析失败）
  | 'NETWORK'
  | 'BAD_RESPONSE';

/** 结构化 API 错误。UI 按 code 映射文案。 */
export class ApiError extends Error {
  readonly code: ApiErrorCode;
  constructor(code: ApiErrorCode, message: string) {
    super(message);
    this.name = 'ApiError';
    this.code = code;
  }
}

/** §2 信封类型。 */
type Envelope<T> = { ok: true; data: T } | { ok: false; error: { code: string; message: string } };

/** 设备形状（契约 §4.1）。 */
export interface DeviceDto {
  name: string;
  createdAt: number;
  /** 主机当前是否在线（接入）。仅 GET /devices 填充；其它内嵌处缺省/false。 */
  online?: boolean;
  /** 主机最近一次接入的秒级 unix 时间戳；缺省表示从未上线。 */
  lastSeenAt?: number;
}

/** 用户形状（契约 §4.1，前后端共用）。 */
export interface UserDto {
  id: string;
  email: string;
  username: string | null;
  plan: 'free' | 'premium';
  devices: DeviceDto[];
}

export interface AuthResult {
  token: string;
  user: UserDto;
}

export interface IceServer {
  urls: string | string[];
  username?: string;
  credential?: string;
}

export interface DeviceCodeResult {
  pairing_code: string;
  poll_token: string;
  expires_in: number;
}

export type DevicePollResult =
  | { status: 'pending' }
  | { status: 'expired' }
  | { status: 'bound'; token: string; device_name: string; username: string };

export interface DeviceActivateResult {
  public_entry: string;
}

/** 浏览器登录授权 — 发起结果（契约 §2.1 `POST /auth/request`）。 */
export interface AuthRequestResult {
  request_code: string;
  poll_token: string;
  authorize_url: string;
  expires_in: number;
  interval: number;
}

/** 浏览器登录授权 — 轮询结果（契约 §2.1 `POST /auth/poll`）。 */
export type AuthPollResult =
  | { status: 'pending' }
  | { status: 'expired' }
  | { status: 'approved'; token: string; user: UserDto };

/**
 * 每日签到结果（契约 §5 `POST /me/checkin`）。
 * - ok=true：本次签到成功，授予 2h 临时 premium 窗口，premiumExpiresAt 为到期秒级 unix。
 * - ok=false + reason='already'：今日已签到（premiumExpiresAt 为当前窗口，可能为 null）。
 * - ok=false + reason='permanent'：已是永久/买断 premium，无需签到（premiumExpiresAt=null）。
 */
export interface CheckinResult {
  ok: boolean;
  reason?: 'already' | 'permanent';
  premiumExpiresAt: number | null;
}

/** 把后端 error.code 字符串安全收敛到枚举（未知归 INTERNAL）。 */
function coerceCode(raw: string): ApiErrorCode {
  const known: ApiErrorCode[] = [
    'UNAUTHORIZED', 'FORBIDDEN', 'NOT_FOUND', 'INVALID_INPUT', 'INVALID_KEY',
    'KEY_ALREADY_USED', 'USERNAME_TAKEN', 'USERNAME_REQUIRED', 'NOT_PREMIUM',
    'PAIRING_EXPIRED', 'PAIRING_NOT_FOUND', 'DEVICE_NAME_TAKEN',
    'SIGNATURE_INVALID', 'RATE_LIMITED', 'INVALID_RESET_CODE', 'INTERNAL',
    'AUTH_REQUEST_NOT_FOUND', 'AUTH_REQUEST_EXPIRED',
  ];
  return (known as string[]).includes(raw) ? (raw as ApiErrorCode) : 'INTERNAL';
}

interface RequestOptions {
  method?: 'GET' | 'POST' | 'DELETE';
  /** Bearer token（user 或 device）。 */
  token?: string;
  /** JSON body（POST）。 */
  body?: unknown;
  /** 带凭证（父域 SSO cookie）。仅 /auth/session bootstrap 用 `'include'`（设计 2026-06-12）。 */
  credentials?: RequestCredentials;
}

/**
 * 401 静默刷新钩子（设计 2026-06-12-cloud-domain-sso）：access token 短时（15min）会过期，
 * auth.ts 在模块初始化时注册一个「用父域 refresh cookie 换新 access」的函数。`request`
 * 收 UNAUTHORIZED 时调它拿新 token、用新 token **重试一次**，避免短 access 过期即掉线。
 * 未注册（如纯 apiClient 单测）则不刷新，原样抛 401。
 */
let onUnauthorized: (() => Promise<string | null>) | null = null;
export function setUnauthorizedHandler(fn: (() => Promise<string | null>) | null): void {
  onUnauthorized = fn;
}

/**
 * 发起 API 请求并解包 §2 信封；带 token 的请求收 401 时尝试静默刷新 + 重试一次。
 * 失败统一抛 ApiError（带结构化 code）。
 */
async function request<T>(path: string, opts: RequestOptions = {}): Promise<T> {
  try {
    return await requestOnce<T>(path, opts);
  } catch (e) {
    // 仅「带 token 的请求收 401 且注册了刷新钩子」才静默刷新 + 重试一次。
    // 无 token 的请求（如 /auth/session 自身）不触发，避免递归。
    if (e instanceof ApiError && e.code === 'UNAUTHORIZED' && opts.token && onUnauthorized) {
      const fresh = await onUnauthorized();
      if (fresh) return requestOnce<T>(path, { ...opts, token: fresh });
    }
    throw e;
  }
}

/** 单次请求（无重试）。被 `request` 包装以支持 401 刷新重试。 */
async function requestOnce<T>(path: string, opts: RequestOptions = {}): Promise<T> {
  const { method = 'GET', token, body, credentials } = opts;
  const headers: Record<string, string> = {};
  if (body !== undefined) headers['Content-Type'] = 'application/json';
  if (token) headers['Authorization'] = `Bearer ${token}`;

  let res: Response;
  try {
    res = await fetch(`${API_BASE}${path}`, {
      method,
      headers,
      body: body !== undefined ? JSON.stringify(body) : undefined,
      credentials,
    });
  } catch (e: unknown) {
    throw new ApiError('NETWORK', e instanceof Error ? e.message : '网络请求失败');
  }

  let envelope: Envelope<T>;
  try {
    envelope = (await res.json()) as Envelope<T>;
  } catch {
    throw new ApiError('BAD_RESPONSE', `响应不是合法 JSON（HTTP ${res.status}）`);
  }

  if (envelope && envelope.ok === true) {
    return envelope.data;
  }
  if (envelope && envelope.ok === false && envelope.error) {
    throw new ApiError(coerceCode(envelope.error.code), envelope.error.message ?? '请求失败');
  }
  throw new ApiError('BAD_RESPONSE', `响应信封格式非法（HTTP ${res.status}）`);
}

// ─── §4.1 账户 ───────────────────────────────────────────────────────────

export function login(email: string, password: string): Promise<AuthResult> {
  return request<AuthResult>('/auth/login', { method: 'POST', body: { email, password } });
}

export function register(email: string, password: string): Promise<AuthResult> {
  return request<AuthResult>('/auth/register', { method: 'POST', body: { email, password } });
}

export function getMe(token: string): Promise<{ user: UserDto }> {
  return request<{ user: UserDto }>('/me', { token });
}

export function setUsername(token: string, username: string): Promise<{ user: UserDto }> {
  return request<{ user: UserDto }>('/auth/set-username', { method: 'POST', token, body: { username } });
}

// ─── 父域 SSO bootstrap（设计 2026-06-12-cloud-domain-sso）─────────────────────

/**
 * 用父域 refresh cookie 换短时 access token。`credentials:'include'` 让浏览器把
 * `Domain=.{base}` 的 `ridge_sso` cookie 带上（子域同站自动发送）。命中回 {token,user}；
 * 无 cookie/失效 → 后端 401 → `request` 抛 ApiError('UNAUTHORIZED')。
 */
export function session(): Promise<AuthResult> {
  return request<AuthResult>('/auth/session', { credentials: 'include' });
}

// ─── §5 每日签到（free 用户每日 2h 免费公网远控）─────────────────────────────

/** 每日签到：授予 2h 临时 premium（已签到/永久 premium 不重复授予）。 */
export function checkin(token: string): Promise<CheckinResult> {
  return request<CheckinResult>('/me/checkin', { method: 'POST', token });
}

// ─── §4.2 国内卡密激活 ─────────────────────────────────────────────────────

export function activateKey(token: string, key: string, username?: string): Promise<AuthResult> {
  const body: { key: string; username?: string } = { key };
  if (username) body.username = username;
  return request<AuthResult>('/auth/activate-key', { method: 'POST', token, body });
}

// ─── §2.1 浏览器登录授权（device-code 形状，产出 user JWT，token 不进 URL）──────

/** 发起登录授权：拿 request_code + poll_token + authorize_url（host 用 opener 打开）。 */
export function authRequest(client: 'desktop' | 'cli'): Promise<AuthRequestResult> {
  return request<AuthRequestResult>('/auth/request', { method: 'POST', body: { client } });
}

/** 轮询登录授权结果：approved 时携带一次性 user JWT + user。 */
export function authPoll(pollToken: string): Promise<AuthPollResult> {
  return request<AuthPollResult>('/auth/poll', { method: 'POST', body: { poll_token: pollToken } });
}

// ─── §4.4 设备配对（Device Code Flow，桌面自助激活）────────────────────────

export function deviceCode(): Promise<DeviceCodeResult> {
  return request<DeviceCodeResult>('/device/code', { method: 'POST', body: {} });
}

export function devicePoll(pollToken: string): Promise<DevicePollResult> {
  return request<DevicePollResult>('/device/poll', { method: 'POST', body: { poll_token: pollToken } });
}

export function deviceActivate(
  token: string,
  pairingCode: string,
  deviceName: string,
): Promise<DeviceActivateResult> {
  return request<DeviceActivateResult>('/device/activate', {
    method: 'POST',
    token,
    body: { pairing_code: pairingCode, device_name: deviceName },
  });
}

export function listDevices(token: string): Promise<{ devices: DeviceDto[] }> {
  return request<{ devices: DeviceDto[] }>('/devices', { token });
}

export function deleteDevice(token: string, name: string): Promise<{ ok: boolean }> {
  return request<{ ok: boolean }>(`/devices/${encodeURIComponent(name)}`, { method: 'DELETE', token });
}

// ─── §4.1 忘记密码 / 重置密码 ──────────────────────────────────────────────

/** 忘记密码：发送重置码到邮箱。始终返回 {ok:true}（防枚举）。 */
export function forgotPassword(email: string): Promise<{ ok: boolean }> {
  return request<{ ok: boolean }>('/auth/forgot-password', { method: 'POST', body: { email } });
}

/** 重置密码：邮箱 + 重置码 + 新密码 → 签发新 token（即登录）。 */
export function resetPassword(email: string, code: string, password: string): Promise<AuthResult> {
  return request<AuthResult>('/auth/reset-password', { method: 'POST', body: { email, code, password } });
}

// ─── §5.2 ICE servers（必须调此接口取 iceServers，不要硬编码）──────────────

export function getIceServers(token: string): Promise<{ iceServers: IceServer[] }> {
  return request<{ iceServers: IceServer[] }>('/ice-servers', { token });
}
