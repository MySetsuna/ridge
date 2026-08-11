import crypto from 'node:crypto';
import { describe, expect, it } from 'vitest';
import { syncGeneratedCsp } from './sync-generated-csp.mjs';

describe('generated CSP synchronizer', () => {
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
});
