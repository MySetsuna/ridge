/**
 * Keep the constrained Windows Job fallback opt-in for the CDP harness.
 * Production/default runs must not inherit a test-only escape hatch.
 */
export function applyKernelBreakawayPolicy(env, allowHarnessFallback = false) {
  if (allowHarnessFallback || env.RIDGE_CDP_ALLOW_NON_BREAKAWAY === '1') {
    env.RIDGE_TEST_ALLOW_NON_BREAKAWAY = '1';
  } else {
    delete env.RIDGE_TEST_ALLOW_NON_BREAKAWAY;
  }
  return env;
}

/**
 * WebView2 on this Windows host does not resolve tenant subdomains below
 * `.localhost` consistently. Keep the production URL/DNS contract untouched;
 * only a local CDP run gets an explicit loopback resolver rule.
 */
export function cloudHostResolverRule(baseDomain = '') {
  const host = String(baseDomain).trim().toLowerCase().split('/')[0].replace(/:\d+$/, '');
  return host === 'localhost' || host.endsWith('.localhost')
    ? ' --host-resolver-rules="MAP *.localhost 127.0.0.1,EXCLUDE localhost"'
    : '';
}

export function cloudBrowserNetworkArgs(baseDomain = '') {
  const resolver = cloudHostResolverRule(baseDomain);
  return resolver ? ` --no-proxy-server${resolver}` : '';
}
