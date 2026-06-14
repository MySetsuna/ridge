import { writable } from 'svelte/store';

// Live "is the remote-control server actually running" flag.
//
// This reflects the BACKEND's real state (`get_remote_info().ready`, i.e. the
// port is bound AND remote is enabled) — NOT the persisted `remoteEnabled`
// setting. The setting can be `true` while the server isn't actually running
// (e.g. an auto-restart on launch that failed to bind a port). UI that should
// appear "only when remote control is started" (the per-pane refresh button)
// gates on this store so it stays hidden unless the server is truly up.
export const remoteRunning = writable(false);

// Live "is this desktop serving the public (cloud/WebRTC) remote" flag.
//
// The cloud host path (`cloudHostBridge` over the relay) is INDEPENDENT of the
// LAN remote server — it does not flip `remoteRunning` (which only reflects
// `get_remote_info().ready`). UI that must appear whenever ANY remote viewer can
// share a pane's PTY (the per-pane "re-lock size" button) also gates on this
// store. Set by RemotePanel's goOnline / goOffline / onHostState transitions.
export const cloudHostOnline = writable(false);

/** Query the backend for the real remote-server status and update the store. */
export async function refreshRemoteRunning(): Promise<boolean> {
  try {
    const { invoke } = await import('@tauri-apps/api/core');
    const info = await invoke<{ ready?: boolean }>('get_remote_info');
    const running = info?.ready === true;
    remoteRunning.set(running);
    return running;
  } catch {
    remoteRunning.set(false);
    return false;
  }
}
