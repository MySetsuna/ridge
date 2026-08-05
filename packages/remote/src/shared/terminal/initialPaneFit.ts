/** Cold-mount fit policy shared by the manager and its deterministic tests. */
export const INITIAL_FIT_RETRY_DELAYS_MS = [0, 16, 50, 150, 400] as const;

export interface InitialFitMeasurement {
	containerWidth: number;
	containerHeight: number;
	paddingLeft: number;
	paddingRight: number;
	paddingTop: number;
	paddingBottom: number;
	cellWidth: number;
	cellHeight: number;
	kernelRows: number;
	kernelCols: number;
	sharedRemoteMode: boolean;
	localGridAuthority: boolean;
}

/** A pane needs another fit while layout/metrics are unavailable or its local
 * kernel grid still differs from the measured content-box capacity. Passive
 * shared viewers intentionally keep the host's grid and are excluded. */
export function needsInitialPaneFit(measurement: InitialFitMeasurement): boolean {
	if (measurement.cellWidth <= 0 || measurement.cellHeight <= 0) return true;
	if (measurement.sharedRemoteMode && !measurement.localGridAuthority) return false;
	const width = measurement.containerWidth - measurement.paddingLeft - measurement.paddingRight;
	const height = measurement.containerHeight - measurement.paddingTop - measurement.paddingBottom;
	if (width <= 0 || height <= 0) return true;
	const cols = Math.max(1, Math.floor(width / measurement.cellWidth));
	const rows = Math.max(1, Math.floor(height / measurement.cellHeight));
	return measurement.kernelCols !== cols || measurement.kernelRows !== rows;
}
