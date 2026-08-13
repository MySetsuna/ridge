// 资源管理器手风琴：cwd 文件区高度 (px) + 下方展示域 / 后续区块 free-follow 压缩。
//
// 渲染策略（Explorer.svelte）：
//   - `.explorer-col-stack`：body + sep +（可选）lower plugins。
//   - body 未设高：`flex: 1 1 0` 吃满栈；lower **内容高度**（空则不渲染），不 50/50 分。
//   - body 已设高：`flex: 0 1 Hpx`（可 shrink，窗口变矮不卡死）；lower 有内容时
//     `flex: 1 1 0; min-h:0` 被 body 挤扁；无内容时不占位。
//   - 拖拽上界：栈顶 → explorer 底 的剩余高度（含后续 cwd 区块），非「假 lower 空 flex」。
//
// key = cwd；拖中只写内存，松手 persist。

import { get, writable } from 'svelte/store';

const STORAGE_KEY = 'rg.explorer.bodyHeights';

/** 文件树区最小高度（px）。 */
export const MIN_BODY_H = 40;
/**
 * 分隔条以下「其余布局」最小保留高度（后续 cwd 头 / 有内容 lower 的底线）。
 * 允许越过原 lower header 上沿，只留一条可辨识余量。
 */
export const MIN_BELOW_H = 0;
/** 分隔条厚度（px），与 Explorer `h-[3px]` 对齐。 */
export const BODY_SEP_H = 3;

/** @deprecated 别名 — 旧测试/调用用 minLower；语义 = MIN_BELOW_H */
export const MIN_LOWER_H = MIN_BELOW_H;

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

export function setExplorerBodyHeight(cwd: string, height: number): void {
	explorerBodyHeights.update((m) => ({ ...m, [cwd]: height }));
}

export function persistExplorerBodyHeights(): void {
	if (typeof localStorage === 'undefined') return;
	try {
		localStorage.setItem(STORAGE_KEY, JSON.stringify(get(explorerBodyHeights)));
	} catch {
		/* 隐私模式 / 配额 */
	}
}

/**
 * 将期望 body 高度夹到 [minBody, maxBody]。
 * `columnInnerH` = 从本 col-stack 顶到 explorer 底的可用高度（含后续 cwd，可被挤）。
 */
export function clampBodyHeight(
	desired: number,
	opts: {
		columnInnerH: number;
		minBody?: number;
		/** 分隔条以下至少保留（后续区块 / lower 内容） */
		minLower?: number;
		minBelow?: number;
		sepH?: number;
	},
): number {
	const minBody = opts.minBody ?? MIN_BODY_H;
	const minBelow = opts.minBelow ?? opts.minLower ?? MIN_BELOW_H;
	const sepH = opts.sepH ?? BODY_SEP_H;
	const col = Math.max(0, opts.columnInnerH);
	const maxBody = Math.max(minBody, col - sepH - minBelow);
	const d = Number.isFinite(desired) ? desired : minBody;
	return Math.min(maxBody, Math.max(minBody, d));
}

export function computeBodyHeightFromDrag(
	startH: number,
	startY: number,
	clientY: number,
	columnInnerH: number,
	opts?: { minBody?: number; minLower?: number; minBelow?: number; sepH?: number },
): number {
	return clampBodyHeight(startH + (clientY - startY), { columnInnerH, ...opts });
}

/** 分隔条以下剩余高度。 */
export function lowerRegionHeight(
	columnInnerH: number,
	bodyH: number,
	sepH: number = BODY_SEP_H,
): number {
	return Math.max(0, columnInnerH - bodyH - sepH);
}

/**
 * 产品路径样式决策（纯函数，单测钉死）：
 * - 无固定 body 高 + 无 lower 内容 → body 吃满，不造空 lower
 * - 有固定 body 高且有 lower 内容 → body flex 0 1 H（可 shrink）；lower 有内容才 flex-1 被压
 * - lower 无内容 → 不占 flex 份额
 */
export function resolveExplorerStackLayout(input: {
	bodyHeightPx: number | null | undefined;
	hasLowerContent: boolean;
}): {
	bodyStyle: string;
	stackClassExtra: string;
	showLower: boolean;
	lowerClass: string;
} {
	// No lower region means no user-resizable split. Ignore a stale persisted
	// height so the tree keeps filling the available column after plugins close.
	const hasFixed = input.hasLowerContent
		&& input.bodyHeightPx != null
		&& Number.isFinite(input.bodyHeightPx);
	const H = hasFixed ? Math.max(MIN_BODY_H, Number(input.bodyHeightPx)) : null;
	const showLower = input.hasLowerContent;

	if (hasFixed && H != null) {
		return {
			// shrink:1 — 窗口/多 cwd 变矮时允许压 body，避免 0 0 卡死
			bodyStyle: `flex: 0 1 ${H}px; min-height: ${MIN_BODY_H}px; max-height: 100%`,
			// 栈随 body 长高，挤后续 cwd；仍可 shrink
			stackClassExtra: 'flex-[0_1_auto]',
			showLower,
			// 有内容 lower：余量 flex，min-h-0 可被 body 压扁
			lowerClass: showLower
				? 'explorer-lower min-h-0 flex-1 overflow-y-auto rg-scroll'
				: '',
		};
	}

	// 默认：body 填满栈；lower 仅内容高，绝不 50/50 空分
	return {
		bodyStyle: 'flex: 1 1 0',
		stackClassExtra: 'flex-1',
		showLower,
		lowerClass: showLower
			? 'explorer-lower min-h-0 flex-[0_0_auto] overflow-y-auto rg-scroll'
			: '',
	};
}

/**
 * 布局变化后（窗口缩小等）把已存 H 重新夹到 live 可用高度。
 * 返回 null 表示无需改写 store。
 */
export function reclampStoredBodyHeight(
	storedH: number,
	liveColumnInnerH: number,
	opts?: { minBody?: number; minBelow?: number; sepH?: number },
): number | null {
	const next = clampBodyHeight(storedH, { columnInnerH: liveColumnInnerH, ...opts });
	return next === storedH ? null : next;
}
