import { describe, expect, it, vi } from 'vitest';
import { createAgentNavigator } from './agentNavigation';

describe('global Agent navigation', () => {
  it('returns the Host to A after B was already in flight and the user clicked A again', async () => {
    let finish!: () => void;
    let host = 'a', ui = 'a';
    const switchWorkspace = vi.fn(async id => {
      if (id === 'b') await new Promise<void>(resolve => { finish = resolve; });
      host = id; return true;
    });
    const navigate = createAgentNavigator({ switchWorkspace, navigate: id => { ui = id; }, isAlive: () => true,
      onError: error => { throw error; }, failureMessage: 'switch failed' });
    const first = navigate('b', 'pane-b'); await Promise.resolve();
    const last = navigate('a', 'pane-a'); finish(); await Promise.all([first, last]);
    expect(switchWorkspace.mock.calls.map(([id]) => id)).toEqual(['b', 'a']);
    expect({ host, ui }).toEqual({ host: 'a', ui: 'a' });
  });
  it('keeps errors observable and permits a later successful click', async () => {
    const errors = vi.fn(), commit = vi.fn();
    const switchWorkspace = vi.fn().mockResolvedValueOnce(false).mockResolvedValueOnce(true);
    const navigate = createAgentNavigator({ switchWorkspace, navigate: commit, isAlive: () => true, onError: errors, failureMessage: 'switch failed' });
    await navigate('a', 'pane-a'); expect(errors).toHaveBeenCalledTimes(1); expect(commit).not.toHaveBeenCalled();
    await navigate('b', 'pane-b'); expect(commit).toHaveBeenCalledWith('b', 'pane-b');
  });
});
