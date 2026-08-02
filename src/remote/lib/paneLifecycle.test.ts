import { describe, expect, it, vi } from 'vitest';
import { detachPaneRefs } from './paneLifecycle';

describe('detachPaneRefs', () => {
	it('passes workspace-qualified keys so the remote kernel can be detached', () => {
		const detach = vi.fn();

		detachPaneRefs(
			[
				{ workspaceId: 'workspace-a', paneId: 'pane-a' },
				{ workspaceId: 'workspace-b', paneId: 'pane-b' },
			],
			detach,
		);

		expect(detach.mock.calls).toEqual([
			['workspace-a:pane-a'],
			['workspace-b:pane-b'],
		]);
	});

	it('is idempotent for duplicate pane references but keeps same-named panes separate', () => {
		const detach = vi.fn();

		detachPaneRefs(
			[
				{ workspaceId: 'workspace-a', paneId: 'pane-a' },
				{ workspaceId: 'workspace-a', paneId: 'pane-a' },
				{ workspaceId: 'workspace-b', paneId: 'pane-a' },
			],
			detach,
		);

		expect(detach.mock.calls).toEqual([
			['workspace-a:pane-a'],
			['workspace-b:pane-a'],
		]);
	});
});
