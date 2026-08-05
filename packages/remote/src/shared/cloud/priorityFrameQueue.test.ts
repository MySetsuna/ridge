import { describe, expect, it, vi } from 'vitest';
import { CHANNEL } from '../transport/cloudMux';
import {
  DEFAULT_MAX_PENDING_PANE_BYTES,
  PriorityFrameQueue,
} from './priorityFrameQueue';

function pane(byte: number, size = 2): Uint8Array {
  const frame = new Uint8Array(size);
  frame[0] = CHANNEL.PANE_RAW;
  frame.fill(byte, 1);
  return frame;
}

describe('PriorityFrameQueue', () => {
  it('sends control before queued pane frames after the channel drains', () => {
    let allowed = false;
    const sent: number[] = [];
    const queue = new PriorityFrameQueue({
      canSendPane: () => allowed,
      send: (frame) => sent.push(frame[0] === CHANNEL.PANE_RAW ? frame[1] : 99),
    });

    queue.enqueue(pane(1));
    queue.enqueue(pane(2));
    queue.enqueue(new Uint8Array([CHANNEL.JSON, 7]));
    expect(sent).toEqual([99]);
    allowed = true;
    queue.resume();
    expect(sent).toEqual([99, 1, 2]);
  });

  it('bounds pending pane bytes and drops the oldest live tail', () => {
    const dropped = vi.fn();
    let allowed = false;
    const queue = new PriorityFrameQueue({
      canSendPane: () => allowed,
      maxPendingPaneBytes: 4,
      onPaneDrop: dropped,
      send: () => {},
    });
    queue.enqueue(pane(1, 3));
    queue.enqueue(pane(2, 3));
    expect(queue.getPendingPaneBytes()).toBe(3);
    expect(dropped).toHaveBeenCalledTimes(1);
    allowed = true;
    queue.resume();
    expect(queue.getPendingPaneBytes()).toBe(0);
  });

  it('does not retain a frame larger than the pane cap', () => {
    const dropped = vi.fn();
    const queue = new PriorityFrameQueue({
      canSendPane: () => false,
      onPaneDrop: dropped,
    });
    expect(queue.enqueue(pane(1, DEFAULT_MAX_PENDING_PANE_BYTES + 1))).toBe(false);
    expect(queue.getPendingPaneBytes()).toBe(0);
    expect(dropped).toHaveBeenCalledTimes(1);
  });
});
