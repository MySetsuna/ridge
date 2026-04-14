import { writable } from 'svelte/store';
import type { Component } from 'svelte';

export interface ContextMenuItem {
	id: string;
	label?: string;
	icon?: Component;
	shortcut?: string;
	action?: () => void;
	disabled?: boolean;
	divider?: boolean;
	children?: ContextMenuItem[];
}

export interface ContextMenuState {
	visible: boolean;
	x: number;
	y: number;
	items: ContextMenuItem[];
	targetElement?: string;
}

function createContextMenuStore() {
	const { subscribe, set, update } = writable<ContextMenuState>({
		visible: false,
		x: 0,
		y: 0,
		items: [],
		targetElement: undefined
	});

	return {
		subscribe,
		show: (x: number, y: number, items: ContextMenuItem[], targetElement?: string) => {
			set({
				visible: true,
				x,
				y,
				items,
				targetElement
			});
		},
		hide: () => {
			update(state => ({ ...state, visible: false }));
		},
		updatePosition: (x: number, y: number) => {
			update(state => ({ ...state, x, y }));
		}
	};
}

export const contextMenu = createContextMenuStore();

// 导出 showContextMenu 作为别名，方便调用
export const showContextMenu = contextMenu.show;
export const hideContextMenu = contextMenu.hide;