import { afterEach, describe, expect, it, vi } from 'vitest';
import { resetRemotePerfTrace, remotePerfSnapshot } from '../transport/remotePerfTrace';
import { CHANNEL, encodeJsonFrame, isPaneOutputFrame } from '../transport/cloudMux';
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
  it('fragments large history RPC results through the data lane while ACKs retain control priority', () => {
    let allowed = false; const sent: Uint8Array[] = [];
    const queue = new PriorityFrameQueue({ canSendPane: () => allowed, send: frame => sent.push(frame) });
    const page = encodeJsonFrame({jsonrpc:'2.0',id:17,result:'a'.repeat(600_000)});
    const ack = encodeJsonFrame({jsonrpc:'2.0',id:18,result:{sequence:1,alreadyApplied:false}});
    expect(isPaneOutputFrame(page)).toBe(true);
    expect(queue.enqueue(page)).toBe(true);
    expect(queue.getPendingControlBytes()).toBe(0);
    queue.enqueue(ack); expect(sent).toEqual([ack]);
    allowed=true;queue.resume();expect(sent).toHaveLength(2);
    expect(sent[0]).toEqual(ack);
    expect(Buffer.from(sent[1]).equals(Buffer.from(page))).toBe(true);
    expect(isPaneOutputFrame(encodeJsonFrame({jsonrpc:'2.0',id:19,method:'write_terminal_input',params:{data:'a'.repeat(200_000)}}))).toBe(false);
  });

  it('keeps large canonical snapshots out of the control lane and bounds their atomic queue', () => {
    let allowed = false;
    const sent: Uint8Array[] = [];
    const dropped = vi.fn();
    const queue = new PriorityFrameQueue({ canSendPane: () => allowed, send: frame => sent.push(frame), onPaneDrop: dropped });
    const snapshot = new Uint8Array(DEFAULT_MAX_PENDING_PANE_BYTES + 1);
    snapshot[0] = CHANNEL.TERMINAL;
    expect(queue.enqueue(snapshot)).toBe(true);
    expect(queue.getPendingControlBytes()).toBe(0);
    expect(queue.getPendingPaneBytes()).toBe(snapshot.length);
    queue.enqueue(new Uint8Array([CHANNEL.JSON, 7]));
    expect(sent).toEqual([new Uint8Array([CHANNEL.JSON, 7])]);
    allowed = true;
    queue.resume();
    expect(sent[1]).toEqual(snapshot);
    const oversized = new Uint8Array(16 * 1024 * 1024 + 1);
    oversized[0] = CHANNEL.TERMINAL;
    expect(queue.enqueue(oversized)).toBe(false);
    expect(dropped).toHaveBeenCalledOnce();
    expect(queue.getPendingPaneBytes()).toBe(0);
  });
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
