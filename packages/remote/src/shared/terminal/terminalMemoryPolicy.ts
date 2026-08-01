export const TERMINAL_MEMORY_SWEEP_MS = 30_000;
export const MIN_SCROLLBACK_BUDGET_ROWS = 20_000;
export const MAX_SCROLLBACK_BUDGET_ROWS = 80_000;
export const DEFAULT_SCROLLBACK_BUDGET_ROWS = 40_000;
export const HEAP_PRESSURE_RATIO = 0.75;

export interface BrowserHeapSnapshot {
	usedJSHeapSize: number;
	jsHeapSizeLimit: number;
}

export interface TerminalMemoryCandidate {
	paneId: string;
	scrollbackRows: number;
	focused: boolean;
	hidden: boolean;
	parked: boolean;
	lastForegroundAt: number;
}

export interface TerminalMemoryPlan {
	clearScrollbackPaneIds: string[];
	parkRendererPaneIds: string[];
	retainedRowsBefore: number;
	retainedRowsAfter: number;
	heapPressure: boolean;
}

/**
 * Bound total terminal history by device class, not pane count. Per-pane rings
 * already have a hard cap; this aggregate cap prevents many long-lived panes
 * from multiplying a safe local limit into an unsafe WebView allocation.
 */
export function terminalScrollbackBudgetRows(deviceMemoryGb?: number): number {
	if (!Number.isFinite(deviceMemoryGb) || !deviceMemoryGb || deviceMemoryGb <= 0) {
		return DEFAULT_SCROLLBACK_BUDGET_ROWS;
	}
	return Math.max(
		MIN_SCROLLBACK_BUDGET_ROWS,
		Math.min(MAX_SCROLLBACK_BUDGET_ROWS, Math.round(deviceMemoryGb * 10_000)),
	);
}

export function isBrowserHeapUnderPressure(memory?: BrowserHeapSnapshot | null): boolean {
	if (!memory) return false;
	if (!Number.isFinite(memory.usedJSHeapSize) || !Number.isFinite(memory.jsHeapSizeLimit)) {
		return false;
	}
	if (memory.usedJSHeapSize < 0 || memory.jsHeapSizeLimit <= 0) return false;
	return memory.usedJSHeapSize / memory.jsHeapSizeLimit >= HEAP_PRESSURE_RATIO;
}

/** Pure reclaim planner. Cold hidden/parked panes go first; focused history is
 * the last resort, but the aggregate bound remains hard under sustained load. */
export function planTerminalMemoryReclaim(args: {
	candidates: readonly TerminalMemoryCandidate[];
	rowBudget: number;
	heapPressure: boolean;
	documentHidden: boolean;
}): TerminalMemoryPlan {
	const rowBudget = Math.max(0, Math.floor(args.rowBudget));
	const retainedRowsBefore = args.candidates.reduce(
		(total, pane) => total + Math.max(0, Math.floor(pane.scrollbackRows)),
		0,
	);
	// Pressure needs headroom; merely returning to the limit causes immediate
	// reclaim churn on the next output burst.
	const targetRows = args.heapPressure ? Math.floor(rowBudget / 2) : rowBudget;
	const ordered = [...args.candidates].sort((a, b) => {
		if (a.focused !== b.focused) return a.focused ? 1 : -1;
		const aCold = a.parked || a.hidden;
		const bCold = b.parked || b.hidden;
		if (aCold !== bCold) return aCold ? -1 : 1;
		if (a.lastForegroundAt !== b.lastForegroundAt) {
			return a.lastForegroundAt - b.lastForegroundAt;
		}
		return b.scrollbackRows - a.scrollbackRows;
	});

	let retainedRowsAfter = retainedRowsBefore;
	const clearScrollbackPaneIds: string[] = [];
	for (const pane of ordered) {
		if (retainedRowsAfter <= targetRows) break;
		const rows = Math.max(0, Math.floor(pane.scrollbackRows));
		if (rows === 0) continue;
		clearScrollbackPaneIds.push(pane.paneId);
		retainedRowsAfter -= rows;
	}

	const parkRendererPaneIds = args.candidates
		// Inactive workspaces cannot present pixels, so retaining their GPU/
		// canvas renderer has no user value even before heap pressure begins.
		.filter((pane) => !pane.parked && (args.documentHidden || pane.hidden))
		.map((pane) => pane.paneId);

	return {
		clearScrollbackPaneIds,
		parkRendererPaneIds,
		retainedRowsBefore,
		retainedRowsAfter,
		heapPressure: args.heapPressure,
	};
}
