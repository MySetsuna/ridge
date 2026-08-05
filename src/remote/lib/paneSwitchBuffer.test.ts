import { describe, expect, it } from 'vitest';
import { PaneSwitchBuffer } from './paneSwitchBuffer';

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
});
