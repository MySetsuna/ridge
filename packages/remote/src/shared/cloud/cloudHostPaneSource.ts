// Activation-scoped canonical terminal source. The bridge subscribes to one pane,
// forwards canonical packets from the Tauri event channel, and tears the host stream
// down in the same order in which it was started.

import type { PaneOutputSource, Unsubscribe } from './cloudHostBridge';
import terminalHello from './signaling/fixtures/terminal/hello.json';

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
 * from leaving a remote stream alive after its pane is destroyed.
 */
export function makeCloudHostPaneSource(deps: CloudHostPaneSourceDeps): PaneOutputSource {
  const log = deps.log ?? (() => {});
  return (
    paneId: string,
    workspaceId: string | undefined,
    onOutput: (raw: Uint8Array) => void,
    options?: Parameters<PaneOutputSource>[3],
  ): Unsubscribe => {
    let active = true;
    let unlisten: (() => void) | null = null;
    let subscribePromise: Promise<void> | null = null;
    let stopRequested = false;
    let stopSent = false;
    if (!workspaceId || !paneId || !Number.isSafeInteger(options?.activationId) || options!.activationId! <= 0) {
      throw new Error('canonical terminal binding required');
    }
    const eventName = `pane-terminal-${workspaceId}-${paneId}-${options!.activationId}`;
    const subscribeCommand = 'subscribe_pane_terminal';
    const unsubscribeCommand = 'unsubscribe_pane_terminal';
    const commandArgs = { paneId, workspaceId, activationId: options!.activationId };

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

    const fail = () => {
      if (!active) return;
      active = false;
      unlisten?.(); unlisten = null;
      requestStop();
      options?.onError?.();
    };

    void deps
      .listen<{ b64?: unknown }>(eventName, (event) => {
        if (!active) return;
        const b64 = event.payload?.b64;
        if (typeof b64 !== 'string' || b64.length > Math.ceil(terminalHello.maxFrameBytes / 3) * 4) return;
        const bytes = base64ToBytes(b64);
        if (bytes[0] !== 0x13 || bytes.length > terminalHello.maxFrameBytes) return;
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
            fail();
          },
        );
        if (stopRequested) void subscribePromise.then(sendStop);
      })
      .catch((e) => {
        log(`listen(${eventName}) failed`, e);
        fail();
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
