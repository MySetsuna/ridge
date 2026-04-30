// src/lib/utils/anchorRect.ts
//
// Convert an anchor element + placement into a fixed-position CSS string for
// portaled popups. Right/bottom edges clamp to ≥8px from viewport so popups
// never escape off-screen.

export type Placement = 'bottom-end' | 'bottom-start' | 'top-end' | 'top-start';

export function popupStyleFor(
	anchor: HTMLElement,
	placement: Placement = 'bottom-end',
	gap: number = 4,
): string {
	const r = anchor.getBoundingClientRect();
	const vw = window.innerWidth;
	const vh = window.innerHeight;
	switch (placement) {
		case 'bottom-end':
			return `position:fixed;top:${r.bottom + gap}px;right:${Math.max(8, vw - r.right)}px`;
		case 'bottom-start':
			return `position:fixed;top:${r.bottom + gap}px;left:${Math.max(8, r.left)}px`;
		case 'top-end':
			return `position:fixed;bottom:${Math.max(8, vh - r.top + gap)}px;right:${Math.max(8, vw - r.right)}px`;
		case 'top-start':
			return `position:fixed;bottom:${Math.max(8, vh - r.top + gap)}px;left:${Math.max(8, r.left)}px`;
	}
}
