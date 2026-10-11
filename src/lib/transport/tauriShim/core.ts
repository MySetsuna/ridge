// src/lib/transport/tauriShim/core.ts
//
// Browser stand-in for `@tauri-apps/api/core`. Aliased in by the web-remote
// Vite build. Re-exports the same public surface the desktop UI imports
// (`invoke`, `isTauri`, `Channel`, `convertFileSrc`, `transformCallback`) but
// routes everything over the LAN remote WebSocket via `bridge`.

import { bridge } from './bridge';
import type { RpcRequestOptions } from '@ridge/remote';
import { readRemoteToken } from '../../../remote/lib/remoteSession';

/**
 * Tauri `Channel<T>` replacement. The desktop's `ptyBridge.ts` creates one for
 * canonical terminal packets and hands it to `register_pane_delta_channel`.
 * The bridge feeds complete packets to the same kernel path as native IPC.
 */
export class Channel<T = unknown> {
  id = 0;
  onmessage: (response: T) => void = () => {};
}

/**
 * In the web-remote build the app IS talking to a real (remote) Tauri host, so
 * `isTauri()` returns true. Every `if (!isTauri()) return;` guard in the desktop
 * UI then runs its normal Tauri path, which the shims tunnel over the WS.
 * Surfaces that genuinely have no browser equivalent are gated on the separate
 * `import.meta.env.RIDGE_WEB_REMOTE` build flag, not on this.
 */
export function isTauri(): boolean {
  return true;
}

export function invoke<T = unknown>(
  cmd: string,
  args: Record<string, unknown> = {},
  options: RpcRequestOptions = {},
): Promise<T> {
  // register_pane_delta_channel carries a non-serializable Channel instance and
  // means "start streaming this pane". Map it onto the subscribe-pane fan-out.
  if (cmd === 'register_pane_delta_channel') {
    const paneId = typeof args.paneId === 'string' ? args.paneId : '';
    const workspaceId = typeof args.workspaceId === 'string' ? args.workspaceId : '';
    bridge.registerTerminalChannel(paneId, workspaceId, typeof args.activationId === 'number' ? args.activationId : NaN, args.channel as Channel<Uint8Array>);
    return Promise.resolve(undefined as T);
  }
  // Browser channels deliver complete packets directly, with no native mailbox.
  if (cmd === 'take_terminal_frames') return Promise.resolve([] as T);
  // Remote subscriptions always stream canonical frames; no native mode toggle.
  if (cmd === 'set_pane_delta_mode') {
    return Promise.resolve(undefined as T);
  }
  return bridge.invoke<T>(cmd, args, options);
}

/**
 * Map a host filesystem path to a URL the browser can fetch. The desktop uses
 * Tauri's `asset://` protocol; here we point at the host's authenticated
 * `/file` endpoint (added in server.rs).
 */
export function convertFileSrc(filePath: string, _protocol = 'asset'): string {
  const token = readRemoteToken() ?? '';
  const qs = new URLSearchParams({ path: filePath });
  if (token) qs.set('token', token);
  return `/file?${qs.toString()}`;
}

/** Tauri internals helper; unused by the WS event path but kept for import
 *  compatibility. Returns a callback id stub. */
export function transformCallback(_callback?: (response: unknown) => void): number {
  return 0;
}
