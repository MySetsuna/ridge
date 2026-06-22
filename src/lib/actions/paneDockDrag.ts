import { get } from 'svelte/store';
import {
	paneDragSourceId,
	paneDockHover,
	dragHoverWorkspaceId,
	dockPane,
	switchWorkspace,
	activePaneId,
	activeWorkspaceId,
} from '$lib/stores/paneTree';
import { resolveDockTarget, passedDragThreshold } from '$lib/terminal/paneDockResolve';
import { alertDialog } from '$lib/components/RidgeDialog.svelte';
import { tr } from '$lib/i18n';

const HOVER_SWITCH_MS = 250;

interface Params {
	paneId: string;
}

export function paneDockDrag(node: HTMLElement, params: Params) {
	let paneId = params.paneId;
	let startX = 0,
		startY = 0,
		dragging = false;
	let pointerId: number | null = null;
	let hoverTimer: ReturnType<typeof setTimeout> | null = null;
	let hoverTabWsId: string | null = null;

	function clearHover() {
		if (hoverTimer !== null) {
			clearTimeout(hoverTimer);
			hoverTimer = null;
		}
		hoverTabWsId = null;
		dragHoverWorkspaceId.set(null);
	}

	function onPointerDown(e: PointerEvent) {
		if (e.button !== 0) return;
		pointerId = e.pointerId;
		startX = e.clientX;
		startY = e.clientY;
		dragging = false;
		node.setPointerCapture(e.pointerId);
		node.addEventListener('pointermove', onPointerMove);
		node.addEventListener('pointerup', onPointerUp);
		node.addEventListener('pointercancel', onPointerCancel);
	}

	function onPointerMove(e: PointerEvent) {
		if (pointerId === null) return;
		if (!dragging) {
			if (!passedDragThreshold(startX, startY, e.clientX, e.clientY)) return;
			dragging = true;
			paneDragSourceId.set(paneId);
		}
		const el = document.elementFromPoint(e.clientX, e.clientY);
		const tab = (el as HTMLElement | null)?.closest('[data-ws-tab-id]') as HTMLElement | null;
		const tabWsId = tab?.getAttribute('data-ws-tab-id') ?? null;
		if (tabWsId && tabWsId !== get(activeWorkspaceId)) {
			paneDockHover.set(null);
			if (hoverTabWsId !== tabWsId) {
				clearHover();
				hoverTabWsId = tabWsId;
				dragHoverWorkspaceId.set(tabWsId);
				hoverTimer = setTimeout(() => {
					if (get(paneDragSourceId) === paneId && hoverTabWsId === tabWsId) {
						void switchWorkspace(tabWsId);
					}
					hoverTimer = null;
				}, HOVER_SWITCH_MS);
			}
			return;
		}
		clearHover();
		paneDockHover.set(resolveDockTarget(el, paneId, e.clientX, e.clientY));
	}

	async function finish(commit: boolean) {
		node.removeEventListener('pointermove', onPointerMove);
		node.removeEventListener('pointerup', onPointerUp);
		node.removeEventListener('pointercancel', onPointerCancel);
		if (pointerId !== null && node.hasPointerCapture(pointerId)) node.releasePointerCapture(pointerId);
		pointerId = null;
		const target = get(paneDockHover);
		const wasDragging = dragging;
		dragging = false;
		clearHover();
		paneDragSourceId.set(null);
		paneDockHover.set(null);
		if (!wasDragging) {
			activePaneId.set(paneId);
			return;
		}
		if (commit && target && target.paneId !== paneId) {
			try {
				await dockPane(paneId, target.paneId, target.region);
			} catch (err) {
				console.error('dockPane failed', err);
				await alertDialog({
					title: tr('workspace.opFailed'),
					message: err instanceof Error ? err.message : String(err),
					danger: true,
				});
			}
		}
	}

	function onPointerUp() {
		void finish(true);
	}
	function onPointerCancel() {
		void finish(false);
	}

	node.addEventListener('pointerdown', onPointerDown);
	return {
		update(p: Params) {
			paneId = p.paneId;
		},
		destroy() {
			node.removeEventListener('pointerdown', onPointerDown);
			// 若在拖拽中途被卸载（如 pane 关闭），释放 capture + 清监听器/拖拽态，
			// 避免 WebView2 下 capture 泄漏致后续鼠标无响应。
			if (pointerId !== null && node.hasPointerCapture(pointerId)) {
				node.releasePointerCapture(pointerId);
			}
			node.removeEventListener('pointermove', onPointerMove);
			node.removeEventListener('pointerup', onPointerUp);
			node.removeEventListener('pointercancel', onPointerCancel);
			pointerId = null;
			paneDragSourceId.set(null);
			paneDockHover.set(null);
			clearHover();
		},
	};
}
