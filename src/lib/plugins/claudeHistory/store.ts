// src/lib/plugins/claudeHistory/store.ts
//
// Per-pane persistence for submitted Claude Code prompts. Lives in
// localStorage under one key per pane so re-opening the workspace restores
// history without depending on the backend. When a pane is closed its entry
// sits idle until the user clears it; UI treats that as cold storage.

import { writable, get } from 'svelte/store';

export interface ClaudeHistoryEntry {
  /** Prompt text as the user submitted it (or "" for bare REPL launches). */
  prompt: string;
  /** UTC millis timestamp. */
  at: number;
  /** agent_id assigned at register_teammate_agent time — for cross-ref. */
  agentId: string;
}

const PREFIX = 'wind-claude-history:';
const MAX_PER_PANE = 50;

function lsKey(paneId: string): string {
  return `${PREFIX}${paneId}`;
}

function loadPane(paneId: string): ClaudeHistoryEntry[] {
  if (typeof localStorage === 'undefined') return [];
  try {
    const raw = localStorage.getItem(lsKey(paneId));
    if (!raw) return [];
    const parsed = JSON.parse(raw) as unknown;
    if (!Array.isArray(parsed)) return [];
    return parsed.filter(
      (v): v is ClaudeHistoryEntry =>
        !!v &&
        typeof v === 'object' &&
        typeof (v as ClaudeHistoryEntry).prompt === 'string' &&
        typeof (v as ClaudeHistoryEntry).at === 'number' &&
        typeof (v as ClaudeHistoryEntry).agentId === 'string'
    );
  } catch {
    return [];
  }
}

function savePane(paneId: string, entries: ClaudeHistoryEntry[]): void {
  if (typeof localStorage === 'undefined') return;
  try {
    localStorage.setItem(
      lsKey(paneId),
      JSON.stringify(entries.slice(-MAX_PER_PANE))
    );
  } catch {
    /* quota — drop silently, history is best-effort */
  }
}

/**
 * Reactive map: { paneId → entries[] }. Initialised lazily — first read for
 * a pane reaches into localStorage, subsequent reads use the cached array.
 */
const _store = writable<Record<string, ClaudeHistoryEntry[]>>({});
export const claudeHistoryStore = { subscribe: _store.subscribe };

export function getHistoryForPane(paneId: string): ClaudeHistoryEntry[] {
  const cached = get(_store)[paneId];
  if (cached) return cached;
  const loaded = loadPane(paneId);
  _store.update((s) => ({ ...s, [paneId]: loaded }));
  return loaded;
}

/** Append a new entry. Writes through to localStorage. */
export function pushHistoryEntry(paneId: string, entry: ClaudeHistoryEntry): void {
  const current = getHistoryForPane(paneId);
  const next = [...current, entry].slice(-MAX_PER_PANE);
  _store.update((s) => ({ ...s, [paneId]: next }));
  savePane(paneId, next);
}

/** Clear history for one pane (user explicit). */
export function clearHistoryForPane(paneId: string): void {
  _store.update((s) => {
    const next = { ...s };
    delete next[paneId];
    return next;
  });
  if (typeof localStorage !== 'undefined') {
    try {
      localStorage.removeItem(lsKey(paneId));
    } catch {
      /* ignore */
    }
  }
}
