/**
 * §1.32 (2026-05-20) — pure helper for placing the shell-history popup.
 *
 * Extracted from `RidgePane.svelte`'s inline ArrowUp/ArrowDown handler
 * so we can lock three contracts in unit tests:
 *
 *   1. When `manager.inputAnchorPixelPosition` returns null (cell
 *      metrics not ready, pane unknown, alt-screen race etc.), this
 *      function returns null. The caller MUST close / refuse to open
 *      the popup in that case — placing it at viewport (0, 0) with
 *      a hardcoded `cellH: 20` fallback (the previous bug) just
 *      stranded the menu in the top-left corner.
 *   2. The popup's `inputH` field is the live `cellH` from the
 *      anchor — not a constant. Different fonts / DPR settings give
 *      different cell heights; the popup's vertical-flip threshold
 *      uses this value, so passing the wrong one offsets the menu
 *      by an entire row.
 *   3. The function is pure — call it again with new inputs to get
 *      a new result. The popup-on-resize and rapid-re-invoke paths
 *      both rely on this; the test suite verifies no implicit
 *      memoisation creeps in.
 */

/** Pixel anchor from `manager.inputAnchorPixelPosition(paneId)`.
 *  `x` / `y` are relative to the pane container's content box. */
export interface PopupAnchor {
	x: number;
	y: number;
	cellH: number;
}

/** Minimal view of a `DOMRect` — only the fields we read. Tests can
 *  pass plain objects without constructing real DOMRects. */
export interface ContainerRect {
	left: number;
	top: number;
}

/** Viewport-absolute position the popup component consumes. */
export interface PopupPosition {
	x: number;
	y: number;
	inputH: number;
}

/**
 * Returns the viewport-absolute position to anchor the popup at, or
 * `null` if the anchor itself is unavailable.
 *
 * On `null` the caller should NOT open / should immediately close
 * the popup — see Bug #2 in the plan file.
 */
export function computePopupPosition(
	anchor: PopupAnchor | null,
	rect: ContainerRect
): PopupPosition | null {
	if (!anchor) return null;
	return {
		x: rect.left + anchor.x,
		y: rect.top + anchor.y,
		inputH: anchor.cellH,
	};
}
