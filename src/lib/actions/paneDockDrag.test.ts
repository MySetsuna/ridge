import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { get } from 'svelte/store';
import {
	activePaneId,
	activeWorkspaceId,
	dragHoverWorkspaceId,
	paneDockHover,
	paneDragSourceId,
} from '$lib/stores/paneTree';
import { paneDockDrag } from './paneDockDrag';

const mocks = vi.hoisted(() => ({
	dockPane: vi.fn().mockResolvedValue(undefined),
	switchWorkspace: vi.fn().mockResolvedValue(undefined),
	resolveDockTarget: vi.fn(),
	alertDialog: vi.fn().mockResolvedValue(undefined),
}));

vi.mock('$lib/stores/paneTree', async () => ({
	...(await vi.importActual<typeof import('$lib/stores/paneTree')>('$lib/stores/paneTree')),
	dockPane: mocks.dockPane,
	switchWorkspace: mocks.switchWorkspace,
}));
vi.mock('@ridge/remote/shared/terminal/paneDockResolve', () => ({
	passedDragThreshold: (sx: number, sy: number, x: number, y: number) => Math.hypot(x - sx, y - sy) >= 6,
	resolveDockTarget: mocks.resolveDockTarget,
}));
vi.mock('$lib/components/RidgeDialog.svelte', () => ({ alertDialog: mocks.alertDialog }));
vi.mock('$lib/i18n', () => ({ tr: (key: string) => key }));

function nodeFixture() {
	const listeners = new Map<string, Set<(event: any) => void>>();
	let captured: number | null = null;
	const node = {
		addEventListener: vi.fn((type: string, listener: (event: any) => void) => {
			if (!listeners.has(type)) listeners.set(type, new Set());
			listeners.get(type)!.add(listener);
		}),
		removeEventListener: vi.fn((type: string, listener: (event: any) => void) => listeners.get(type)?.delete(listener)),
		setPointerCapture: vi.fn((id: number) => { captured = id; }),
		hasPointerCapture: vi.fn((id: number) => captured === id),
		releasePointerCapture: vi.fn((id: number) => { if (captured === id) captured = null; }),
		emit(type: string, event: any = {}) {
			for (const listener of listeners.get(type) ?? []) listener(event);
		},
	};
	return node as typeof node & HTMLElement;
}

beforeEach(() => {
	vi.useFakeTimers();
	vi.clearAllMocks();
	activePaneId.set('');
	activeWorkspaceId.set('ws-1');
	dragHoverWorkspaceId.set(null);
	paneDragSourceId.set(null);
	paneDockHover.set(null);
	vi.stubGlobal('document', { elementFromPoint: vi.fn(() => null) });
});

afterEach(() => {
	vi.useRealTimers();
	vi.unstubAllGlobals();
});

describe('paneDockDrag lifecycle', () => {
	it('activates a pane without docking when pointer movement stays below threshold', async () => {
		const node = nodeFixture();
		const action = paneDockDrag(node, { paneId: 'pane-a' });
		node.emit('pointerdown', { button: 0, pointerId: 1, clientX: 10, clientY: 10 });
		node.emit('pointermove', { clientX: 12, clientY: 12 });
		node.emit('pointerup');
		await Promise.resolve();
		expect(get(activePaneId)).toBe('pane-a');
		expect(mocks.dockPane).not.toHaveBeenCalled();
		action.destroy();
		expect(get(paneDragSourceId)).toBeNull();
	});

	it('resolves and commits a dock target, then cleans pointer capture', async () => {
		const node = nodeFixture();
		const targetElement = { closest: vi.fn(() => null) };
		(document.elementFromPoint as ReturnType<typeof vi.fn>).mockReturnValue(targetElement);
		mocks.resolveDockTarget.mockReturnValue({ paneId: 'pane-b', region: 'right' });
		const action = paneDockDrag(node, { paneId: 'pane-a' });
		node.emit('pointerdown', { button: 0, pointerId: 2, clientX: 10, clientY: 10 });
		node.emit('pointermove', { clientX: 20, clientY: 10 });
		expect(get(paneDragSourceId)).toBe('pane-a');
		node.emit('pointerup');
		await Promise.resolve();
		expect(mocks.dockPane).toHaveBeenCalledWith('pane-a', 'pane-b', 'right');
		expect(node.releasePointerCapture).toHaveBeenCalledWith(2);
		action.destroy();
	});

	it('switches workspace after hovering a foreign workspace tab', async () => {
		const node = nodeFixture();
		const tab = { getAttribute: vi.fn(() => 'ws-2') };
		const element = { closest: vi.fn(() => tab) };
		(document.elementFromPoint as ReturnType<typeof vi.fn>).mockReturnValue(element);
		const action = paneDockDrag(node, { paneId: 'pane-a' });
		node.emit('pointerdown', { button: 0, pointerId: 3, clientX: 0, clientY: 0 });
		node.emit('pointermove', { clientX: 10, clientY: 0 });
		expect(get(dragHoverWorkspaceId)).toBe('ws-2');
		vi.advanceTimersByTime(250);
		await Promise.resolve();
		expect(mocks.switchWorkspace).toHaveBeenCalledWith('ws-2');
		action.destroy();
		expect(get(dragHoverWorkspaceId)).toBeNull();
	});
});
