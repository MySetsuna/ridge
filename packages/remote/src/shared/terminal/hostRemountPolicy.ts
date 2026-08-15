/** Shared swap-chain wipe on remount blacks siblings that skip a redraw. */
export function shouldWipeHostOnPaneRemount(retainRenderer: boolean): boolean {
	return !retainRenderer;
}

/** After a host wipe or remount, cached instances are the old scissor/size. */
export function shouldReplayHostCache(
	dirty: boolean,
	surfaceJustWiped: boolean,
	becameVisible: boolean,
): boolean {
	return !dirty && !surfaceJustWiped && !becameVisible;
}
