export interface RectLike {
	left: number;
	top: number;
	width: number;
	height: number;
}

export interface PanePadding {
	left: number;
	top: number;
	right: number;
	bottom: number;
}

export interface PaneGeometry {
	rows: number;
	cols: number;
	contentWidthCss: number;
	contentHeightCss: number;
	cellWidthCss: number;
	cellHeightCss: number;
	gridClientXCss: number;
	gridClientYCss: number;
	gridWidthCss: number;
	gridHeightCss: number;
	viewportDevice: { x: number; y: number; w: number; h: number };
}

export interface PaneGeometryInput {
	container: RectLike;
	host: RectLike;
	padding: PanePadding;
	cellWidthCss: number;
	cellHeightCss: number;
	dpr: number;
	/** Shared PTY grid. Omit when this viewer owns the grid size. */
	sharedGrid?: { rows: number; cols: number };
}

export function computePaneGeometry(input: PaneGeometryInput): PaneGeometry | null {
	const { container, host, padding } = input;
	const cellW = input.cellWidthCss;
	const cellH = input.cellHeightCss;
	const dpr = Number.isFinite(input.dpr) && input.dpr > 0 ? input.dpr : 1;
	const contentWidthCss = Math.max(0, container.width - padding.left - padding.right);
	const contentHeightCss = Math.max(0, container.height - padding.top - padding.bottom);
	if (contentWidthCss <= 0 || contentHeightCss <= 0 || cellW <= 0 || cellH <= 0) return null;

	const capacityCols = Math.max(1, Math.floor(contentWidthCss / cellW));
	const capacityRows = Math.max(1, Math.floor(contentHeightCss / cellH));
	const cols = Math.max(1, Math.floor(input.sharedGrid?.cols ?? capacityCols));
	const rows = Math.max(1, Math.floor(input.sharedGrid?.rows ?? capacityRows));
	const fullGridWidthCss = cols * cellW;
	const fullGridHeightCss = rows * cellH;
	const offsetXCss = input.sharedGrid ? Math.max(0, (contentWidthCss - fullGridWidthCss) / 2) : 0;
	const offsetYCss = input.sharedGrid ? Math.max(0, (contentHeightCss - fullGridHeightCss) / 2) : 0;
	const gridWidthCss = Math.min(fullGridWidthCss, contentWidthCss);
	const gridHeightCss = Math.min(fullGridHeightCss, contentHeightCss);
	const gridClientXCss = container.left + padding.left + offsetXCss;
	const gridClientYCss = container.top + padding.top + offsetYCss;
	const hostXCss = gridClientXCss - host.left;
	const hostYCss = gridClientYCss - host.top;
	const hostWDevice = Math.max(0, Math.round(host.width * dpr));
	const hostHDevice = Math.max(0, Math.round(host.height * dpr));
	const x = Math.max(0, Math.floor(hostXCss * dpr));
	const y = Math.max(0, Math.floor(hostYCss * dpr));
	const w = Math.max(
		0,
		Math.min(hostWDevice - x, Math.ceil((hostXCss + gridWidthCss) * dpr) - x),
	);
	const h = Math.max(
		0,
		Math.min(hostHDevice - y, Math.ceil((hostYCss + gridHeightCss) * dpr) - y),
	);
	return {
		rows,
		cols,
		contentWidthCss,
		contentHeightCss,
		cellWidthCss: cellW,
		cellHeightCss: cellH,
		gridClientXCss,
		gridClientYCss,
		gridWidthCss,
		gridHeightCss,
		viewportDevice: { x, y, w, h },
	};
}

export function cellFromClientPoint(
	geometry: PaneGeometry,
	clientX: number,
	clientY: number,
	rows = geometry.rows,
	cols = geometry.cols,
): { row: number; col: number } {
	const col = Math.max(0, Math.min(cols - 1, Math.floor(
		(clientX - geometry.gridClientXCss) / geometry.cellWidthCss,
	)));
	const row = Math.max(0, Math.min(rows - 1, Math.floor(
		(clientY - geometry.gridClientYCss) / geometry.cellHeightCss,
	)));
	return { row, col };
}
