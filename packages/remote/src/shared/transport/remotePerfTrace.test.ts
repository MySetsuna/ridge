import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
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
    vi.restoreAllMocks();
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

  it('does no sampling or clock work when disabled', async () => {
    (globalThis as { __RIDGE_REMOTE_PERF_TRACE?: boolean }).__RIDGE_REMOTE_PERF_TRACE = false;
    const clock = vi.spyOn(performance, 'now');
    const getStats = vi.fn();
    remotePerfMark('input-rpc', { bytes: 1 });
    expect(remotePerfStart('input-rpc')).toBeNull();
    await remotePerfSamplePeerConnection({ getStats });
    expect(clock).not.toHaveBeenCalled();
    expect(getStats).not.toHaveBeenCalled();
    expect(remotePerfSnapshot().samples).toEqual([]);
    clock.mockRestore();
  });

  it('reports truncation and rejects payload fields, invalid numbers and snapshot mutation', () => {
    for (let i = 0; i < 300; i++) remotePerfMark('raw-feed', { bytes: i });
    expect(remotePerfSnapshot().samples).toHaveLength(256);
    expect(remotePerfSnapshot().droppedSamples).toBe(44);
    remotePerfMark('raw-feed', { bytes: NaN, queueBytes: Infinity, paneKey: 'x'.repeat(300), payload: 'private-terminal-text', address: '192.0.2.1', at: -1 } as never);
    const snapshot = remotePerfSnapshot();
    const last = snapshot.samples.at(-1)!;
    expect(last.bytes).toBeUndefined();
    expect(last.queueBytes).toBeUndefined();
    expect(last.paneKey).toHaveLength(96);
    expect(last.at).toBeGreaterThanOrEqual(0);
    expect(JSON.stringify(snapshot)).not.toMatch(/private-terminal-text|192\.0\.2\.1/);
    last.paneKey = 'mutated';
    expect(remotePerfSnapshot().samples.at(-1)?.paneKey).not.toBe('mutated');
    resetRemotePerfTrace();
    expect(remotePerfSnapshot().droppedSamples).toBe(0);
  });

  it('resolves the transport selected pair instead of an unrelated nominated pair', async () => {
    const report = [
      { type: 'transport', selectedCandidatePairId: 'actual' },
      { type: 'candidate-pair', id: 'actual', localCandidateId: 'local', remoteCandidateId: 'remote', currentRoundTripTime: 0.01 },
      { type: 'local-candidate', id: 'local', candidateType: 'host', protocol: 'udp', address: '192.0.2.1' },
      { type: 'remote-candidate', id: 'remote', candidateType: 'relay', address: '192.0.2.2' },
      { type: 'data-channel', dataChannelIdentifier: 5, bytesSent: 77 },
      { type: 'data-channel', dataChannelIdentifier: 2, bytesSent: 66 },
      { type: 'candidate-pair', id: 'wrong', nominated: true, localCandidateId: 'wrong-local', currentRoundTripTime: 8 },
      { type: 'local-candidate', id: 'wrong-local', candidateType: 'srflx' },
    ];
    const pc = { getStats: async () => ({ forEach: (visit: (v: unknown) => void) => report.forEach(visit) }) as never };
    await remotePerfSamplePeerConnection(pc, { id: 5, ordered: false, maxRetransmits: 0, maxPacketLifeTime: null, bufferedAmount: 12 });
    expect(remotePerfSnapshot().samples).toContainEqual(expect.objectContaining({ candidateType: 'host', remoteCandidateType: 'relay', protocol: 'udp', rttMs: 10, ordered: false, maxRetransmits: 0, queueBytes: 12, channelId: 5, bytesSent: 77 }));
    expect(JSON.stringify(remotePerfSnapshot())).not.toMatch(/192\.0\.2\./);
    resetRemotePerfTrace();
    await remotePerfSamplePeerConnection(pc);
    expect(remotePerfSnapshot().samples).toHaveLength(1);
    expect(remotePerfSnapshot().samples[0].bytesSent).toBeUndefined();
  });
});
