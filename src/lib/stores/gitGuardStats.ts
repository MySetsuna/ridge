/**
 * OP-GIT-BYPASS / iter 31: desktop store for git process hard-guard counters.
 * Backed by `get_git_guard_stats` (ridge-core GitGuardStats).
 */
import { writable, get } from 'svelte/store';
import { invoke, isTauri } from '@tauri-apps/api/core';

export interface GitGuardStats {
  activeChildren: number;
  peakActiveChildren: number;
  timeoutKills: number;
  acquireTimeouts: number;
  logicalConcurrencyCap: number;
  concurrencyMin: number;
  concurrencyMax: number;
}

export const gitGuardStats = writable<GitGuardStats | null>(null);
export const gitGuardStatsError = writable('');

export async function refreshGitGuardStats(): Promise<GitGuardStats | null> {
  if (!isTauri()) {
    gitGuardStats.set(null);
    return null;
  }
  try {
    const s = await invoke<GitGuardStats>('get_git_guard_stats');
    gitGuardStats.set(s);
    gitGuardStatsError.set('');
    return s;
  } catch (e) {
    gitGuardStatsError.set(e instanceof Error ? e.message : String(e));
    return get(gitGuardStats);
  }
}

/** Pure: whether counters indicate pressure worth surfacing in UI. */
export function gitGuardNeedsAttention(s: GitGuardStats | null): boolean {
  if (!s) return false;
  return s.timeoutKills > 0 || s.acquireTimeouts > 0 || s.activeChildren >= s.logicalConcurrencyCap;
}
