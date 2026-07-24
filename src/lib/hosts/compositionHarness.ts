/**
 * CONTRACT-59 / OP-USER-RAIL+composition: multi-module product composition checks.
 * Deterministic pure harness — used by vitest + documents integration seams.
 */

import { planAttachSeed, planDetachHistory } from './foreignHistorySession';
import { checkHostTaskIsolation, scheduleReconnectTask, stepHostTask } from './hostSessionIsolation';
import { buildHostRowAlerts, type HostRowModel } from './hostControlSurface';
import { buildOrchControlModel } from '../teammate/orchControlPlane';
import { pressureFromStats } from '../stores/processGuardPolicy';
import type { GitGuardStats } from '../stores/gitGuardStats';
import {
  simulateDetachDoesNotError,
  simulateHappyPath,
} from '../../../packages/remote/src/shared/hosts/outboundLifecycle';
import { applyPumpBatch, initialPumpState } from '../../../packages/remote/src/shared/hosts/livePumpPolicy';
import { admitRemoteMethod } from '../../../packages/remote/src/shared/transport/protocolAdmission';
import { planHostOpen } from '../../../packages/remote/src/shared/terminal/linkOpenHost';
import { filterAuditItems } from '../teammate/hitlAuditFilter';

export interface CompositionScenario {
  name: string;
  ok: boolean;
  evidence: string[];
  failures: string[];
}

function pass(name: string, evidence: string[]): CompositionScenario {
  return { name, ok: true, evidence, failures: [] };
}

function fail(name: string, failures: string[], evidence: string[] = []): CompositionScenario {
  return { name, ok: false, evidence, failures };
}

/** C50+C58: attach seed + outbound happy path + detach. */
export function scenarioOutboundHistory(): CompositionScenario {
  const evidence: string[] = [];
  const life = simulateHappyPath('host-a', 'sess-1');
  evidence.push(`lifecycle ${life.phase} fanout=${life.fanoutBytes}`);
  const seed = planAttachSeed({
    localTailBytes: life.fanoutBytes,
    rows: 24,
    cols: 80,
    reattach: false,
    hostHistoryKnown: true,
  });
  evidence.push(`seed ${seed.reason} bytes=${seed.seedBytes}`);
  const det = simulateDetachDoesNotError('host-a', 'sess-1');
  const dh = planDetachHistory({ keepLocalTail: true });
  if (det.phase !== 'Detached' || dh.killRemote) {
    return fail('outbound_history', ['detach killed remote or wrong phase'], evidence);
  }
  return pass('outbound_history', evidence);
}

/** C56+C54: isolation + backpressure. */
export function scenarioIsolationPump(): CompositionScenario {
  const evidence: string[] = [];
  let t1 = scheduleReconnectTask(undefined, 'h1', ['p1']);
  let t2 = scheduleReconnectTask(undefined, 'h2', ['p2']);
  t1 = stepHostTask(t1, { hostReachable: false });
  const iso = checkHostTaskIsolation([t1, t2]);
  evidence.push(`isolation ok=${iso.ok}`);
  let pump = initialPumpState(64);
  for (let i = 0; i < 5; i++) {
    const r = applyPumpBatch(pump, { hostId: 'h1', sessionId: 'p1', byteLength: 20 });
    pump = r.state;
  }
  evidence.push(`pump buffered=${pump.bufferedBytes} dropped=${pump.droppedBytes}`);
  if (!iso.ok) return fail('isolation_pump', iso.issues, evidence);
  if (pump.bufferedBytes > 64) return fail('isolation_pump', ['buffer over cap'], evidence);
  return pass('isolation_pump', evidence);
}

/** C55+C51: protocol deny + link open. */
export function scenarioProtocolLink(): CompositionScenario {
  const evidence: string[] = [];
  const deny = admitRemoteMethod('connect_host');
  const allow = admitRemoteMethod('list_hitl_pending');
  evidence.push(`admit connect_host=${deny.ok} hitl=${allow.ok}`);
  const open = planHostOpen('https://example.com', 'url');
  evidence.push(`open ${open.type}`);
  if (deny.ok || !allow.ok || open.type !== 'open_url') {
    return fail('protocol_link', ['admission or open failed'], evidence);
  }
  return pass('protocol_link', evidence);
}

/** C52+C57: git guard + orch model. */
export function scenarioGitOrch(): CompositionScenario {
  const stats: GitGuardStats = {
    activeChildren: 4,
    peakActiveChildren: 4,
    timeoutKills: 1,
    acquireTimeouts: 0,
    logicalConcurrencyCap: 4,
    concurrencyMin: 1,
    concurrencyMax: 4,
  };
  const pg = pressureFromStats(stats);
  const orch = buildOrchControlModel({
    suspendedAgents: 1,
    pendingHitl: 1,
    hitlEnabled: true,
    degraded: true,
    foreignAttached: 1,
    outboundHostsConnected: 1,
    level: 'degraded',
    generation: 9,
  });
  const evidence = [`git ${pg.pressure}`, `orch ${orch.level}`, orch.badge];
  if (pg.pressure !== 'critical' || orch.level !== 'degraded') {
    return fail('git_orch', ['expected critical+degraded'], evidence);
  }
  return pass('git_orch', evidence);
}

/** C53+C5 UI alerts. */
export function scenarioHitlHostsUi(): CompositionScenario {
  const audit = filterAuditItems(
    [
      {
        id: 'a',
        ts: 1,
        source: 'desktop',
        initiator: 'x',
        verdict: 'approve',
        riskLevel: 'Dangerous',
        reasonSummary: 'token=abcdefghijklmnopqrstuvwxyz0123456789',
        outcome: 'ok',
      },
    ],
    { limit: 5 },
  );
  const row: HostRowModel = {
    id: 'h1',
    kind: 'remote',
    label: 'lab',
    status: 'connected',
    sessionCount: 1,
    attachedCount: 0,
    reconnectAttempt: 0,
    historyBytes: 2048,
    outbound: {
      state: 'Live',
      fanoutBytes: 100,
      writeOk: 1,
      liveBufferCap: 100,
      liveBufferBytes: 95,
      liveDroppedBytes: 10,
    },
  };
  const alerts = buildHostRowAlerts(row);
  const evidence = [audit.summary, ...alerts];
  if (audit.items[0]?.reasonSummary.includes('abcdefghijklmnopqrstu')) {
    return fail('hitl_hosts_ui', ['secret not redacted'], evidence);
  }
  if (alerts.length === 0) return fail('hitl_hosts_ui', ['expected alerts'], evidence);
  return pass('hitl_hosts_ui', evidence);
}

export function runAllCompositionScenarios(): CompositionScenario[] {
  return [
    scenarioOutboundHistory(),
    scenarioIsolationPump(),
    scenarioProtocolLink(),
    scenarioGitOrch(),
    scenarioHitlHostsUi(),
  ];
}

export function compositionAllGreen(): boolean {
  return runAllCompositionScenarios().every((s) => s.ok);
}

export function compositionReport(): string {
  return runAllCompositionScenarios()
    .map((s) => `${s.ok ? 'OK' : 'FAIL'} ${s.name} · ${s.evidence.join('; ')}${s.failures.length ? ' :: ' + s.failures.join(',') : ''}`)
    .join('\n');
}
