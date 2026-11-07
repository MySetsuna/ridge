import { beforeEach, describe, expect, it, vi } from 'vitest';
import { startPaneSession } from './paneStartup';

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock('@tauri-apps/api/core', () => ({ invoke }));
const pane = { workspaceId: 'workspace', paneId: 'pane' };

beforeEach(() => { invoke.mockReset().mockResolvedValue(undefined); });

describe('pane session startup recovery', () => {
  it('subscribes before activation and uses the final fitted grid', async () => {
    let cols = 80;
    const prepareStream = vi.fn(async () => {
      expect(invoke).toHaveBeenCalledTimes(1);
      cols = 120;
    });
    await startPaneSession({ pane, shell: null, grid: () => ({ rows: 24, cols }), prepareStream, alive: () => true });
    expect(prepareStream).toHaveBeenCalledOnce();
    expect(invoke.mock.calls).toEqual([
      ['create_pane', { ...pane, shell: null, rows: 24, cols: 80 }],
      ['activate_pane_pty', { ...pane, rows: 24, cols: 120 }],
    ]);
  });

  it('retries an unknown create outcome with the same explicit identity', async () => {
    invoke.mockRejectedValueOnce(new Error('RPC timeout'));
    const prepareStream = vi.fn().mockResolvedValue(undefined);
    const options = { pane, shell: null, grid: () => ({ rows: 24, cols: 80 }), prepareStream, alive: () => true };
    await expect(startPaneSession(options)).rejects.toThrow('RPC timeout');
    expect(prepareStream).not.toHaveBeenCalled();
    await startPaneSession(options);
    expect(invoke.mock.calls.map(([method]) => method)).toEqual(['create_pane', 'create_pane', 'activate_pane_pty']);
    expect(invoke.mock.calls[0][1]).toEqual(invoke.mock.calls[1][1]);
  });

  it('does not activate after stream registration fails', async () => {
    await expect(startPaneSession({ pane, shell: null, grid: () => ({ rows: 24, cols: 80 }), prepareStream: async () => { throw new Error('stream unavailable'); }, alive: () => true })).rejects.toThrow('stream unavailable');
    expect(invoke).toHaveBeenCalledTimes(1);
  });

  it.each(['create', 'stream'])('stops after a pane is retired during %s', async stage => {
    let alive = true;
    if (stage === 'create') invoke.mockImplementationOnce(async () => { alive = false; });
    const prepareStream = vi.fn(async () => { alive = false; });
    await startPaneSession({ pane, shell: null, grid: () => ({ rows: 24, cols: 80 }), prepareStream, alive: () => alive });
    expect(invoke).toHaveBeenCalledTimes(1);
    expect(prepareStream).toHaveBeenCalledTimes(stage === 'create' ? 0 : 1);
  });
});
