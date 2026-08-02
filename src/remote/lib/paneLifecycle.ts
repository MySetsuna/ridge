import type { PaneRef } from '@ridge/remote';

/**
 * Release remote pane resources through the manager's pane-id API.
 *
 * Remote UI bookkeeping is keyed by `workspaceId:paneId`, while
 * `TerminalManager` owns kernels by the bare `paneId`. Keeping this
 * conversion here prevents a workspace-qualified cache key from silently
 * skipping the actual kernel teardown.
 */
export function detachPaneRefs(
	refs: readonly PaneRef[],
	detach: (paneId: string) => void,
): void {
	const seen = new Set<string>();
	for (const ref of refs) {
		if (!ref.paneId || seen.has(ref.paneId)) continue;
		seen.add(ref.paneId);
		detach(ref.paneId);
	}
}
