// 资源管理器手风琴：cwd 文件区高度 (px) + 下方展示域随拖自由压缩。
//
// 渲染策略（Explorer.svelte）：
//   - 每个 cwd 的 body+分隔条+下方 plugin 包在 `.explorer-col-stack`（真 flex 列）。
//   - body：`flex: 0 0 Hpx`（拖过后）或 `flex: 1 1 0`（默认平分）。
//   - lower：`flex: 1 1 0; min-height: 0; overflow: auto` —— 随 body 增高实时压缩，
//     不再被插件内容 min-height:auto 卡在「下方 header 上方」。
//
// key = cwd 绝对路径；value = 像素高度。拖中只写内存，松手 persist。

import { get, writable } from 'svelte/store';

const STORAGE_KEY = 'rg.explorer.bodyHeights';

/** 文件树区最小高度（px）。 */
export const MIN_BODY_H = 40;
/** 下方展示域（plugin / SCM 等）最小可见高度 —— 允许压到一条头的量级。 */
export const MIN_LOWER_H = 28;
/** 分隔条厚度（px），与 Explorer 模板 `h-[3px]` 对齐。 */
export const BODY_SEP_H = 3;

function load(): Record<string, number> {
	if (typeof localStorage === 'undefined') return {};
	try {
		const raw = localStorage.getItem(STORAGE_KEY);
		const parsed = raw ? JSON.parse(raw) : {};
		return parsed && typeof parsed === 'object' ? (parsed as Record<string, number>) : {};
	} catch {
		return {};
	}
}

export const explorerBodyHeights = writable<Record<string, number>>(load());

/** 拖拽中：更新某个 cwd 的高度（仅内存），不落盘。 */
export function setExplorerBodyHeight(cwd: string, height: number): void {
	explorerBodyHeights.update((m) => ({ ...m, [cwd]: height }));
}

/** 松手：落 localStorage。 */
export function persistExplorerBodyHeights(): void {
	if (typeof localStorage === 'undefined') return;
	try {
		localStorage.setItem(STORAGE_KEY, JSON.stringify(get(explorerBodyHeights)));
	} catch {
		/* 隐私模式 / 配额：静默 */
	}
}

/**
 * 将期望 body 高度夹到 [minBody, maxBody]，max 由「列栈可用高度 − 分隔条 − 下方最小高度」决定。
 * 这是 free-follow 的核心：允许 body 顶到下方区域内部，只保留 minLower 余量。
 */
export function clampBodyHeight(
	desired: number,
	opts: {
		columnInnerH: number;
		minBody?: number;
		minLower?: number;
		sepH?: number;
	},
): number {
	const minBody = opts.minBody ?? MIN_BODY_H;
	const minLower = opts.minLower ?? MIN_LOWER_H;
	const sepH = opts.sepH ?? BODY_SEP_H;
	const col = Math.max(0, opts.columnInnerH);
	// 下方至少 minLower；body 至少 minBody；若列太矮优先保 minBody。
	const maxBody = Math.max(minBody, col - sepH - minLower);
	const d = Number.isFinite(desired) ? desired : minBody;
	return Math.min(maxBody, Math.max(minBody, d));
}

/**
 * 拖拽：startH + (clientY − startY) → 夹紧后的 body 高度。
 * 不读取 DOM；调用方传入 start 时测得的 columnInnerH。
 */
export function computeBodyHeightFromDrag(
	startH: number,
	startY: number,
	clientY: number,
	columnInnerH: number,
	opts?: { minBody?: number; minLower?: number; sepH?: number },
): number {
	const delta = clientY - startY;
	return clampBodyHeight(startH + delta, { columnInnerH, ...opts });
}

/** 给定列栈高度与 body 高度，下方展示域应得高度（≥ 0）。 */
export function lowerRegionHeight(
	columnInnerH: number,
	bodyH: number,
	sepH: number = BODY_SEP_H,
): number {
	return Math.max(0, columnInnerH - bodyH - sepH);
}
