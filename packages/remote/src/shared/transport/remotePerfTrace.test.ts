import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import {
  remotePerfEnd,
  remotePerfMark,
  remotePerfSamplePeerConnection,
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

  it('records WebRTC path facts without retaining the stats report', async () => {
    const report = [
      {
        type: 'candidate-pair',
        selected: true,
        localCandidateId: 'local',
        currentRoundTripTime: 0.042,
        availableOutgoingBitrate: 240_000,
      },
      { type: 'local-candidate', id: 'local', candidateType: 'relay' },
      { type: 'inbound-rtp', kind: 'application', bytesReceived: 1234, packetsLost: 2 },
      { type: 'outbound-rtp', kind: 'application', bytesSent: 5678 },
    ];
    let called = false;
    await remotePerfSamplePeerConnection({
      getStats: async () => {
        called = true;
        return { forEach: (visit: (value: unknown) => void) => report.forEach(visit) } as never;
      },
    });
    expect(called).toBe(true);
    expect(remotePerfSnapshot().samples).toContainEqual(expect.objectContaining({
      stage: 'transport-stats',
      candidateType: 'relay',
      rttMs: 42,
      availableOutgoingBitrate: 240_000,
      bytesSent: 5678,
      bytesReceived: 1234,
      packetsLost: 2,
    }));
  });
});
