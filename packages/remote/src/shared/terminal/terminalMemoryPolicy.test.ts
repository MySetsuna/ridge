import { describe, expect, it } from 'vitest';
import {
	DEFAULT_SCROLLBACK_BUDGET_ROWS,
	isBrowserHeapUnderPressure,
	planTerminalMemoryReclaim,
	terminalScrollbackBudgetRows,
} from './terminalMemoryPolicy';

describe('terminal memory policy', () => {
	it('scales the aggregate row budget by device memory within hard bounds', () => {
		expect(terminalScrollbackBudgetRows()).toBe(DEFAULT_SCROLLBACK_BUDGET_ROWS);
		expect(terminalScrollbackBudgetRows(1)).toBe(20_000);
		expect(terminalScrollbackBudgetRows(4)).toBe(40_000);
		expect(terminalScrollbackBudgetRows(64)).toBe(80_000);
	});

	it('detects only valid high heap ratios', () => {
		expect(isBrowserHeapUnderPressure({ usedJSHeapSize: 750, jsHeapSizeLimit: 1000 })).toBe(true);
		expect(isBrowserHeapUnderPressure({ usedJSHeapSize: 749, jsHeapSizeLimit: 1000 })).toBe(false);
		expect(isBrowserHeapUnderPressure({ usedJSHeapSize: 1, jsHeapSizeLimit: 0 })).toBe(false);
	});

	it('reclaims cold history before focused history and leaves headroom under pressure', () => {
		const plan = planTerminalMemoryReclaim({
			rowBudget: 20_000,
			heapPressure: true,
			documentHidden: false,
			candidates: [
				{ paneId: 'focused', scrollbackRows: 9_000, focused: true, hidden: false, parked: false, lastForegroundAt: 30 },
				{ paneId: 'cold-old', scrollbackRows: 7_000, focused: false, hidden: true, parked: false, lastForegroundAt: 10 },
				{ paneId: 'cold-new', scrollbackRows: 6_000, focused: false, hidden: false, parked: true, lastForegroundAt: 20 },
			],
		});

		expect(plan.clearScrollbackPaneIds).toEqual(['cold-old', 'cold-new']);
		expect(plan.parkRendererPaneIds).toEqual(['cold-old']);
		expect(plan.retainedRowsAfter).toBe(9_000);
	});

	it('isolates a two-pane reclaim and preserves the active pane', () => {
		const plan = planTerminalMemoryReclaim({
			rowBudget: 3_000,
			heapPressure: false,
			documentHidden: false,
			candidates: [
				{ paneId: 'active', scrollbackRows: 2_000, focused: true, hidden: false, parked: false, lastForegroundAt: 20 },
				{ paneId: 'inactive', scrollbackRows: 2_000, focused: false, hidden: true, parked: false, lastForegroundAt: 10 },
			],
		});

		expect(plan.clearScrollbackPaneIds).toEqual(['inactive']);
		expect(plan.parkRendererPaneIds).toEqual(['inactive']);
		expect(plan.retainedRowsAfter).toBe(2_000);
	});

	it('releases inactive workspace renderers before heap pressure', () => {
		const plan = planTerminalMemoryReclaim({
			rowBudget: 20_000,
			heapPressure: false,
			documentHidden: false,
			candidates: [
				{ paneId: 'active', scrollbackRows: 10, focused: true, hidden: false, parked: false, lastForegroundAt: 2 },
				{ paneId: 'inactive', scrollbackRows: 10, focused: false, hidden: true, parked: false, lastForegroundAt: 1 },
			],
		});

		expect(plan.clearScrollbackPaneIds).toEqual([]);
		expect(plan.parkRendererPaneIds).toEqual(['inactive']);
	});

	it('parks every live renderer while the document is hidden without deleting bounded history', () => {
		const plan = planTerminalMemoryReclaim({
			rowBudget: 20_000,
			heapPressure: false,
			documentHidden: true,
			candidates: [
				{ paneId: 'a', scrollbackRows: 2_000, focused: true, hidden: false, parked: false, lastForegroundAt: 2 },
				{ paneId: 'b', scrollbackRows: 2_000, focused: false, hidden: true, parked: false, lastForegroundAt: 1 },
			],
		});

		expect(plan.clearScrollbackPaneIds).toEqual([]);
		expect(plan.parkRendererPaneIds).toEqual(['a', 'b']);
	});
});
