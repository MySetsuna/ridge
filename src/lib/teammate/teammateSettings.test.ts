import { beforeEach, expect, it, vi } from 'vitest';
import { get } from 'svelte/store';
const mocks = vi.hoisted(() => ({ invoke: vi.fn(), setSetting: vi.fn(), listen: vi.fn(), ready: vi.fn() }));
vi.mock('@tauri-apps/api/core', () => ({ invoke: mocks.invoke }));
vi.mock('@tauri-apps/api/event', () => ({ listen: mocks.listen }));
vi.mock('$lib/transport/tauriShim/bridge', () => ({ bridge: { onReady: mocks.ready } }));
vi.mock('$lib/stores/settings', async () => {
  const { writable } = await import('svelte/store');
  return { settingsStore: writable({ teammateHitlEnabled: true }), setSetting: mocks.setSetting };
});
beforeEach(() => {
  vi.resetModules();
  mocks.invoke.mockReset();
  mocks.setSetting.mockReset();
  mocks.listen.mockReset();
  mocks.ready.mockReset().mockReturnValue(() => {});
});
it('keeps the previous preference and exposes failed safety-gate acknowledgement', async () => {
  mocks.invoke.mockRejectedValue(new Error('Runtime unavailable'));
  const settings = await import('./teammateSettings');
  await settings.setTeammateHitlEnabled(false);
  expect(mocks.setSetting).not.toHaveBeenCalled();
  expect(get(settings.hitlSync)).toEqual({ pending: false, error: '安全审批未确认：Error: Runtime unavailable' });
});
it('serializes gate changes and persists each preference only after acknowledgement', async () => {
  let acknowledge!: () => void;
  mocks.invoke.mockImplementationOnce(() => new Promise<void>((resolve) => { acknowledge = resolve; }))
    .mockResolvedValueOnce(null);
  const settings = await import('./teammateSettings');
  const first = settings.setTeammateHitlEnabled(true);
  const second = settings.setTeammateHitlEnabled(false);
  await Promise.resolve();
  expect(mocks.invoke).toHaveBeenCalledTimes(1);
  expect(mocks.setSetting).not.toHaveBeenCalled();
  expect(get(settings.hitlSync).pending).toBe(true);
  acknowledge();
  await first;
  await second;
  expect(mocks.invoke.mock.calls).toEqual([
    ['set_hitl_enabled', { enabled: true }], ['set_hitl_enabled', { enabled: false }],
  ]);
  expect(mocks.setSetting.mock.calls).toEqual([
    ['teammateHitlEnabled', true], ['teammateHitlEnabled', false],
  ]);
  expect(get(settings.hitlSync)).toEqual({ pending: false, error: '' });
});
it('reports boot synchronization failure without discarding the saved preference', async () => {
  mocks.invoke.mockRejectedValue('method not found');
  const settings = await import('./teammateSettings');
  await settings.syncTeammateBackend();
  expect(mocks.invoke).toHaveBeenCalledWith('initialize_hitl_settings', { enabled: true });
  expect(mocks.setSetting).not.toHaveBeenCalled();
  expect(get(settings.hitlSync).error).toContain('method not found');
});

it('hydrates the shared preference instead of overwriting it with a new window preference', async () => {
  mocks.invoke.mockResolvedValue(false);
  const settings = await import('./teammateSettings');
  await settings.syncTeammateBackend();
  expect(mocks.invoke).toHaveBeenCalledWith('initialize_hitl_settings', { enabled: true });
  expect(mocks.setSetting).toHaveBeenCalledWith('teammateHitlEnabled', false);
  expect(get(settings.hitlSync).error).toBe('');
});

it('reloads shared settings on notifications and reconnect, and detaches listeners', async () => {
  const handlers = new Map<string, (message: { payload: unknown }) => void>();
  const stop = vi.fn();
  mocks.ready.mockReturnValue(stop);
  mocks.listen.mockImplementation((event, handler) => {
    handlers.set(event, handler);
    return Promise.resolve(stop);
  });
  mocks.invoke.mockResolvedValueOnce(true).mockResolvedValue(false);
  const settings = await import('./teammateSettings');
  const dispose = settings.initTeammateBoot();
  await vi.waitFor(() => expect(mocks.setSetting).toHaveBeenCalledWith('teammateHitlEnabled', true));
  handlers.get('teammate://hitl-settings-changed')!({ payload: { enabled: true } });
  await vi.waitFor(() => expect(mocks.setSetting).toHaveBeenLastCalledWith('teammateHitlEnabled', false));
  expect(mocks.invoke).toHaveBeenLastCalledWith('get_hitl_enabled');
  const calls = mocks.invoke.mock.calls.length;
  mocks.ready.mock.calls[0][0]();
  await vi.waitFor(() => expect(mocks.invoke).toHaveBeenCalledTimes(calls + 1));
  dispose();
  expect(stop).toHaveBeenCalledTimes(2);
});
it('detaches late listeners and ignores responses after the window disposes', async () => {
  const stops = [vi.fn(), vi.fn()];
  mocks.ready.mockReturnValue(stops[1]);
  const registrations: ((stop: () => void) => void)[] = [];
  mocks.listen.mockImplementation(() => new Promise(resolve => registrations.push(resolve)));
  let reply!: (value: boolean) => void;
  mocks.invoke.mockImplementation(() => new Promise<boolean>(resolve => { reply = resolve; }));
  const settings = await import('./teammateSettings');
  const dispose = settings.initTeammateBoot();
  await Promise.resolve();
  dispose();
  registrations.forEach((resolve, i) => resolve(stops[i]));
  reply(true);
  await vi.waitFor(() => expect(stops[1]).toHaveBeenCalledOnce());
  expect(stops[0]).toHaveBeenCalledOnce();
  expect(mocks.setSetting).not.toHaveBeenCalled();
  expect(get(settings.hitlSync).pending).toBe(false);
});
