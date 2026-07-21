/**
 * imeAnchor — scissor-同源的桌面 IME textarea 坐标换算（缺陷 B 修复）。
 *
 * 为什么单独成模块：桌面 WebGPU 把所有分区画在同一张全局 host canvas 上，
 * 用 scissor 把每个分区切片到对应位置。scissor 的设备像素原点由
 * `manager.ts::_recomputeViewport` 计算：
 *     xDev = floor((container.left - host.left + padL) * dpr)
 * 而每个 cell 在 scissor 内部由渲染器（webgpu.rs draw_row_*，与
 * `quantizeCellSize` 一致）放在
 *     round(col * cellW * dpr)
 * 这些设备像素处。
 *
 * IME textarea 是分区容器内的 `position:absolute` 元素，原来用
 * `round(col * cellW) + pad`（CSS 像素 round + 原始 padding 原点）来设
 * left/top，与上面两者「取整基准」「原点」都不同——只有 dpr=1 时恰好相等，
 * 125%/150% 缩放或多分屏下就会偏。
 *
 * 本模块把 textarea 的 CSS-px left/top 从渲染器/ scissor 用的同一组数字
 * 反推出来：先算出 cell 在 host canvas 上的绝对设备像素
 * （scissor 原点 + cell 设备偏移），再换回分区容器（padding-box 原点）的
 * CSS 像素坐标系。这样 textarea 左上角就精确压在渲染器真正绘制该 cell 的
 * 设备像素上，OS IME 候选框也就落在可见光标格处。
 *
 * 纯函数、无副作用，方便 Vitest 枚举不同 dpr / 偏移下的对齐不变式。
 */

export interface ImeAnchorInput {
	/** 分区容器 padding-box 左边界相对视口（getBoundingClientRect().left）。 */
	containerLeft: number;
	/** 分区容器 padding-box 上边界相对视口（getBoundingClientRect().top）。 */
	containerTop: number;
	/** 全局 host canvas 左边界相对视口。 */
	hostLeft: number;
	/** 全局 host canvas 上边界相对视口。 */
	hostTop: number;
	/** 容器 padding-left（CSS px）。 */
	padL: number;
	/** 容器 padding-top（CSS px）。 */
	padT: number;
	/** 每格宽度（CSS px，已 quantizeCellSize 量化）。 */
	cellW: number;
	/** 每格高度（CSS px，已 quantizeCellSize 量化）。 */
	cellH: number;
	/** 目标列（视口内列号，0 基）。 */
	col: number;
	/** 目标行（视口内行号，0 基）。 */
	row: number;
	/** devicePixelRatio。 */
	dpr: number;
}

/**
 * scissor 的设备像素原点，逐字对应 `_recomputeViewport`：
 *   cssX = container.left - host.left + padL
 *   xDev = max(0, floor(cssX * dpr))
 * （Y 轴同理）。textarea 必须以同一个 floor 后的原点为基准，否则两坐标系
 * 在亚像素层面错开。
 */
export function scissorOriginDevicePx(
	input: Pick<
		ImeAnchorInput,
		'containerLeft' | 'containerTop' | 'hostLeft' | 'hostTop' | 'padL' | 'padT' | 'dpr'
	>,
): { xDev: number; yDev: number } {
	const cssX = input.containerLeft - input.hostLeft + input.padL;
	const cssY = input.containerTop - input.hostTop + input.padT;
	return {
		xDev: Math.max(0, Math.floor(cssX * input.dpr)),
		yDev: Math.max(0, Math.floor(cssY * input.dpr)),
	};
}

/**
 * 渲染器内部对某一格左/上边沿的设备像素偏移，逐字对应 webgpu.rs 的
 * `round(cell_css * dpr)`（cell 累加在设备空间取整，而非先在 CSS 空间
 * round 再乘 dpr）。这正是把 `quantizeCellSize` 的量化基准搬到 IME 这边。
 */
export function cellDeviceOffsetPx(cellIndex: number, cellSizeCss: number, dpr: number): number {
	return Math.round(cellIndex * cellSizeCss * dpr);
}

/**
 * 计算 IME textarea 的 left/top（CSS px，相对分区容器 padding-box 原点）。
 *
 * 推导：cell 在 host canvas 上的绝对设备像素左边沿
 *     absDevX = scissorOriginX + cellDeviceOffsetX
 * textarea 的 left 相对容器 padding-box 原点（绝对设备像素为
 * `(containerLeft - hostLeft) * dpr`），故
 *     left_css = absDevX / dpr - (containerLeft - hostLeft)
 * Y 轴同理。这样 textarea 左上角在设备空间精确等于渲染器绘制该 cell 的
 * 像素，消除「CSS-round vs device-round」「raw padding vs floored scissor」
 * 两处不一致。
 */
export function imeHelperCssPosition(input: ImeAnchorInput): {
	x: number;
	y: number;
	cellW: number;
	cellH: number;
} {
	const { xDev, yDev } = scissorOriginDevicePx(input);
	const cellX = cellDeviceOffsetPx(input.col, input.cellW, input.dpr);
	const cellY = cellDeviceOffsetPx(input.row, input.cellH, input.dpr);
	const absDevX = xDev + cellX;
	const absDevY = yDev + cellY;
	return {
		x: absDevX / input.dpr - (input.containerLeft - input.hostLeft),
		y: absDevY / input.dpr - (input.containerTop - input.hostTop),
		cellW: input.cellW,
		cellH: input.cellH,
	};
}
