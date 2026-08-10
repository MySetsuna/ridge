/**
 * Keep the constrained Windows Job fallback opt-in for the CDP harness.
 * Production/default runs must not inherit a test-only escape hatch.
 */
export function applyKernelBreakawayPolicy(env) {
  if (env.RIDGE_CDP_ALLOW_NON_BREAKAWAY === '1') {
    env.RIDGE_TEST_ALLOW_NON_BREAKAWAY = '1';
  } else {
    delete env.RIDGE_TEST_ALLOW_NON_BREAKAWAY;
  }
  return env;
}
