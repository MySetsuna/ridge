// Ridge Cloud — HTTP API 客户端（契约 §2 信封、§4 API、§1 域名）。
//
// 所有请求/响应走统一信封（§2）：
//   成功：{ ok: true,  data: <T> }
//   失败：{ ok: false, error: { code, message } }
// 客户端按 error.code 返回结构化错误，UI 据 code 映射本地化文案（不直接拼接
// 后端 message）。

/** Base zone（契约 §1）。集中为一个常量便于改。 */
export const BASE_DOMAIN = 'remo2ridge.duckdns.org';

/** 主域名 API 根（契约 §4：全部挂在主域名 /api/v1）。 */
export const API_BASE = `https://${BASE_DOMAIN}/api/v1`;

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
  | 'INTERNAL'
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

/** 把后端 error.code 字符串安全收敛到枚举（未知归 INTERNAL）。 */
function coerceCode(raw: string): ApiErrorCode {
  const known: ApiErrorCode[] = [
    'UNAUTHORIZED', 'FORBIDDEN', 'NOT_FOUND', 'INVALID_INPUT', 'INVALID_KEY',
    'KEY_ALREADY_USED', 'USERNAME_TAKEN', 'USERNAME_REQUIRED', 'NOT_PREMIUM',
    'PAIRING_EXPIRED', 'PAIRING_NOT_FOUND', 'DEVICE_NAME_TAKEN',
    'SIGNATURE_INVALID', 'RATE_LIMITED', 'INTERNAL',
  ];
  return (known as string[]).includes(raw) ? (raw as ApiErrorCode) : 'INTERNAL';
}

interface RequestOptions {
  method?: 'GET' | 'POST' | 'DELETE';
  /** Bearer token（user 或 device）。 */
  token?: string;
  /** JSON body（POST）。 */
  body?: unknown;
}

/**
 * 发起一次 API 请求并解包 §2 信封。
 * 失败统一抛 ApiError（带结构化 code）。
 */
async function request<T>(path: string, opts: RequestOptions = {}): Promise<T> {
  const { method = 'GET', token, body } = opts;
  const headers: Record<string, string> = {};
  if (body !== undefined) headers['Content-Type'] = 'application/json';
  if (token) headers['Authorization'] = `Bearer ${token}`;

  let res: Response;
  try {
    res = await fetch(`${API_BASE}${path}`, {
      method,
      headers,
      body: body !== undefined ? JSON.stringify(body) : undefined,
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

// ─── §4.2 国内卡密激活 ─────────────────────────────────────────────────────

export function activateKey(token: string, key: string, username?: string): Promise<AuthResult> {
  const body: { key: string; username?: string } = { key };
  if (username) body.username = username;
  return request<AuthResult>('/auth/activate-key', { method: 'POST', token, body });
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

// ─── §5.2 ICE servers（必须调此接口取 iceServers，不要硬编码）──────────────

export function getIceServers(token: string): Promise<{ iceServers: IceServer[] }> {
  return request<{ iceServers: IceServer[] }>('/ice-servers', { token });
}
