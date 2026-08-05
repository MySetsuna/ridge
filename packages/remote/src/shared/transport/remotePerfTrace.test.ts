import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import {
  remotePerfEnd,
  remotePerfMark,
  remotePerfSnapshot,
  remotePerfStart,
  resetRemotePerfTrace,
} from './remotePerfTrace';

describe('remotePerfTrace', () => {
  beforeEach(() => {
    (globalThis as { __RIDGE_REMOTE_PERF_TRACE?: boolean }).__RIDGE_REMOTE_PERF_TRACE = true;
    resetRemotePerfTrace();
  });
  afterEach(() => {
    (globalThis as { __RIDGE_REMOTE_PERF_TRACE?: boolean }).__RIDGE_REMOTE_PERF_TRACE = false;
    resetRemotePerfTrace();
  });

  it('records bounded stage samples without payload retention', () => {
    const token = remotePerfStart('pane-switch', { paneKey: 'ws:pane' });
    remotePerfEnd(token, { bytes: 4 });
    remotePerfMark('raw-feed', { paneKey: 'ws:pane', bytes: 4, queueBytes: 0 });
    const snapshot = remotePerfSnapshot();
    expect(snapshot.enabled).toBe(true);
    expect(snapshot.samples).toEqual([
      expect.objectContaining({ stage: 'pane-switch', paneKey: 'ws:pane', bytes: 4 }),
      expect.objectContaining({ stage: 'raw-feed', paneKey: 'ws:pane', bytes: 4 }),
    ]);
  });
});
