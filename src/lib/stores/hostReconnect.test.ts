import { beforeEach, describe, expect, it, vi } from 'vitest';
import { get } from 'svelte/store';

const tauriMocks = vi.hoisted(() => ({
  invoke: vi.fn(),
  isTauri: vi.fn(() => false),
}));
vi.mock('@tauri-apps/api/core', () => tauriMocks);

import {
  cancelHostReconnect,
  hostIsolationTasks,
  hostReconnectById,
  isolationBadge,
  parsePhaseMessage,
  runReconnectLoop,
  scheduleIsolationTask,
  sleepMsForAttempt,
  stepHostReconnect,
} from './hostReconnect';
import { outboundReconnectDelayMs } from '../../../packages/remote/src/shared/hosts/outboundReconnect';

beforeEach(() => {
  tauriMocks.invoke.mockReset();
  tauriMocks.isTauri.mockReset();
  tauriMocks.isTauri.mockReturnValue(false);
  hostReconnectById.set({});
  hostIsolationTasks.set({});
});

describe('hostReconnect pure helpers', () => {
  it('sleepMsForAttempt matches outbound schedule', () => {
    expect(sleepMsForAttempt(0)).toBe(outboundReconnectDelayMs(0));
    expect(sleepMsForAttempt(3)).toBe(1600);
    expect(sleepMsForAttempt(4)).toBeNull();
  });

  it('parses step_host_reconnect messages with attempt', () => {
    expect(parsePhaseMessage('phase=Waiting attempt=1 cancelled=0 next_delay_ms=200')).toEqual({
      phase: 'Waiting',
      nextDelayMs: 200,
      attempt: 1,
      cancelled: false,
    });
    expect(parsePhaseMessage('phase=Idle attempt=0 cancelled=0 terminal')).toEqual({
      phase: 'Idle',
      nextDelayMs: null,
      attempt: 0,
      cancelled: false,
    });
    expect(parsePhaseMessage('phase=Cancelled attempt=2 cancelled=1 terminal')).toEqual({
      phase: 'Cancelled',
      nextDelayMs: null,
      attempt: 2,
      cancelled: true,
    });
  });

  it('fails closed outside Tauri without invoking desktop IPC', async () => {
    await expect(stepHostReconnect('host-a', true)).resolves.toEqual({
      hostId: 'host-a',
      phase: 'Idle',
      attempt: 0,
      lastMessage: 'not-tauri',
      nextDelayMs: null,
    });
    expect(tauriMocks.invoke).not.toHaveBeenCalled();
  });

  it('projects server phases and updates the per-host stores', async () => {
    tauriMocks.isTauri.mockReturnValue(true);
    tauriMocks.invoke
      .mockResolvedValueOnce('phase=Waiting attempt=2 cancelled=0 next_delay_ms=10')
      .mockResolvedValueOnce('phase=Resubscribing cancelled=0');

    await expect(stepHostReconnect('host-a', false)).resolves.toMatchObject({
      phase: 'Waiting',
      attempt: 2,
      nextDelayMs: 10,
    });
    await expect(stepHostReconnect('host-a', true)).resolves.toMatchObject({
      phase: 'Resubscribing',
      attempt: 2,
      nextDelayMs: null,
    });
    expect(get(hostReconnectById)['host-a']).toMatchObject({ phase: 'Resubscribing' });
    expect(get(hostIsolationTasks)['host-a'].phase).toBe('Succeeded');
    expect(tauriMocks.invoke).toHaveBeenNthCalledWith(1, 'step_host_reconnect', {
      hostId: 'host-a',
      hostReachable: false,
    });
  });

  it('cancels a reconnect and marks its status only after backend confirmation', async () => {
    tauriMocks.isTauri.mockReturnValue(true);
    scheduleIsolationTask('host-a', ['pane-a', 'pane-a', '']);
    tauriMocks.invoke.mockResolvedValueOnce(false);
    await expect(cancelHostReconnect('host-a')).resolves.toBe(false);
    expect(get(hostReconnectById)).toEqual({});

    tauriMocks.invoke.mockResolvedValueOnce(true);
    await expect(cancelHostReconnect('host-a')).resolves.toBe(true);
    expect(get(hostReconnectById)['host-a']).toMatchObject({ phase: 'Cancelled' });
    expect(get(hostIsolationTasks)['host-a']).toMatchObject({
      phase: 'Cancelled',
      cancelled: true,
      attachedPaneIds: ['pane-a'],
    });
    expect(isolationBadge('host-a')).not.toBe('');
  });

  it('runs reconnect steps until a terminal server phase and caps sleeps', async () => {
    tauriMocks.isTauri.mockReturnValue(true);
    tauriMocks.invoke
      .mockResolvedValueOnce('phase=Waiting attempt=1 cancelled=0 next_delay_ms=1000')
      .mockResolvedValueOnce('phase=Succeeded attempt=1 cancelled=0 terminal');
    const sleep = vi.fn(async () => {});

    await expect(runReconnectLoop('host-a', {
      maxSteps: 3,
      isReachable: (step) => step > 0,
      sleep,
    })).resolves.toMatchObject({ phase: 'Succeeded', attempt: 1 });
    expect(sleep).toHaveBeenCalledWith(50);
    expect(tauriMocks.invoke).toHaveBeenCalledTimes(2);
  });
});
