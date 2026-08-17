/** Shared swap-chain wipe on remount blacks siblings that skip a redraw. */
export function shouldWipeHostOnPaneRemount(retainRenderer: boolean): boolean {
	return !retainRenderer;
}
