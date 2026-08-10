/** Select the Ridge WebView page, excluding WebView2's about:blank target. */
export function isRidgeCdpTarget(target) {
  if (target?.type !== 'page' || !target.webSocketDebuggerUrl) return false;
  if (target.url === 'about:blank') return false;
  if ((target.title || '').toLowerCase() === 'about:blank') return false;
  return target.title === 'Ridge' || /(?:127\.0\.0\.1|localhost):\d+|tauri\.localhost/.test(target.url || '');
}
