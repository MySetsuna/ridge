import { describe, expect, it } from 'vitest';
import {
  aggregateDrops,
  applyPumpBatch,
  formatPumpBadge,
  initialPumpState,
  orderSessionsForPump,
  pumpIntervalMs,
} from './livePumpPolicy';

describe('livePumpPolicy (C54)', () => {
  it('accepts under cap without drop', () => {
    let st = initialPumpState(100);
    const r = applyPumpBatch(st, { hostId: 'h', sessionId: 's', byteLength: 40 });
    expect(r.decision.dropBytes).toBe(0);
    expect(r.state.bufferedBytes).toBe(40);
    expect(r.decision.level).toBe('ok');
  });

  it('sheds when exceeding cap', () => {
    let st = initialPumpState(100);
    st = applyPumpBatch(st, { hostId: 'h', sessionId: 's', byteLength: 90 }).state;
    const r = applyPumpBatch(st, { hostId: 'h', sessionId: 's', byteLength: 30 });
    expect(r.state.bufferedBytes).toBe(100);
    expect(r.state.droppedBytes).toBeGreaterThan(0);
    expect(r.decision.level).toBe('shedding');
  });

  it('replaces buffer when single batch > cap', () => {
    const st = initialPumpState(50);
    const r = applyPumpBatch(st, { hostId: 'h', sessionId: 's', byteLength: 200 });
    expect(r.state.bufferedBytes).toBe(50);
    expect(r.state.droppedBytes).toBeGreaterThan(0);
  });

  it('orders sessions by fill ratio', () => {
    const order = orderSessionsForPump([
      { sessionId: 'full', bufferedBytes: 90, capBytes: 100 },
      { sessionId: 'empty', bufferedBytes: 0, capBytes: 100 },
    ]);
    expect(order[0]).toBe('empty');
  });

  it('aggregate and interval', () => {
    expect(aggregateDrops([initialPumpState(1), { ...initialPumpState(1), droppedBytes: 5 }])).toBe(
      5,
    );
    expect(pumpIntervalMs('shedding', 100)).toBeLessThan(pumpIntervalMs('ok', 100));
    expect(formatPumpBadge({ ...initialPumpState(100), droppedBytes: 10, bufferedBytes: 100 })).toMatch(
      /丢/,
    );
  });
});
