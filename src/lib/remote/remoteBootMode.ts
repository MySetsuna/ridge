export type RemoteBootMode = 'lan' | 'cloud';

function hostOnly(value: string): string {
  const trimmed = value.trim().toLowerCase().replace(/\.$/, '');
  if (!trimmed) return '';
  if (trimmed.startsWith('[')) {
    const close = trimmed.indexOf(']');
    return close >= 0 ? trimmed.slice(1, close) : trimmed;
  }
  return trimmed.split(':')[0];
}

/**
 * Select the transport before any network request.
 *
 * The cloud artifact only lives on BASE_DOMAIN or one of its tenant
 * subdomains (unless an explicit cloudHost query is present). A bundle served
 * by an rdg/LAN host must therefore enter the LAN TOTP/session path directly;
 * probing the public auth API first can leave the desktop shell waiting on an
 * unrelated cloud failure.
 */
export function remoteBootMode(
  hostname: string,
  search: string,
  baseDomain: string,
): RemoteBootMode {
  const query = new URLSearchParams(search);
  if (query.get('cloudHost')?.trim()) return 'cloud';

  const host = hostOnly(hostname);
  const base = hostOnly(baseDomain);
  if (host && base && (host === base || host.endsWith(`.${base}`))) return 'cloud';
  return 'lan';
}
