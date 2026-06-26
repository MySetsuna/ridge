// 资源管理器手风琴：用户拖拽分隔条调整的「cwd 文件区高度权重」，跨会话持久化。
//
// 用 flex-grow 权重而非固定像素：每个展开的 cwd 文件区 `flex: 权重 1 0`，
// 它们按权重比例瓜分「头部之外的剩余空间」。这保证：
//   - 自动填满剩余空间（无空隙）；
//   - 拖拽只是重分配固定的那块空间，永远不会把其它工作区/cwd 头挤出可见区；
//   - 窗口缩放时浏览器按比例自动重算，所有头始终可见（body 可压缩到 0）。
//
// key = cwd 绝对路径（跨会话稳定）；value = 权重（缺省 1）。约定每次拖拽后归一化到
// 「均值 ≈ 1」，使新出现的 cwd（默认权重 1）拿到接近平均的份额，不会被既有权重压扁。
//
// 拖拽中频繁 update（仅内存，驱动响应式权重），松手才 persist 落 localStorage。

import { get, writable } from 'svelte/store';

const STORAGE_KEY = 'rg.explorer.bodyWeights';

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

export const explorerBodyWeights = writable<Record<string, number>>(load());

/** 拖拽中：合并多个 cwd 的新权重（仅内存，驱动响应式 flex-grow），不落盘。 */
export function applyExplorerBodyWeights(changes: Record<string, number>): void {
	explorerBodyWeights.update((m) => ({ ...m, ...changes }));
}

/** 松手时：把当前权重表落 localStorage，跨会话恢复。 */
export function persistExplorerBodyWeights(): void {
	if (typeof localStorage === 'undefined') return;
	try {
		localStorage.setItem(STORAGE_KEY, JSON.stringify(get(explorerBodyWeights)));
	} catch {
		/* 存储不可用（隐私模式 / 配额）时静默忽略，仅丢失持久化。 */
	}
}
