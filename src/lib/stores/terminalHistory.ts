import { writable, get } from 'svelte/store';
import { invoke } from '@tauri-apps/api/core';

const _store = writable<string[]>([]);
export const terminalHistoryStore = {
    subscribe: _store.subscribe,
    fetch: async () => {
        try {
            const history: string[] = await invoke<string[]>('get_shell_history', { shellKind: '' });
            _store.set(history);
        } catch (e) {
            console.error('Failed to fetch shell history', e);
        }
    },
    add: (command: string) => {
        if (!command.trim()) return;
        _store.update(history => {
            const newHistory = [command, ...history.filter(h => h !== command)];
            return newHistory.slice(0, 1000);
        });
    }
};
