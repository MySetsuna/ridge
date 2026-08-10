import { afterEach, describe, expect, it, vi } from 'vitest';
import { get } from 'svelte/store';
import { contextMenu, isResizeInProgress } from './contextMenu';
import { splitResizeUiState } from './paneTree';

afterEach(() => {
	vi.useRealTimers();
	contextMenu.hide();
	splitResizeUiState.set({ phase: 'idle' });
	Reflect.deleteProperty(globalThis, 'document');
});

describe('context menu state and focus lifecycle', () => {
	it('shows, positions, and hides a menu in SSR-safe mode', () => {
		const item = { id: 'copy', label: 'Copy' };
		contextMenu.show(12, 24, [item], 'editor', 'pane-1', 'workspace-1');
		expect(get(contextMenu)).toMatchObject({
			visible: true,
			x: 12,
			y: 24,
			items: [item],
			target: 'editor',
			paneId: 'pane-1',
			workspaceId: 'workspace-1',
			previousActiveElement: null,
		});

		contextMenu.updatePosition(40, 50);
		expect(get(contextMenu)).toMatchObject({ x: 40, y: 50, visible: true });
		contextMenu.hide();
		expect(get(contextMenu)).toMatchObject({ visible: false, previousActiveElement: undefined });
	});

	it('restores the previous focus only when focus returned to body', () => {
		vi.useFakeTimers();
		const body = {} as unknown as Element;
		const previousFocus = vi.fn();
		const previous = { isConnected: true, focus: previousFocus };
		const documentState: { activeElement: Element | null; body: Element; documentElement: object } = {
			activeElement: previous as unknown as Element,
			body,
			documentElement: {},
		};
		Object.defineProperty(globalThis, 'document', { configurable: true, value: documentState });

		contextMenu.show(0, 0, [], 'terminal');
		documentState.activeElement = body;
		contextMenu.hide();
		vi.runAllTimers();
		expect(previousFocus).toHaveBeenCalledOnce();

		previousFocus.mockClear();
		contextMenu.show(0, 0, [], 'terminal');
		documentState.activeElement = { id: 'new-input' } as unknown as Element;
		contextMenu.hide();
		vi.runAllTimers();
		expect(previousFocus).not.toHaveBeenCalled();
	});

	it('recognizes every active split-resize phase', () => {
		expect(isResizeInProgress()).toBe(false);
		splitResizeUiState.set({
			phase: 'pending',
			primary: { splitPath: [], splitterIndex: 0, axis: 'x', basisPx: 100 },
			orthogonals: [],
			sameAxisCandidates: [],
			pointer: { x: 0, y: 0 },
			snapState: null,
		});
		expect(isResizeInProgress()).toBe(true);
		splitResizeUiState.set({
			phase: 'drag',
			pointer: { x: 0, y: 0 },
			dragStart: { x: 0, y: 0 },
			snapshots: [],
			pendingUpdates: [],
			snapState: null,
			sameAxisAttractors: [],
			pxAnchors: [],
		});
		expect(isResizeInProgress()).toBe(true);
	});
});
