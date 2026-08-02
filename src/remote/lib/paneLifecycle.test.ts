import { describe, expect, it, vi } from 'vitest';
import { detachPaneRefs } from './paneLifecycle';

describe('detachPaneRefs', () => {
	it('passes bare pane ids to the manager, not workspace-qualified cache keys', () => {
		const detach = vi.fn();

		detachPaneRefs(
			[
				{ workspaceId: 'workspace-a', paneId: 'pane-a' },
				{ workspaceId: 'workspace-b', paneId: 'pane-b' },
			],
			detach,
		);

		expect(detach.mock.calls).toEqual([['pane-a'], ['pane-b']]);
	});

	it('is idempotent for duplicate pane references', () => {
		const detach = vi.fn();

		detachPaneRefs(
			[
				{ workspaceId: 'workspace-a', paneId: 'pane-a' },
				{ workspaceId: 'workspace-a', paneId: 'pane-a' },
			],
			detach,
		);

		expect(detach).toHaveBeenCalledOnce();
		expect(detach).toHaveBeenCalledWith('pane-a');
	});
});
