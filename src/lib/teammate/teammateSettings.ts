/** HITL preferences commit only after the Runtime acknowledges the safety gate. */
import { get, writable } from 'svelte/store';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { bridge } from '$lib/transport/tauriShim/bridge';
import { settingsStore, setSetting, type UserSettings } from '$lib/stores/settings';

export const hitlSync = writable({ pending: false, error: '' });
let pending: Promise<void> = Promise.resolve();
let queued = 0;

function syncHitl(operation: 'set' | 'initialize' | 'read', enabled?: boolean, active = () => true): Promise<void> {
  queued++;
  hitlSync.set({ pending: true, error: '' });
  pending = pending.then(async () => {
    try {
      if (!active()) return;
      const effective = operation === 'read'
        ? await invoke<boolean>('get_hitl_enabled')
        : operation === 'initialize'
          ? await invoke<boolean>('initialize_hitl_settings', { enabled })
          : (await invoke('set_hitl_enabled', { enabled }), enabled);
      if (!active()) return;
      if (typeof effective !== 'boolean') throw new Error('Invalid HITL settings response');
      setSetting('teammateHitlEnabled', effective);
      hitlSync.set({ pending: queued > 1, error: '' });
    } catch (error) {
      if (active()) hitlSync.set({ pending: queued > 1, error: `安全审批未确认：${String(error)}` });
    } finally {
      queued--;
      hitlSync.update(status => ({ ...status, pending: queued > 0 }));
    }
  });
  return pending;
}

export function syncTeammateBackend(s: UserSettings = get(settingsStore)): Promise<void> {
  return syncHitl('initialize', s.teammateHitlEnabled);
}

/** The visibility switch never changes the safety gate. */
export function setTeammateEnabled(enabled: boolean): void {
  setSetting('teammateEnabled', enabled);
}

export function setTeammateHitlEnabled(enabled: boolean): Promise<void> {
  return syncHitl('set', enabled);
}

export function initTeammateBoot(): () => void {
  let active = true;
  const stops: (() => void)[] = [];
  for (const event of ['teammate://hitl-settings-changed']) {
    void listen<{ initial?: boolean }>(event, () => {
      void syncHitl('read', undefined, () => active);
    }).then(stop => { if (active) stops.push(stop); else stop(); }).catch(error => {
      if (active) hitlSync.set({ pending: queued > 0, error: `安全审批未确认：${String(error)}` });
    });
  }
  stops.push(bridge.onReady(() => { void syncHitl('read', undefined, () => active); }));
  void syncHitl('initialize', get(settingsStore).teammateHitlEnabled, () => active);
  return () => { active = false; for (const stop of stops) stop(); };
}
