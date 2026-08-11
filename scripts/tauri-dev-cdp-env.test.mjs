import { describe, expect, it } from 'vitest';
import {
  applyKernelBreakawayPolicy,
  cloudBrowserNetworkArgs,
  cloudHostResolverRule,
} from './tauri-dev-cdp-env.mjs';

describe('tauri dev CDP kernel breakaway policy', () => {
  it('removes the test-only fallback by default', () => {
    const env = applyKernelBreakawayPolicy({ RIDGE_TEST_ALLOW_NON_BREAKAWAY: '1' });
    expect(env.RIDGE_TEST_ALLOW_NON_BREAKAWAY).toBeUndefined();
  });

  it('enables the fallback only with explicit harness opt-in', () => {
    const env = applyKernelBreakawayPolicy({ RIDGE_CDP_ALLOW_NON_BREAKAWAY: '1' });
    expect(env.RIDGE_TEST_ALLOW_NON_BREAKAWAY).toBe('1');
  });

  it('maps local cloud tenant subdomains to loopback for WebView2 only', () => {
    expect(cloudHostResolverRule('localhost:5050')).toContain('MAP *.localhost 127.0.0.1');
    expect(cloudHostResolverRule('tenant.localhost:5050')).toContain('MAP *.localhost 127.0.0.1');
    expect(cloudHostResolverRule('9527127.xyz')).toBe('');
  });

  it('keeps local WebView2 cloud traffic off the system proxy', () => {
    expect(cloudBrowserNetworkArgs('localhost:5050')).toContain('--no-proxy-server');
    expect(cloudBrowserNetworkArgs('localhost:5050')).toContain('--host-resolver-rules=');
    expect(cloudBrowserNetworkArgs('9527127.xyz')).toBe('');
  });
});
