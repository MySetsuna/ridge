import type { SidebarProvider } from '../../shared/sidebar/types';

export type RemoteGitAction = 'stage' | 'unstage' | 'commit' | 'push';

export interface RemoteGitActionOptions {
  provider: SidebarProvider;
  action: RemoteGitAction;
  paths?: readonly string[];
  message?: string;
  setUpstream?: boolean;
  /** Confirmation is deliberately injected so browser/UI policy is testable. */
  confirm?: () => boolean | Promise<boolean>;
  signal?: AbortSignal;
}

export type RemoteGitActionResult =
  | { status: 'success' }
  | { status: 'cancelled'; reason: 'aborted' | 'rejected' }
  | { status: 'unavailable'; action: RemoteGitAction };

function isAborted(signal: AbortSignal | undefined): boolean {
  return !!signal?.aborted;
}

async function dispatchGitAction(options: RemoteGitActionOptions): Promise<void> {
  const { provider, action, signal } = options;
  const paths = [...options.paths ?? []];
  switch (action) {
    case 'stage':
      if (signal) await provider.gitStage?.(paths, signal);
      else await provider.gitStage?.(paths);
      break;
    case 'unstage':
      if (signal) await provider.gitUnstage?.(paths, signal);
      else await provider.gitUnstage?.(paths);
      break;
    case 'commit': {
      const message = options.message?.trim() ?? '';
      if (!message) throw new Error('Commit message cannot be empty');
      if (signal) await provider.gitCommit?.(message, false, signal);
      else await provider.gitCommit?.(message);
      break;
    }
    case 'push':
      if (signal) await provider.gitPush?.(options.setUpstream ?? false, signal);
      else await provider.gitPush?.(options.setUpstream ?? false);
      break;
  }
}

/**
 * One guarded mutation path for the mobile Git panel. Capability is checked
 * before confirmation, confirmation is awaited before any RPC, and callers
 * can abort before dispatch without touching the repository.
 */
export async function runRemoteGitAction(
  options: RemoteGitActionOptions,
): Promise<RemoteGitActionResult> {
  const { provider, action, signal } = options;
  const method = provider[`git${action[0].toUpperCase()}${action.slice(1)}` as 'gitStage'];
  if (typeof method !== 'function') return { status: 'unavailable', action };
  if (isAborted(signal)) return { status: 'cancelled', reason: 'aborted' };

  if (options.confirm && !(await options.confirm())) {
    return { status: 'cancelled', reason: 'rejected' };
  }
  if (isAborted(signal)) return { status: 'cancelled', reason: 'aborted' };

  await dispatchGitAction(options);
  if (isAborted(signal)) return { status: 'cancelled', reason: 'aborted' };
  return { status: 'success' };
}

export function hasRemoteGitWriteCapability(provider: SidebarProvider): boolean {
  return typeof provider.gitStage === 'function'
    && typeof provider.gitCommit === 'function'
    && typeof provider.gitPush === 'function';
}
