import { describe, expect, it } from 'vitest';
import { isRidgeCdpTarget } from './cdpTarget.mjs';

describe('Ridge CDP target selection', () => {
  it('rejects blank and non-page targets', () => {
    expect(isRidgeCdpTarget({ type: 'page', url: 'about:blank', webSocketDebuggerUrl: 'ws://blank' })).toBe(false);
    expect(isRidgeCdpTarget({ type: 'page', title: 'about:blank', url: 'http://127.0.0.1:5173/', webSocketDebuggerUrl: 'ws://blank' })).toBe(false);
    expect(isRidgeCdpTarget({ type: 'service_worker', url: 'http://127.0.0.1:5173', webSocketDebuggerUrl: 'ws://worker' })).toBe(false);
  });

  it('accepts the titled Ridge page or a local dev page', () => {
    expect(isRidgeCdpTarget({ type: 'page', title: 'Ridge', url: 'tauri://localhost', webSocketDebuggerUrl: 'ws://ridge' })).toBe(true);
    expect(isRidgeCdpTarget({ type: 'page', title: '', url: 'http://127.0.0.1:5173/', webSocketDebuggerUrl: 'ws://vite' })).toBe(true);
  });

  it('rejects a stale local dev page when the current origin is known', () => {
    const current = { type: 'page', title: 'Ridge', url: 'http://127.0.0.1:6000/', webSocketDebuggerUrl: 'ws://current' };
    const stale = { type: 'page', title: 'Ridge', url: 'http://127.0.0.1:3929/', webSocketDebuggerUrl: 'ws://stale' };
    expect(isRidgeCdpTarget(current, 'http://127.0.0.1:6000')).toBe(true);
    expect(isRidgeCdpTarget(stale, 'http://127.0.0.1:6000')).toBe(false);
  });
});
