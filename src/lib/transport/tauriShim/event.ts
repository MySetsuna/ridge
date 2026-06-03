// src/lib/transport/tauriShim/event.ts
//
// Browser stand-in for `@tauri-apps/api/event`. Aliased in by the web-remote
// Vite build. `listen()` registers interest with the bridge; the host pushes
// matching events over the WS (see server.rs event taps). Tauri's `listen` is
// async (returns Promise<UnlistenFn>), so we mirror that signature exactly.

import { bridge, type TauriEvent, type EventCallback, type UnlistenFn } from './bridge';

export type { UnlistenFn, EventCallback };
export type Event<T> = TauriEvent<T>;

export async function listen<T>(
  event: string,
  handler: EventCallback<T>,
): Promise<UnlistenFn> {
  return bridge.listen<T>(event, handler);
}

export async function once<T>(
  event: string,
  handler: EventCallback<T>,
): Promise<UnlistenFn> {
  const unlisten = bridge.listen<T>(event, (e) => {
    unlisten();
    handler(e);
  });
  return unlisten;
}

/** No desktop-UI emit sites exist today; provided for import compatibility. */
export async function emit(_event: string, _payload?: unknown): Promise<void> {
  /* no-op: the browser never originates host events */
}

export async function emitTo(
  _target: string,
  _event: string,
  _payload?: unknown,
): Promise<void> {
  /* no-op */
}
