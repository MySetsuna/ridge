import { describe, expect, it } from 'vitest';
import { applyKernelBreakawayPolicy } from './tauri-dev-cdp-env.mjs';

describe('tauri dev CDP kernel breakaway policy', () => {
  it('removes the test-only fallback by default', () => {
    const env = applyKernelBreakawayPolicy({ RIDGE_TEST_ALLOW_NON_BREAKAWAY: '1' });
    expect(env.RIDGE_TEST_ALLOW_NON_BREAKAWAY).toBeUndefined();
  });

  it('enables the fallback only with explicit harness opt-in', () => {
    const env = applyKernelBreakawayPolicy({ RIDGE_CDP_ALLOW_NON_BREAKAWAY: '1' });
    expect(env.RIDGE_TEST_ALLOW_NON_BREAKAWAY).toBe('1');
  });
});
