import { describe, expect, it } from 'vitest';
import {
  badgeForHostSession,
  buildHostRowAlerts,
  confirmDetachMessage,
  hostsHeaderSummary,
  hostsPollIntervalMs,
  hostStatusToLink,
  showReconnectControls,
  sortHostRows,
  summarizeOutbound,
  type HostRowModel,
} from './hostControlSurface';

const base = (over: Partial<HostRowModel> = {}): HostRowModel => ({
  id: 'lan:h',
  kind: 'remote',
  label: 'office',
  status: 'connected',
  sessionCount: 2,
  attachedCount: 1,
  reconnectAttempt: 0,
  ...over,
});

describe('hostControlSurface', () => {
  it('maps status and builds alerts', () => {
    expect(hostStatusToLink('error')).toBe('error');
    const alerts = buildHostRowAlerts(
      base({
        status: 'disconnected',
        reconnectAttempt: 2,
        outbound: {
          state: 'Disconnected',
          fanoutBytes: 0,
          writeOk: 0,
          liveBufferCap: 100,
          liveBufferBytes: 95,
          liveDroppedBytes: 10,
        },
      }),
    );
    expect(alerts.some((a) => a.includes('重连') || a.includes('丢弃') || a.includes('出站'))).toBe(
      true,
    );
  });

  it('badge and detach copy', () => {
    const b = badgeForHostSession({
      hostStatus: 'connected',
      hostLabel: 'office',
      attached: true,
      subscribed: true,
      reconnectAttempt: 0,
    });
    expect(b.kind).toBe('live');
    expect(confirmDetachMessage('office')).toMatch(/继续运行/);
  });

  it('reconnect controls and poll interval', () => {
    expect(showReconnectControls(base({ status: 'disconnected' }))).toBe(true);
    expect(showReconnectControls(base({ kind: 'headless' }))).toBe(false);
    const fast = hostsPollIntervalMs([base({ reconnectAttempt: 1, status: 'connecting' })]);
    expect(fast).toBeLessThanOrEqual(5000);
    expect(fast).toBeGreaterThanOrEqual(500);
  });

  it('sorts error first and summarizes', () => {
    const rows = sortHostRows([
      base({ id: 'b', label: 'b', status: 'connected' }),
      base({ id: 'a', label: 'a', status: 'error' }),
    ]);
    expect(rows[0].status).toBe('error');
    expect(hostsHeaderSummary(rows)).toMatch(/已连/);
    expect(
      summarizeOutbound(
        base({
          outbound: { state: 'Subscribed', fanoutBytes: 12, writeOk: 3, liveBufferCap: 100 },
        }),
      ),
    ).toMatch(/写3/);
  });
});
