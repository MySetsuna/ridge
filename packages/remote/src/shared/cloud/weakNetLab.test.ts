// 弱网实验室参数化扫描（iteration 7 G2，差距 R1 实验室轨）。
//
// **实验室确定性模型，非真机结论**：fake RTC/WS + fake timers 下扫描三族行为轴，
// 断言不变量（场景全绿）并把每场景指标累积写入 artifacts/weak-net-lab/metrics.json
// （gitignored；scripts/run-weaknet-lab.mjs 负责触发与结构校验）。真实 TURN-only/
// 蜂窝/后台冻结指标只能来自真机 runbook（用户轨），本文件不冒充。

import { mkdirSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { afterAll, afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { CloudHostBridge } from './cloudHostBridge';
import { encodeJsonFrame } from '../transport/cloudMux';
import {
  authorize,
  completeE2ee,
  createControllerRig,
  FaultPeerConnection,
  FaultWebSocket,
  installFaultGlobals,
} from './__faultRig';

vi.mock('./apiClient', async (importOriginal) => {
  const actual = await importOriginal<typeof import('./apiClient')>();
  return {
    ...actual,
    getIceServers: vi.fn(async () => ({ iceServers: [{ urls: 'stun:test.invalid' }] })),
  };
});

interface ScenarioMetric {
  family: string;
  params: Record<string, unknown>;
  observed: Record<string, unknown>;
}

const metrics: ScenarioMetric[] = [];

const OUT_DIR = resolve(import.meta.dirname, '../../../../..', 'artifacts', 'weak-net-lab');

afterAll(() => {
  mkdirSync(OUT_DIR, { recursive: true });
  writeFileSync(
    resolve(OUT_DIR, 'metrics.json'),
    JSON.stringify(
      {
        model: 'deterministic-lab',
        disclaimer:
          '实验室确定性模型（fake RTC/WS/timers）；不是真机/真实网络结论，不得用于宣称双平台或生产弱网表现。',
        scenarios: metrics,
      },
      null,
      2,
    ),
  );
});

describe('弱网实验室 — A. disconnected 脉冲时长扫描（15s watchdog / 12s ICE deadline 邻域）', () => {
  beforeEach(() => {
    FaultPeerConnection.instances = [];
    FaultWebSocket.instances = [];
    installFaultGlobals();
    vi.useFakeTimers();
    vi.spyOn(Math, 'random').mockReturnValue(0);
    vi.spyOn(console, 'log').mockImplementation(() => {});
    vi.spyOn(console, 'warn').mockImplementation(() => {});
  });
  afterEach(() => {
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  for (const pulseMs of [1_000, 5_000, 14_000]) {
    it(`脉冲 ${pulseMs}ms（< 15s watchdog）自愈：零 ICE restart、零重建、零重复恢复`, async () => {
      const rig = await createControllerRig();
      const subscribe = vi.fn(() => rig.rpc.notify('subscribe-pane', { paneId: 'pane-a' }));
      rig.rpc.onReconnected(subscribe);
      const sentBefore = rig.dc.sent.length;

      rig.pc.connectionState = 'disconnected';
      rig.pc.onconnectionstatechange?.();
      await vi.advanceTimersByTimeAsync(pulseMs);
      rig.pc.connectionState = 'connected';
      rig.pc.onconnectionstatechange?.();

      expect(rig.pc.offers).toHaveLength(0);
      expect(FaultPeerConnection.instances).toHaveLength(1);
      expect(rig.adapter.authState()).toBe('authorized');
      expect(subscribe).not.toHaveBeenCalled();
      expect(rig.dc.sent).toHaveLength(sentBefore);
      expect(vi.getTimerCount()).toBe(0);
      metrics.push({
        family: 'pulse-self-heal',
        params: { pulseMs },
        observed: { iceRestarts: rig.pc.offers.length, rebuilds: 0, duplicateRecoveries: 0 },
      });

      rig.rpc.dispose();
      rig.adapter.close();
      rig.adapter.dispose();
    });
  }

  it('脉冲 15s+12s（越过 watchdog 与 ICE deadline）：恰一次 ICE restart 后升级整体重建，恢复恰一次', async () => {
    const rig = await createControllerRig();
    const subscribe = vi.fn(() => rig.rpc.notify('subscribe-pane', { paneId: 'pane-a' }));
    rig.rpc.onReconnected(subscribe);

    rig.pc.connectionState = 'disconnected';
    rig.pc.onconnectionstatechange?.();
    await vi.advanceTimersByTimeAsync(16_000);
    expect(rig.pc.offers).toEqual([{ iceRestart: true }]);
    await vi.advanceTimersByTimeAsync(12_000);
    expect(FaultPeerConnection.instances).toHaveLength(2);

    const pc2 = FaultPeerConnection.instances[1];
    const ws2 = FaultWebSocket.instances[1];
    ws2.fireOpen();
    const next = completeE2ee(pc2, ws2);
    authorize(next.dc, next.hostSession, 0);
    expect(rig.adapter.authState()).toBe('authorized');
    expect(subscribe).toHaveBeenCalledTimes(1);
    expect(vi.getTimerCount()).toBe(0);
    metrics.push({
      family: 'watchdog-escalation',
      params: { pulseMs: 28_000 },
      observed: { iceRestarts: 1, rebuilds: 1, duplicateRecoveries: 0 },
    });

    rig.rpc.dispose();
    rig.adapter.close();
    rig.adapter.dispose();
  });

  for (const cycles of [10, 50]) {
    it(`fail/recover ${cycles} 周期：零 pending 泄漏、零 timer 泄漏、恢复恰 ${cycles} 次`, async () => {
      const rig = await createControllerRig();
      const subscribe = vi.fn(() => rig.rpc.notify('subscribe-pane', { paneId: 'pane-a' }));
      rig.rpc.onReconnected(subscribe);

      for (let cycle = 1; cycle <= cycles; cycle += 1) {
        rig.pc.connectionState = 'failed';
        rig.pc.onconnectionstatechange?.();
        await vi.advanceTimersByTimeAsync(1000);
        rig.pc.connectionState = 'connected';
        rig.pc.onconnectionstatechange?.();
        authorize(rig.dc, rig.hostSession, cycle);
        expect(rig.rpc.inFlight).toBe(0);
        expect(vi.getTimerCount()).toBe(0);
      }
      expect(subscribe).toHaveBeenCalledTimes(cycles);
      metrics.push({
        family: 'fail-recover-cycles',
        params: { cycles },
        observed: {
          recoveries: cycles,
          pendingLeaks: rig.rpc.inFlight,
          timerLeaks: vi.getTimerCount(),
        },
      });

      rig.rpc.dispose();
      rig.adapter.close();
      rig.adapter.dispose();
    });
  }
});

describe('弱网实验室 — B. DataChannel 背压水位扫描（8MiB 上水位邻域）', () => {
  const MIB = 1024 * 1024;
  for (const buffered of [1 * MIB, 8 * MIB + 1, 12 * MIB]) {
    const overHigh = buffered > 8 * MIB;
    it(`buffered=${(buffered / MIB).toFixed(0)}MiB → ${overHigh ? '丢帧并在 drain 后每 pane 恰一次 resync' : '直发不丢'}`, async () => {
      const invoke = vi.fn(async (_method: string, _params?: Record<string, unknown>) => ({
        frame: '\x1bcRECOVERED',
      }));
      const sent: Uint8Array[] = [];
      const bridge = new CloudHostBridge({ invoke, sendFrame: (frame) => sent.push(frame) });
      let level = buffered;
      let drain: (() => void) | null = null;
      bridge.attachChannelControl({
        bufferedAmount: () => level,
        onDrained: (cb) => {
          drain = cb;
          return () => {};
        },
      });
      const panes = ['pane-a', 'pane-b', 'pane-c'];
      bridge.handleFrame(encodeJsonFrame({
        jsonrpc: '2.0',
        method: 'subscribe-pane',
        params: { paneId: panes[0], active: true },
      }));
      for (const paneId of panes) bridge.pushPaneOutput(paneId, new Uint8Array([1, 2, 3]));

      if (buffered <= MIB) {
        expect(sent).toHaveLength(1);
        metrics.push({
          family: 'backpressure',
          params: { bufferedMiB: buffered / MIB, panes: panes.length },
          observed: { dropped: panes.length - 1, resyncs: 0 },
        });
        return;
      }
      if (!overHigh) {
        expect(sent).toHaveLength(1);
        metrics.push({
          family: 'backpressure',
          params: { bufferedMiB: buffered / MIB, panes: panes.length },
          observed: { dropped: panes.length - 1, resyncs: 0 },
        });
        return;
      }

      expect(sent).toHaveLength(0);
      level = 0;
      drain!();
      await vi.waitFor(() => expect(sent).toHaveLength(1));
      const resyncs = invoke.mock.calls.filter(([m]) => m === 'get_pane_resync_frame');
      expect(resyncs).toHaveLength(1);
      expect((resyncs[0][1] as { paneId: string }).paneId).toBe(panes[0]);
      metrics.push({
        family: 'backpressure',
        params: { bufferedMiB: buffered / MIB, panes: panes.length },
        observed: { dropped: panes.length, resyncs: 1 },
      });
    });
  }
});
