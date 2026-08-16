import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { describe, expect, it } from 'vitest';
import { syncGeneratedCsp } from './sync-generated-csp.mjs';

const html = fs.readFileSync(path.resolve('src/app.html'), 'utf8');
const effectiveHtml = syncGeneratedCsp(html);
const csp = effectiveHtml.match(/Content-Security-Policy" content="([^"]+)"/)?.[1] ?? '';

describe('desktop app CSP', () => {
  it('authorizes every current inline bootstrap script', () => {
    const declared = new Set(csp.match(/'sha256-[^']+'/g) ?? []);
    const inline = [...html.matchAll(/<script(?:\s[^>]*)?>([\s\S]*?)<\/script>/gi)]
      .map(([, body]) => body.replace(/\r\n?/g, '\n'))
      .map((body) => `'sha256-${crypto.createHash('sha256').update(body, 'utf8').digest('base64')}'`)
      .filter((hash) => hash !== `'sha256-${crypto.createHash('sha256').update('', 'utf8').digest('base64')}'`);
    expect(inline.every((hash) => declared.has(hash))).toBe(true);
  });

  it('keeps runtime styles and bounded local WebView connections usable', () => {
    const styleSource = csp.match(/(?:^|;\s*)style-src\s+([^;]+)/)?.[1] ?? '';
    const styleAttributeSource = csp.match(/(?:^|;\s*)style-src-attr\s+([^;]+)/)?.[1] ?? '';
    expect(styleSource).not.toContain("'unsafe-inline'");
    expect(styleSource).toContain("'sha256-2ZuKPAtYnC8VI7pq7bkN4gYzSYWdHwyzJiRW4JiCF5c='");
    expect(styleAttributeSource).toContain("'unsafe-inline'");
    expect(csp).toContain('http://127.0.0.1:5174');
    expect(csp).toContain('ws://localhost:5174');

    const connectSources = csp.match(/(?:^|;\s*)connect-src\s+([^;]+)/)?.[1]?.split(/\s+/) ?? [];
    const wildcardSources = connectSources.filter((source) => source.includes('*'));
    expect(wildcardSources).toEqual([
      'http://*.localhost:5001',
      'https://*.localhost:5001',
      'http://*.localhost:5050',
      'https://*.localhost:5050',
      'ws://*.localhost:5001',
      'wss://*.localhost:5001',
      'ws://*.localhost:5050',
      'wss://*.localhost:5050',
      'https://*.9527127.xyz',
      'wss://*.9527127.xyz',
    ]);
    expect(csp).toContain('http://*.localhost:5001');
    expect(csp).toContain('ws://*.localhost:5050');
  });

  it('writes theme variables through the trusted bootstrap stylesheet', () => {
    expect(html).toContain('id="ridge-bootstrap-style"');
    expect(html).toContain('function ridgeBootstrapStyle()');
    expect(html).toContain('style.setProperty(name, value)');
  });
});
