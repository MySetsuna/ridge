import { paneRefKey, type PaneRef } from '@ridge/remote';

/**
 * Release remote pane resources through the manager's pane-key API.
 *
 * Remote UI and `TerminalManager` both use the workspace-qualified key. Pane
 * ids are only unique inside one workspace; stripping `workspaceId` here can
 * silently skip teardown (or tear down the wrong same-named pane).
 */
export function detachPaneRefs(
	refs: readonly PaneRef[],
	detach: (paneKey: string) => void,
): void {
	const seen = new Set<string>();
	for (const ref of refs) {
		if (!ref.paneId) continue;
		const key = paneRefKey(ref);
		if (seen.has(key)) continue;
		seen.add(key);
		detach(key);
	}
}
