import { describe, expect, it } from 'vitest';
import {
  decideForeignPaneBadge,
  foreignCloseConfirmMessage,
} from './foreignPaneStatus';

describe('decideForeignPaneBadge', () => {
  const base = {
    hostLabel: 'office-pc',
    attachedLocally: true,
    subscribed: true,
    reconnectAttempt: 0,
    hostStatus: 'connected' as const,
  };

  it('live when connected+subscribed+attached', () => {
    expect(decideForeignPaneBadge(base)).toEqual({
      kind: 'live',
      label: '远端 · office-pc',
    });
  });

  it('detached when local view closed', () => {
    expect(
      decideForeignPaneBadge({ ...base, attachedLocally: false }),
    ).toEqual({ kind: 'detached', label: '视图已断开 · 远端继续' });
  });

  it('reconnecting on host disconnect', () => {
    const b = decideForeignPaneBadge({
      ...base,
      hostStatus: 'disconnected',
      reconnectAttempt: 2,
    });
    expect(b.kind).toBe('reconnecting');
    if (b.kind === 'reconnecting') {
      expect(b.attempt).toBe(2);
      expect(b.label).toContain('#2');
    }
  });

  it('error surfaces detail', () => {
    expect(
      decideForeignPaneBadge({
        ...base,
        hostStatus: 'error',
        lastError: 'tls failed',
      }),
    ).toEqual({
      kind: 'error',
      label: 'office-pc 错误',
      detail: 'tls failed',
    });
  });
});

describe('foreignCloseConfirmMessage', () => {
  it('states detach not kill', () => {
    const m = foreignCloseConfirmMessage('staging');
    expect(m).toContain('本地视图');
    expect(m).toContain('继续运行');
    expect(m).not.toMatch(/结束进程|kill/i);
  });
});
