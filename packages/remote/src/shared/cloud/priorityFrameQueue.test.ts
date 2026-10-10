import { afterEach, describe, expect, it, vi } from 'vitest';
import { resetRemotePerfTrace, remotePerfSnapshot } from '../transport/remotePerfTrace';
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
  afterEach(() => { (globalThis as { __RIDGE_REMOTE_PERF_TRACE?: boolean }).__RIDGE_REMOTE_PERF_TRACE = false; resetRemotePerfTrace(); vi.restoreAllMocks(); });
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

  it('observes queue age, depth and overflow without changing priority or byte order', () => {
    (globalThis as { __RIDGE_REMOTE_PERF_TRACE?: boolean }).__RIDGE_REMOTE_PERF_TRACE = true;
    let at = 10;
    vi.spyOn(performance, 'now').mockImplementation(() => at);
    let allowed = false;
    const sent: number[] = [];
    const queue = new PriorityFrameQueue({ canSendPane: () => allowed, maxPendingPaneBytes: 4, send: frame => sent.push(frame[0] === CHANNEL.PANE_RAW ? frame[1] : 99) });
    queue.enqueue(pane(1, 3));
    at = 20;
    expect(queue.diagnostics).toMatchObject({ queueBytes: 3, queueDepth: 1, oldestAgeMs: 10, oldestAgeKnown: true });
    queue.enqueue(pane(2, 3));
    queue.enqueue(new Uint8Array([CHANNEL.JSON, 0]));
    expect(queue.diagnostics.droppedFrames).toBe(1);
    allowed = true; queue.resume();
    expect(sent).toEqual([99, 2]);
    expect(queue.diagnostics).toMatchObject({ queueDepth: 0, enqueuedFrames: 3, dequeuedFrames: 2, droppedFrames: 1 });
    expect(remotePerfSnapshot().samples.some(sample => sample.stage === 'transport-queue')).toBe(true);
  });

  it('does not timestamp frames off, and marks pre-enable queue age as unknown', () => {
    (globalThis as { __RIDGE_REMOTE_PERF_TRACE?: boolean }).__RIDGE_REMOTE_PERF_TRACE = false;
    const clock = vi.spyOn(performance, 'now');
    const queue = new PriorityFrameQueue({ canSendPane: () => false, send: () => {} });
    queue.enqueue(pane(1));
    expect(clock).not.toHaveBeenCalled();
    (globalThis as { __RIDGE_REMOTE_PERF_TRACE?: boolean }).__RIDGE_REMOTE_PERF_TRACE = true;
    expect(queue.diagnostics.oldestAgeKnown).toBe(false);
    expect(queue.diagnostics.oldestAgeMs).toBeUndefined();
    queue.clear();
    expect(queue.diagnostics.queueDepth).toBe(0);
  });
});
