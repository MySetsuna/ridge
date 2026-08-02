import { describe, expect, it, vi } from 'vitest';
import type { SidebarProvider } from '../../shared/sidebar/types';
import { hasRemoteGitWriteCapability, runRemoteGitAction } from './remoteGitActions';

function provider(overrides: Partial<SidebarProvider> = {}): SidebarProvider {
  return {
    listDir: vi.fn(),
    gitStatus: vi.fn(),
    search: vi.fn(),
    readFile: vi.fn(),
    writeFile: vi.fn(),
    gitDiff: vi.fn(),
    gitStage: vi.fn(),
    gitCommit: vi.fn(),
    gitPush: vi.fn(),
    ...overrides,
  } as unknown as SidebarProvider;
}

describe('remote Git action contract', () => {
  it('requires all write methods before enabling the mobile controls', () => {
    expect(hasRemoteGitWriteCapability(provider())).toBe(true);
    expect(hasRemoteGitWriteCapability(provider({ gitPush: undefined }))).toBe(false);
  });

  it('confirms before dispatch and treats rejection as cancellation', async () => {
    const gitCommit = vi.fn();
    const result = await runRemoteGitAction({
      provider: provider({ gitCommit }),
      action: 'commit',
      message: 'message',
      confirm: () => false,
    });
    expect(result).toEqual({ status: 'cancelled', reason: 'rejected' });
    expect(gitCommit).not.toHaveBeenCalled();
  });

  it('trims commit messages and dispatches exact query-backed mutation', async () => {
    const gitCommit = vi.fn(async () => undefined);
    const result = await runRemoteGitAction({
      provider: provider({ gitCommit }),
      action: 'commit',
      message: '  fix remote git  ',
      confirm: () => true,
    });
    expect(result).toEqual({ status: 'success' });
    expect(gitCommit).toHaveBeenCalledWith('fix remote git');
  });

  it('forwards the cancellation signal to a remote mutation', async () => {
    const controller = new AbortController();
    const gitPush = vi.fn(async (_setUpstream: boolean, signal?: AbortSignal) => {
      expect(signal).toBe(controller.signal);
    });
    const result = await runRemoteGitAction({
      provider: provider({ gitPush }),
      action: 'push',
      signal: controller.signal,
    });
    expect(result).toEqual({ status: 'success' });
    expect(gitPush).toHaveBeenCalledWith(false, controller.signal);
  });

  it('does not dispatch after an abort and reports unavailable capabilities', async () => {
    const controller = new AbortController();
    controller.abort();
    const gitPush = vi.fn();
    await expect(runRemoteGitAction({
      provider: provider({ gitPush }),
      action: 'push',
      signal: controller.signal,
    })).resolves.toEqual({ status: 'cancelled', reason: 'aborted' });
    expect(gitPush).not.toHaveBeenCalled();

    await expect(runRemoteGitAction({
      provider: provider({ gitPush: undefined }),
      action: 'push',
    })).resolves.toEqual({ status: 'unavailable', action: 'push' });
  });
});
