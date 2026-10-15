// Cloud state is a Runtime projection; mounting a panel never owns a connection.
import { writable, get } from 'svelte/store';
import { invoke } from '@tauri-apps/api/core';
import { tr } from '$lib/i18n';
import { cloudHostOnline } from '$lib/stores/remoteStatus';
import * as cloudAuth from '@ridge/remote/shared/cloud/auth';
import { BASE_DOMAIN, cloudHttpScheme } from '@ridge/remote/shared/cloud/apiClient';
import type { CloudControllerSession, HostSignalState } from '@ridge/remote/shared/cloud/ridgeCloudProvider';
export const hostState = writable<HostSignalState>('offline');
export const cloudSessions = writable<CloudControllerSession[]>([]);
export const hostError = writable('');
let timer: ReturnType<typeof setInterval> | undefined;
let refreshing: Promise<void> | undefined;
export function refreshCloudHost(): Promise<void> {
  return refreshing ??= invoke<{ state: HostSignalState; error: string; sessions: CloudControllerSession[] }>('get_cloud_host_state')
    .then(snapshot => { hostState.set(snapshot.state); hostError.set(snapshot.error); cloudSessions.set(snapshot.sessions); cloudHostOnline.set(snapshot.state === 'online'); })
    .catch(error => { hostError.set(error instanceof Error ? error.message : tr('cloud.hostError')); })
    .finally(() => { refreshing = undefined; });
}
export function startCloudHostProjection(): () => void {
  if (!timer) { void refreshCloudHost(); timer = setInterval(() => void refreshCloudHost(), 2000); }
  return () => { if (timer) clearInterval(timer); timer = undefined; };
}
export async function goOnline(): Promise<void> {
  hostError.set('');
  const current = cloudAuth.snapshot();
  if (!current.deviceToken || !current.deviceName || !current.user?.username) { hostError.set(tr('cloud.errDeviceNotActivated')); return; }
  try {
    await invoke('cloud_host_online', { deviceToken: current.deviceToken, deviceName: current.deviceName, username: current.user.username, baseDomain: BASE_DOMAIN, plaintext: cloudHttpScheme(BASE_DOMAIN) === 'http' });
    await refreshCloudHost();
  } catch (error) { hostError.set(error instanceof Error ? error.message : tr('cloud.errConnectFailed')); }
}
export async function goOffline(): Promise<void> {
  try { await invoke('cloud_host_offline'); await refreshCloudHost(); }
  catch (error) { hostError.set(error instanceof Error ? error.message : tr('cloud.errConnectFailed')); }
}
export function kickController(cid: string): void { void invoke('cloud_host_kick', { cid }).then(refreshCloudHost).catch(error => hostError.set(String(error))); }
export function blacklistController(cid: string): void { void invoke('cloud_host_blacklist', { cid }).then(refreshCloudHost).catch(error => hostError.set(String(error))); }
export function isHostOnline(): boolean { return get(hostState) === 'online'; }
