// 资源管理器手风琴：用户拖拽分隔条调整的「cwd 文件区高度」(px)，跨会话持久化。
//
// 渲染策略（在 Explorer.svelte）：
//   - 未设过高度的 cwd 文件区 → flex:1 1 0：自动平分/填满剩余空间（默认无空隙）。
//   - 设过高度的 → flex:0 1 H px：固定为 H、可被手动缩小（留空），但 shrink:1 保证
//     窗口变小或拖太大时仍会收缩，绝不溢出 → 所有工作区/cwd 头始终可见。
//
// key = cwd 绝对路径（跨会话稳定）；value = 像素高度。
// 拖拽中频繁 update（仅内存，驱动响应式高度），松手才 persist 落 localStorage。

import { get, writable } from 'svelte/store';

const STORAGE_KEY = 'rg.explorer.bodyHeights';

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

/** 拖拽中：更新某个 cwd 的高度（仅内存，驱动响应式 flex-basis），不落盘。 */
export function setExplorerBodyHeight(cwd: string, height: number): void {
	explorerBodyHeights.update((m) => ({ ...m, [cwd]: height }));
}

/** 松手时：把当前高度表落 localStorage，跨会话恢复。 */
export function persistExplorerBodyHeights(): void {
	if (typeof localStorage === 'undefined') return;
	try {
		localStorage.setItem(STORAGE_KEY, JSON.stringify(get(explorerBodyHeights)));
	} catch {
		/* 存储不可用（隐私模式 / 配额）时静默忽略，仅丢失持久化。 */
	}
}
