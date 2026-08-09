import { describe, expect, it } from 'vitest';
import { isForeignOrigin, paneOriginBadge } from './paneOrigin';

describe('pane origin badge', () => {
  it('recognizes optional external origins', () => {
    expect(isForeignOrigin(undefined)).toBe(false);
    expect(isForeignOrigin({ kind: 'headless', host_id: 'h', host_label: '', session_id: 's' })).toBe(true);
  });

  it.each([
    [{ kind: 'headless', host_id: 'h', host_label: '', session_id: 's' }, 'HEADLESS'],
    [{ kind: 'remote', host_id: 'h', host_label: '', session_id: 's' }, 'LAN'],
    [{ kind: 'remote', host_id: 'h', host_label: 'alice', session_id: 's' }, 'alice'],
    [{ kind: 'rdg', host_id: 'h', host_label: '', session_id: 's' }, 'rdg'],
    [{ kind: 'rdg', host_id: 'h', host_label: 'box', session_id: 's' }, 'box'],
  ] as const)('maps %s to a stable badge', (origin, label) => {
    const badge = paneOriginBadge(origin);
    expect(badge.label).toBe(label);
    expect(badge.pillClass).toContain('border-');
    expect(badge.title).toBeTruthy();
  });
});
