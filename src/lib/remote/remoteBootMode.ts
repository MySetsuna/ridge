export type RemoteBootMode = 'lan' | 'cloud';

interface HostAddress {
  host: string;
  port: string | null;
}

function parseHost(value: string): HostAddress {
  const trimmed = value.trim().toLowerCase();
  if (!trimmed) return { host: '', port: null };

  if (trimmed.startsWith('[')) {
    const close = trimmed.indexOf(']');
    if (close >= 0) {
      const port = trimmed.slice(close + 1);
      return {
        host: trimmed.slice(1, close).replace(/\.$/, ''),
        port: /^:\d+$/.test(port) ? port.slice(1) : null,
      };
    }
  }

  const colon = trimmed.lastIndexOf(':');
  if (colon > 0 && colon === trimmed.indexOf(':')) {
    return {
      host: trimmed.slice(0, colon).replace(/\.$/, ''),
      port: /^\d+$/.test(trimmed.slice(colon + 1)) ? trimmed.slice(colon + 1) : null,
    };
  }

  return { host: trimmed.replace(/\.$/, ''), port: null };
}

function isLocalHost(host: string): boolean {
  if (host === 'localhost' || host.endsWith('.localhost') || host === '0.0.0.0' || host === '::1') {
    return true;
  }
  const octets = host.split('.');
  return octets.length === 4 && octets[0] === '127'
    && octets.slice(1).every((octet) => /^\d+$/.test(octet) && Number(octet) <= 255);
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

  const host = parseHost(hostname);
  const base = parseHost(baseDomain);
  const sameHost = host.host && base.host
    && (host.host === base.host || host.host.endsWith(`.${base.host}`));
  // Local cloud is a listener, so a different port means the bundle is LAN-served.
  const sameLocalPort = !isLocalHost(base.host) || host.port === base.port;
  if (sameHost && sameLocalPort) return 'cloud';
  return 'lan';
}
