// src/lib/transport/tauriShim/core.ts
//
// Browser stand-in for `@tauri-apps/api/core`. Aliased in by the web-remote
// Vite build. Re-exports the same public surface the desktop UI imports
// (`invoke`, `isTauri`, `Channel`, `convertFileSrc`, `transformCallback`) but
// routes everything over the LAN remote WebSocket via `bridge`.

import { bridge } from './bridge';

const TOKEN_KEY = 'ridge_remote_token';

/**
 * Tauri `Channel<T>` replacement. The desktop's `ptyBridge.ts` creates one for
 * PTY delta bytes and hands it to `register_pane_delta_channel`. In the browser
 * the host can't push into a JS Channel, so we never feed it — PTY bytes arrive
 * through the binary raw-byte fan-out and are routed to the `pty-output-*`
 * listener instead (see bridge.dispatchRawBytes). The class only needs to exist
 * so `new Channel()` and `channel.onmessage = …` don't blow up.
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

export function invoke<T = unknown>(cmd: string, args: Record<string, unknown> = {}): Promise<T> {
  // register_pane_delta_channel carries a non-serializable Channel instance and
  // means "start streaming this pane". Map it onto the subscribe-pane fan-out.
  if (cmd === 'register_pane_delta_channel') {
    const paneId = String(args.paneId ?? '');
    if (paneId) bridge.subscribePane(paneId);
    return Promise.resolve(undefined as T);
  }
  // The browser consumes raw PTY bytes (not postcard deltas), so the delta-mode
  // toggle is a no-op here.
  if (cmd === 'set_pane_delta_mode') {
    return Promise.resolve(undefined as T);
  }
  return bridge.invoke<T>(cmd, args);
}

/**
 * Map a host filesystem path to a URL the browser can fetch. The desktop uses
 * Tauri's `asset://` protocol; here we point at the host's authenticated
 * `/file` endpoint (added in server.rs).
 */
export function convertFileSrc(filePath: string, _protocol = 'asset'): string {
  const token = (() => {
    try {
      return localStorage.getItem(TOKEN_KEY) ?? '';
    } catch {
      return '';
    }
  })();
  const qs = new URLSearchParams({ path: filePath });
  if (token) qs.set('token', token);
  return `/file?${qs.toString()}`;
}

/** Tauri internals helper; unused by the WS event path but kept for import
 *  compatibility. Returns a callback id stub. */
export function transformCallback(callback?: (response: unknown) => void): number {
  void callback;
  return 0;
}
