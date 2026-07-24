/**
 * CONTRACT-52 / OP-GIT-BYPASS: dual-end process guard policy surface.
 * Aligns frontend display with ridge-core process_guard + git caps.
 */

import type { GitGuardStats } from './gitGuardStats';

/** Must match ridge-core git concurrency constants (cross-commented). */
export const GIT_CONCURRENCY_MIN = 1;
export const GIT_CONCURRENCY_MAX = 4;

export interface ProcessGuardView {
  active: number;
  peak: number;
  timeoutKills: number;
  acquireTimeouts: number;
  cap: number;
  pressure: 'ok' | 'elevated' | 'critical';
  badge: string;
  detail: string;
}

export function clampGitConcurrency(n: number): number {
  if (!Number.isFinite(n)) return GIT_CONCURRENCY_MAX;
  return Math.max(GIT_CONCURRENCY_MIN, Math.min(GIT_CONCURRENCY_MAX, Math.floor(n)));
}

export function pressureFromStats(s: GitGuardStats | null): ProcessGuardView {
  if (!s) {
    return {
      active: 0,
      peak: 0,
      timeoutKills: 0,
      acquireTimeouts: 0,
      cap: GIT_CONCURRENCY_MAX,
      pressure: 'ok',
      badge: '',
      detail: '无 git 护栏数据',
    };
  }
  const cap = s.logicalConcurrencyCap || GIT_CONCURRENCY_MAX;
  let pressure: ProcessGuardView['pressure'] = 'ok';
  if (s.timeoutKills > 0 || s.acquireTimeouts > 0) pressure = 'critical';
  else if (s.activeChildren >= cap) pressure = 'elevated';
  else if (s.activeChildren >= Math.max(1, cap - 1)) pressure = 'elevated';

  let badge = '';
  if (pressure === 'critical') {
    badge = `git 超时杀 ${s.timeoutKills || s.acquireTimeouts}`;
  } else if (pressure === 'elevated') {
    badge = `git ${s.activeChildren}/${cap}`;
  }

  const detail = [
    `活跃 ${s.activeChildren}`,
    `峰值 ${s.peakActiveChildren}`,
    `超时杀 ${s.timeoutKills}`,
    `获取超时 ${s.acquireTimeouts}`,
    `上限 ${cap} [${s.concurrencyMin}–${s.concurrencyMax}]`,
  ].join(' · ');

  return {
    active: s.activeChildren,
    peak: s.peakActiveChildren,
    timeoutKills: s.timeoutKills,
    acquireTimeouts: s.acquireTimeouts,
    cap,
    pressure,
    badge,
    detail,
  };
}

/** Whether Agent Center / Hosts should surface a red badge. */
export function shouldSurfaceGitGuard(s: GitGuardStats | null): boolean {
  const v = pressureFromStats(s);
  return v.pressure !== 'ok';
}

/**
 * Policy checklist for new spawn sites (audit helper).
 * Mirrors external_spawn_registry invariants for frontend docs/tests.
 */
export interface SpawnSitePolicy {
  binary: 'git' | 'taskkill' | 'kill' | 'other';
  module: string;
  requiresTreeKillOnTimeout: boolean;
}

export const KNOWN_SPAWN_SITES: SpawnSitePolicy[] = [
  { binary: 'git', module: 'commands::git', requiresTreeKillOnTimeout: true },
  { binary: 'taskkill', module: 'process_guard::kill_process_tree', requiresTreeKillOnTimeout: false },
  { binary: 'kill', module: 'process_guard::kill_process_tree', requiresTreeKillOnTimeout: false },
];

export function allRequiredTreeKillSitesCovered(sites: SpawnSitePolicy[] = KNOWN_SPAWN_SITES): boolean {
  return sites
    .filter((s) => s.requiresTreeKillOnTimeout)
    .every((s) => s.module.includes('git') || s.module.includes('process_guard'));
}

/** Frontend abort does not kill OS process — UI copy for cancel. */
export function cancelRefreshCopy(stats: ProcessGuardView): string {
  if (stats.active <= 0) return '无进行中的 git';
  return `取消仅停调度；后端超时后杀进程树（当前活跃 ${stats.active}）`;
}

export function mergePeak(prev: number, next: number): number {
  return Math.max(prev, next);
}
