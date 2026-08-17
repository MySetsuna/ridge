/** Select the Ridge WebView page, excluding blank and stale dev targets. */
export function isRidgeCdpTarget(target, expectedOrigin) {
  if (target?.type !== 'page' || !target.webSocketDebuggerUrl) return false;
  if (target.url === 'about:blank') return false;
  if ((target.title || '').toLowerCase() === 'about:blank') return false;
  if (expectedOrigin) {
    try {
      return new URL(target.url).origin === expectedOrigin;
    } catch {
      return false;
    }
  }
  return target.title === 'Ridge' || /(?:127\.0\.0\.1|localhost):\d+|tauri\.localhost/.test(target.url || '');
}
