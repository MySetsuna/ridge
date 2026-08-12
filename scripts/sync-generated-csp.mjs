import crypto from 'node:crypto';
import fs from 'node:fs';

const CSP_META = /(Content-Security-Policy"\s+content=")([^"]+)(")/i;

/** @param {string} html @param {string} tag @returns {string[]} */
function inlineHashes(html, tag) {
  const pattern = String.raw`<${tag}(?:\s[^>]*)?>([\s\S]*?)</${tag}>`;
  return [...html.matchAll(new RegExp(pattern, 'gi'))]
    .map(([, body]) => body)
    .filter((body) => body.length > 0)
    // HTML tokenization normalizes CRLF/CR to LF before browsers hash inline
    // script/style text for CSP. Match that canonical form in dev and builds.
    .map((body) => body.replace(/\r\n?/g, '\n'))
    .map((body) => `sha256-${crypto.createHash('sha256').update(body, 'utf8').digest('base64')}`);
}

/** @param {string} csp @param {string} directive @param {string[]} hashes @returns {string} */
function addDirectiveHashes(csp, directive, hashes) {
  const unique = [...new Set(hashes)];
  if (unique.length === 0) return csp;
  const pattern = String.raw`(^|;\s*)${directive}\s+([^;]+)`;
  const match = new RegExp(pattern).exec(csp);
  if (!match) return csp;
  const [, prefix, values] = match;
  const tokens = new Set(values.split(/\s+/));
  unique.forEach((hash) => tokens.add(`'${hash}'`));
  const replacement = `${prefix}${directive} ${[...tokens].join(' ')}`;
  const start = match.index ?? 0;
  return csp.slice(0, start) + replacement + csp.slice(start + match[0].length);
}

/** @param {string} html @returns {string} */
export function syncGeneratedCsp(html) {
  const match = CSP_META.exec(html);
  if (!match) return html;
  let csp = match[2];
  csp = addDirectiveHashes(csp, 'script-src', inlineHashes(html, 'script'));
  csp = addDirectiveHashes(csp, 'style-src', inlineHashes(html, 'style'));
  return html.replace(CSP_META, `$1${csp}$3`);
}

/** @param {string} filePath @param {typeof fs} [fsImpl] @returns {boolean} */
export function syncGeneratedCspFile(filePath, fsImpl = fs) {
  const html = fsImpl.readFileSync(filePath, 'utf8');
  const updated = syncGeneratedCsp(html);
  if (updated !== html) fsImpl.writeFileSync(filePath, updated);
  return updated !== html;
}

if (process.argv[1]?.endsWith('sync-generated-csp.mjs')) {
  const files = process.argv.slice(2);
  for (const file of files) syncGeneratedCspFile(file);
}
