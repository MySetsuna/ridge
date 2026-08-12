import crypto from 'node:crypto';
import fs from 'node:fs';
import { describe, expect, it } from 'vitest';
import { syncGeneratedCsp } from './sync-generated-csp.mjs';

describe('generated CSP synchronizer', () => {
  it('hashes inline text using browser HTML line-ending normalization', () => {
    const html = '<meta http-equiv="Content-Security-Policy" content="script-src \'self\'; style-src \'self\'">\r\n<script>\r\nconsole.log(1);\r\n</script>';
    const expected = crypto.createHash('sha256').update('\nconsole.log(1);\n', 'utf8').digest('base64');
    expect(syncGeneratedCsp(html)).toContain(`'sha256-${expected}'`);
  });

  it('adds hashes for generated inline scripts and styles without weakening CSP', () => {
    const script = '\nwindow.__generated = true;\n';
    const style = '\nbody { color: red; }\n';
    const html = `<meta http-equiv="Content-Security-Policy" content="default-src 'self'; script-src 'self'; style-src 'self'"><script>${script}</script><style>${style}</style>`;
    const updated = syncGeneratedCsp(html);
    const scriptHash = crypto.createHash('sha256').update(script, 'utf8').digest('base64');
    const styleHash = crypto.createHash('sha256').update(style, 'utf8').digest('base64');
    expect(updated).toContain(`'sha256-${scriptHash}'`);
    expect(updated).toContain(`'sha256-${styleHash}'`);
    expect(updated).not.toContain("'unsafe-inline'");
  });

  it('leaves pages without a CSP meta unchanged', () => {
    const html = '<html><script>window.ok = true;</script></html>';
    expect(syncGeneratedCsp(html)).toBe(html);
  });

  it('allows cloud tenant signaling without opening arbitrary external origins', () => {
    const html = fs.readFileSync(new URL('../src/app.html', import.meta.url), 'utf8');
    const csp = html.match(/Content-Security-Policy"\s+content="([^"]+)"/i)?.[1] ?? '';
    expect(csp).toContain('http://*.localhost:5001');
    expect(csp).toContain('ws://*.localhost:5050');
    expect(csp).toContain('https://*.9527127.xyz');
    expect(csp).toContain('wss://*.9527127.xyz');
    expect(csp).not.toContain("connect-src *");
  });
});
