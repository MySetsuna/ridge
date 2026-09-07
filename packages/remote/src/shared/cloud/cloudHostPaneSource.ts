// Host-side cloud pane PTY byte source. The bridge subscribes to one pane,
// forwards raw bytes from the Tauri event channel, and tears the host stream
// down in the same order in which it was started.

import type { PaneOutputSource, Unsubscribe } from './cloudHostBridge';

/** Minimal Tauri event.listen shape, kept injectable for deterministic tests. */
export type ListenFn = <T = unknown>(
  event: string,
  handler: (event: { payload: T }) => void,
) => Promise<() => void>;

/** Minimal Tauri invoke shape, kept injectable for deterministic tests. */
export type InvokeFn = (cmd: string, args?: Record<string, unknown>) => Promise<unknown>;

export interface CloudHostPaneSourceDeps {
  invoke: InvokeFn;
  listen: ListenFn;
  log?: (message: string, detail?: unknown) => void;
}

/** Decode an event payload without allowing malformed input to break the stream. */
export function base64ToBytes(b64: string): Uint8Array {
  try {
    const bin = atob(b64);
    const out = new Uint8Array(bin.length);
    for (let i = 0; i < bin.length; i++) out[i] = bin.codePointAt(i) ?? 0;
    return out;
  } catch {
    return new Uint8Array(0);
  }
}

/**
 * Build the host PTY source used by CloudHostBridge. Listener registration is
 * completed before subscribing so the first output frame cannot be lost.
 * Unsubscribe is serialized after subscribe, preventing a close/reconnect race
 * from leaving a remote raw stream alive after its pane is destroyed.
 */
export function makeCloudHostPaneSource(deps: CloudHostPaneSourceDeps): PaneOutputSource {
  const log = deps.log ?? (() => {});
  return (
    paneId: string,
    workspaceId: string | undefined,
    onOutput: (raw: Uint8Array) => void,
    options?: { activationId?: number },
  ): Unsubscribe => {
    let active = true;
    let unlisten: (() => void) | null = null;
    let subscribePromise: Promise<void> | null = null;
    let stopRequested = false;
    let stopSent = false;
    const semantic = options?.activationId !== undefined && workspaceId !== undefined;
    const eventName = semantic
      ? `pane-terminal-v2-${workspaceId}-${paneId}-${options.activationId}`
      : workspaceId
        ? `pane-raw-${workspaceId}-${paneId}`
        : `pane-raw-${paneId}`;
    const subscribeCommand = semantic ? 'subscribe_pane_terminal_v2' : 'subscribe_pane_raw';
    const unsubscribeCommand = semantic ? 'unsubscribe_pane_terminal_v2' : 'unsubscribe_pane_raw';
    const commandArgs = semantic
      ? { paneId, workspaceId, activationId: options!.activationId }
      : { paneId, workspaceId };

    const sendStop = () => {
      if (stopSent || subscribePromise === null) return;
      stopSent = true;
      void Promise.resolve()
        .then(() => deps.invoke(unsubscribeCommand, commandArgs))
        .catch((e) => log(`${unsubscribeCommand} failed`, e));
    };
    const requestStop = () => {
      stopRequested = true;
      if (subscribePromise !== null) void subscribePromise.then(sendStop);
    };

    void deps
      .listen<{ b64?: unknown }>(eventName, (event) => {
        if (!active) return;
        const b64 = event.payload?.b64;
        if (typeof b64 !== 'string') return;
        const bytes = base64ToBytes(b64);
        if (bytes.length === 0) return;
        try {
          onOutput(bytes);
        } catch (e) {
          log('pane output callback failed', e);
        }
      })
      .then((u) => {
        if (!active) {
          u();
          return;
        }
        unlisten = u;
        const started = deps.invoke(subscribeCommand, commandArgs);
        // Always settle bookkeeping, even when the host rejects.
        subscribePromise = Promise.resolve(started).then(
          () => undefined,
          (e) => {
            log(`${subscribeCommand} failed`, e);
          },
        );
        if (stopRequested) void subscribePromise.then(sendStop);
      })
      .catch((e) => {
        log(`listen(${eventName}) failed`, e);
        requestStop();
      });

    return () => {
      active = false;
      if (unlisten) {
        unlisten();
        unlisten = null;
      }
      requestStop();
    };
  };
}
