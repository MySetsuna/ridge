/** Resolve a source-module URL for a dev server or preserve a browser-relative specifier. */
export function resolveCdpModuleUrl(devUrl, specifier) {
  if (!devUrl) return specifier;
  return new URL(specifier, devUrl.endsWith('/') ? devUrl : `${devUrl}/`).href;
}
