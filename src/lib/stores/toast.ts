// src/lib/stores/toast.ts
//
// Lightweight session-scoped toast notification system.
// Usage: `import { showToast } from '$lib/stores/toast'`
//        `showToast('已切换到 main')`
//        `showToast('操作失败', 'error')`

import { writable } from 'svelte/store';

export type ToastType = 'success' | 'error' | 'info';

export interface ToastItem {
  id: number;
  message: string;
  type: ToastType;
}

let _nextId = 0;

const _store = writable<ToastItem[]>([]);

export const toastStore = { subscribe: _store.subscribe };

const TOAST_DURATION_MS = 3000;

export function showToast(message: string, type: ToastType = 'success'): void {
  const id = ++_nextId;
  _store.update((list) => [...list, { id, message, type }]);
  setTimeout(() => {
    _store.update((list) => list.filter((t) => t.id !== id));
  }, TOAST_DURATION_MS);
}
