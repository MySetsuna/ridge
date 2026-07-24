/**
 * V-B3: pure helper — bump image preview version on external fs change.
 * Keeps cache-bust logic unit-testable without the full fileEditor store.
 */

export function nextImageVersion(current: number | undefined): number {
  return (current ?? 0) + 1;
}

/** Append `?v=` / `&v=` cache buster to an asset URL. */
export function imageUrlWithVersion(url: string, version: number | undefined): string {
  if (!version) return url;
  const sep = url.includes('?') ? '&' : '?';
  return `${url}${sep}v=${version}`;
}
