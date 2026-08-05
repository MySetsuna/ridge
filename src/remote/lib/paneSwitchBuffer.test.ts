import { describe, expect, it } from 'vitest';
import { MAX_PANE_SWITCH_DRAIN_BYTES, PaneSwitchBuffer } from './paneSwitchBuffer';

describe('PaneSwitchBuffer', () => {
  it('retains frame order across a keyed canvas gap and copies source bytes', () => {
    const buffer = new PaneSwitchBuffer(32);
    const source = new Uint8Array([1, 2]);
    buffer.enqueue('ws:pane', source);
    source[0] = 9;
    buffer.enqueue('ws:pane', new Uint8Array([3]));
    expect([...buffer.drain('ws:pane').frames.flatMap((f) => [...f])]).toEqual([1, 2, 3]);
  });

  it('bounds retained bytes and asks the caller to resync after shedding', () => {
    const buffer = new PaneSwitchBuffer(3);
    buffer.enqueue('ws:pane', new Uint8Array([1, 2]));
    buffer.enqueue('ws:pane', new Uint8Array([3, 4]));
    const drained = buffer.drain('ws:pane');
    expect(drained.needsResync).toBe(true);
    expect([...drained.frames.flatMap((f) => [...f])]).toEqual([3, 4]);
    expect(buffer.bytes).toBe(0);
  });

  it('never returns an unbounded synchronous switch backlog', () => {
    const buffer = new PaneSwitchBuffer(MAX_PANE_SWITCH_DRAIN_BYTES * 2);
    buffer.enqueue('ws:pane', new Uint8Array(MAX_PANE_SWITCH_DRAIN_BYTES));
    buffer.enqueue('ws:pane', new Uint8Array(32));
    const drained = buffer.drain('ws:pane');
    expect(drained.frames.reduce((n, frame) => n + frame.byteLength, 0)).toBeLessThanOrEqual(
      MAX_PANE_SWITCH_DRAIN_BYTES,
    );
    expect(drained.needsResync).toBe(true);
  });
});
