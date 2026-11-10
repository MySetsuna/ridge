import fs from 'node:fs';
import path from 'node:path';
import { describe, expect, it } from 'vitest';
import { transformMonacoStyles } from './monaco-trusted-styles.mjs';

const files = [
  'base/browser/domStylesheets.js',
  'base/browser/ui/contextview/contextview.js',
  'editor/standalone/browser/standaloneThemeService.js',
];

describe('installed Monaco strict CSP integration', () => {
  it.each(files)('guards the current library hooks in %s', file => {
    const id = path.resolve('node_modules/monaco-editor/esm/vs', file);
    const source = fs.readFileSync(id, 'utf8');
    const transformed = transformMonacoStyles(source, id);
    expect(transformed).not.toBeNull();
    expect(transformed.code).not.toBe(source);
    // A changed hook must never silently ship the unadapted library.
    expect(() => transformMonacoStyles('', id)).toThrow('Monaco CSP hook changed');
    expect(() => transformMonacoStyles(source + source, id)).toThrow('Monaco CSP hook changed');
  });

  it('leaves unrelated modules unchanged', () => {
    expect(transformMonacoStyles("document.createElement('style')", '/src/unrelated.js')).toBeNull();
  });
});
