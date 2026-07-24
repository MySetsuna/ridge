/**
 * CONTRACT-57 / OP-AGENT-CP: orchestration control-plane UI model.
 * Binds get_orchestration_health + suspend + HITL + hosts counts.
 */

export type ControlPlaneLevel = 'ok' | 'watch' | 'degraded';

export interface OrchHealthDto {
  suspendedAgents?: number;
  pendingHitl?: number;
  hitlEnabled?: boolean;
  degraded?: boolean;
  generation?: number;
  foreignAttached?: number;
  outboundHostsConnected?: number;
  level?: string;
}

export interface OrchControlModel {
  level: ControlPlaneLevel;
  degraded: boolean;
  suspended: number;
  pendingHitl: number;
  hitlEnabled: boolean;
  foreignAttached: number;
  outboundConnected: number;
  generation: number;
  badge: string;
  lines: string[];
  showPauseAllHint: boolean;
  showHitlStrip: boolean;
}

export function normalizeLevel(raw: string | undefined, degraded: boolean): ControlPlaneLevel {
  if (degraded) return 'degraded';
  if (raw === 'watch' || raw === 'degraded' || raw === 'ok') return raw;
  return 'ok';
}

export function buildOrchControlModel(dto: OrchHealthDto | null | undefined): OrchControlModel {
  const suspended = Number(dto?.suspendedAgents ?? 0);
  const pendingHitl = Number(dto?.pendingHitl ?? 0);
  const hitlEnabled = Boolean(dto?.hitlEnabled);
  const degraded = Boolean(
    dto?.degraded ?? (suspended > 0 || (hitlEnabled && pendingHitl > 0)),
  );
  const foreignAttached = Number(dto?.foreignAttached ?? 0);
  const outboundConnected = Number(dto?.outboundHostsConnected ?? 0);
  const generation = Number(dto?.generation ?? 0);
  const level = normalizeLevel(dto?.level, degraded);

  const lines: string[] = [];
  if (suspended > 0) lines.push(`已暂停 agent ${suspended}`);
  if (hitlEnabled && pendingHitl > 0) lines.push(`待审批 ${pendingHitl}`);
  if (foreignAttached > 0) lines.push(`foreign 视图 ${foreignAttached}`);
  if (outboundConnected > 0) lines.push(`出站主机 ${outboundConnected}`);
  if (lines.length === 0) lines.push('控制面正常');

  let badge = '';
  if (level === 'degraded') badge = pendingHitl > 0 ? `HITL ${pendingHitl}` : `暂停 ${suspended}`;
  else if (level === 'watch') badge = `监视 · F${foreignAttached}/H${outboundConnected}`;

  return {
    level,
    degraded,
    suspended,
    pendingHitl,
    hitlEnabled,
    foreignAttached,
    outboundConnected,
    generation,
    badge,
    lines,
    showPauseAllHint: suspended > 0,
    showHitlStrip: hitlEnabled && pendingHitl > 0,
  };
}

export function remoteRosterDot(
  model: OrchControlModel,
  agentSuspended: boolean,
): 'ok' | 'warn' | 'bad' {
  if (agentSuspended || model.level === 'degraded') return 'bad';
  if (model.level === 'watch') return 'warn';
  return 'ok';
}

export function shouldRefreshHealth(
  prevGen: number,
  nextGen: number,
  pollMs: number,
  lastPollAt: number,
  now: number,
): boolean {
  if (nextGen > prevGen) return true;
  return now - lastPollAt >= pollMs;
}

export function healthPollMs(model: OrchControlModel): number {
  if (model.level === 'degraded') return 1500;
  if (model.level === 'watch') return 3000;
  return 8000;
}

export function formatOrchHeader(model: OrchControlModel): string {
  return `编排 · ${model.level} · gen ${model.generation}`;
}
