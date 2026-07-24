import { describe, expect, it } from 'vitest';
import {
  aggregateDropped,
  backpressureLevel,
  bufferFillPercent,
  bytesToDropOnAppend,
  formatAggregateDropBadge,
  formatBackpressureBadge,
  hostsLineAlert,
  mergeOutboundIntoSnapshot,
  shouldAccelerateHostsPoll,
  shouldWarnOperator,
  snapshotFromOutboundStats,
} from './liveBackpressure';

describe('liveBackpressure', () => {
  it('levels by ratio and drops', () => {
    expect(
      backpressureLevel({
        bufferedBytes: 10,
        capBytes: 100,
        droppedBytes: 0,
        highWaterMark: 10,
      }),
    ).toBe('ok');
    expect(
      backpressureLevel({
        bufferedBytes: 80,
        capBytes: 100,
        droppedBytes: 0,
        highWaterMark: 80,
      }),
    ).toBe('elevated');
    expect(
      backpressureLevel({
        bufferedBytes: 50,
        capBytes: 100,
        droppedBytes: 1,
        highWaterMark: 100,
      }),
    ).toBe('shedding');
  });

  it('bytesToDropOnAppend matches append_capped overflow math', () => {
    expect(bytesToDropOnAppend(0, 10, 100)).toBe(0);
    expect(bytesToDropOnAppend(90, 20, 100)).toBe(10);
    expect(bytesToDropOnAppend(0, 200, 100)).toBe(100);
  });

  it('badge and warn', () => {
    const ok = {
      bufferedBytes: 1,
      capBytes: 100,
      droppedBytes: 0,
      highWaterMark: 1,
    };
    expect(shouldWarnOperator(ok)).toBe(false);
    expect(formatBackpressureBadge(ok)).toBe('');
    const bad = { ...ok, droppedBytes: 12, bufferedBytes: 100 };
    expect(shouldWarnOperator(bad)).toBe(true);
    expect(formatBackpressureBadge(bad)).toContain('丢弃');
  });

  it('snapshotFromOutboundStats + hostsLineAlert', () => {
    const snap = snapshotFromOutboundStats({
      liveBufferCap: 100,
      liveBufferBytes: 90,
      liveDroppedBytes: 0,
    });
    expect(backpressureLevel(snap)).toBe('elevated');
    expect(hostsLineAlert({ backpressure: snap, reconnectAttempt: 0 })).toContain('缓冲');
    expect(hostsLineAlert({ backpressure: snap, reconnectAttempt: 2 })).toBe('重连 #2');
  });

  it('merge / accelerate / aggregate product helpers', () => {
    const prev = snapshotFromOutboundStats({
      liveBufferCap: 100,
      liveBufferBytes: 10,
      liveDroppedBytes: 5,
    });
    const merged = mergeOutboundIntoSnapshot(prev, {
      liveBufferCap: 100,
      liveBufferBytes: 99,
      liveDroppedBytes: 3,
    });
    expect(merged.droppedBytes).toBe(5);
    expect(merged.highWaterMark).toBeGreaterThanOrEqual(99);
    expect(bufferFillPercent(merged)).toBe(99);
    const shed = { ...merged, droppedBytes: 1, bufferedBytes: 100 };
    expect(shouldAccelerateHostsPoll(shed)).toBe(true);
    const agg = aggregateDropped([shed, prev]);
    expect(agg.sheddingHosts).toBeGreaterThanOrEqual(1);
    expect(formatAggregateDropBadge(agg)).toMatch(/主机|丢/);
  });
});
