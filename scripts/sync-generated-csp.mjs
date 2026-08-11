import crypto from 'node:crypto';
import fs from 'node:fs';

const CSP_META = /(Content-Security-Policy\"\s+content=\")([^\"]+)(\")/i;

function inlineHashes(html, tag) {
  return [...html.matchAll(new RegExp(`<${tag}(?:\\s[^>]*)?>([\\s\\S]*?)</${tag}>`, 'gi'))]
    .map(([, body]) => body)
    .filter((body) => body.length > 0)
    .map((body) => `sha256-${crypto.createHash('sha256').update(body, 'utf8').digest('base64')}`);
}

function addDirectiveHashes(csp, directive, hashes) {
  const unique = [...new Set(hashes)];
  if (unique.length === 0) return csp;
  return csp.replace(new RegExp(`(^|;\\s*)${directive}\\s+([^;]+)`), (full, prefix, values) => {
    const tokens = new Set(values.split(/\s+/));
    unique.forEach((hash) => tokens.add(`'${hash}'`));
    return `${prefix}${directive} ${[...tokens].join(' ')}`;
  });
}

export function syncGeneratedCsp(html) {
  const match = html.match(CSP_META);
  if (!match) return html;
  let csp = match[2];
  csp = addDirectiveHashes(csp, 'script-src', inlineHashes(html, 'script'));
  csp = addDirectiveHashes(csp, 'style-src', inlineHashes(html, 'style'));
  return html.replace(CSP_META, `$1${csp}$3`);
}

export function syncGeneratedCspFile(filePath, fsImpl = fs) {
  const html = fsImpl.readFileSync(filePath, 'utf8');
  const updated = syncGeneratedCsp(html);
  if (updated !== html) fsImpl.writeFileSync(filePath, updated);
  return updated !== html;
}

if (process.argv[1] && process.argv[1].endsWith('sync-generated-csp.mjs')) {
  const files = process.argv.slice(2);
  for (const file of files) syncGeneratedCspFile(file);
}
