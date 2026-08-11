import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { describe, expect, it } from 'vitest';

const html = fs.readFileSync(path.resolve('src/app.html'), 'utf8');
const csp = html.match(/Content-Security-Policy" content="([^"]+)"/)?.[1] ?? '';

describe('desktop app CSP', () => {
  it('authorizes every current inline bootstrap script', () => {
    const declared = new Set(csp.match(/'sha256-[^']+'/g) ?? []);
    const inline = [...html.matchAll(/<script(?:\s[^>]*)?>([\s\S]*?)<\/script>/gi)]
      .map(([, body]) => `'sha256-${crypto.createHash('sha256').update(body, 'utf8').digest('base64')}'`)
      .filter((hash) => hash !== `'sha256-${crypto.createHash('sha256').update('', 'utf8').digest('base64')}'`);
    expect(inline.every((hash) => declared.has(hash))).toBe(true);
  });

  it('keeps runtime styles and explicit local WebView connections usable', () => {
    expect(csp).not.toContain("'unsafe-inline'");
    expect(csp).toContain('http://127.0.0.1:5174');
    expect(csp).toContain('ws://localhost:5174');

    const connectSources = csp.match(/(?:^|;\s*)connect-src\s+([^;]+)/)?.[1]?.split(/\s+/) ?? [];
    const wildcardSources = connectSources.filter((source) => source.includes('*'));
    const approvedWildcardSources = new Set([
      'http://localhost:*',
      'https://localhost:*',
      'http://*.localhost:*',
      'https://*.localhost:*',
      'http://127.0.0.1:*',
      'https://127.0.0.1:*',
      'ws://localhost:*',
      'wss://localhost:*',
      'ws://*.localhost:*',
      'wss://*.localhost:*',
      'ws://127.0.0.1:*',
      'wss://127.0.0.1:*',
      'https://*.9527127.xyz',
      'wss://*.9527127.xyz',
    ]);
    expect(wildcardSources).not.toContain('*');
    expect(wildcardSources.every((source) => approvedWildcardSources.has(source))).toBe(true);
  });
});
