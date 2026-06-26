// 资源管理器手风琴：用户拖拽分隔条调整的「cwd 文件区高度」，跨会话持久化。
//
// key = cwd 绝对路径（跨会话稳定，比会话内的 column id 更适合持久化）；
// value = 像素高度。未设过的 cwd 不在表里 —— 渲染时回退到 flex-1 自动平分。
//
// 拖拽过程中频繁 update（仅内存，驱动响应式高度），松手才 persist 落 localStorage，
// 避免每次 pointermove 都写盘。

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

/** 拖拽中：仅更新内存（响应式驱动 body 高度），不落盘。 */
export function updateExplorerBodyHeight(cwd: string, height: number): void {
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
