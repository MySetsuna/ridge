import { describe, it, expect } from 'vitest';
import { pathToUri, uriToPath, lspLanguageId, parseDefinition, parseHover } from './lspClient';

describe('lspClient URI helpers', () => {
  it('pathToUri: windows path', () => {
    expect(pathToUri('C:\\code\\wind\\src\\a.ts')).toBe('file:///C:/code/wind/src/a.ts');
  });
  it('pathToUri: posix path', () => {
    expect(pathToUri('/home/u/a.ts')).toBe('file:///home/u/a.ts');
  });
  it('pathToUri: encodes spaces', () => {
    expect(pathToUri('C:\\my code\\a.ts')).toBe('file:///C:/my%20code/a.ts');
  });
  it('uriToPath: windows roundtrip uses backslashes', () => {
    expect(uriToPath('file:///C:/code/wind/src/a.ts')).toBe('C:\\code\\wind\\src\\a.ts');
  });
  it('uriToPath: posix', () => {
    expect(uriToPath('file:///home/u/a.ts')).toBe('/home/u/a.ts');
  });
  it('uriToPath: decodes percent-encoding', () => {
    expect(uriToPath('file:///C:/my%20code/a.ts')).toBe('C:\\my code\\a.ts');
  });
  it('uriToPath: lowercase drive normalized to uppercase (avoid dup tabs)', () => {
    expect(uriToPath('file:///c:/code/a.ts')).toBe('C:\\code\\a.ts');
    expect(uriToPath('file:///c%3A/code/a.ts')).toBe('C:\\code\\a.ts');
  });
});

describe('lspLanguageId', () => {
  it('maps TS/JS family', () => {
    expect(lspLanguageId('a.ts')).toBe('typescript');
    expect(lspLanguageId('a.tsx')).toBe('typescriptreact');
    expect(lspLanguageId('a.js')).toBe('javascript');
    expect(lspLanguageId('a.jsx')).toBe('javascriptreact');
    expect(lspLanguageId('a.mts')).toBe('typescript');
    expect(lspLanguageId('main.rs')).toBe('rust');
  });
  it('returns null for unsupported', () => {
    expect(lspLanguageId('a.svelte')).toBeNull();
    expect(lspLanguageId('README.md')).toBeNull();
  });
});

describe('parseDefinition', () => {
  it('null → empty', () => {
    expect(parseDefinition(null)).toEqual([]);
  });
  it('single Location → 1-based target', () => {
    const loc = { uri: 'file:///home/u/a.ts', range: { start: { line: 9, character: 4 } } };
    expect(parseDefinition(loc)).toEqual([{ path: '/home/u/a.ts', line: 10, column: 5 }]);
  });
  it('Location[] → multiple', () => {
    const arr = [
      { uri: 'file:///home/u/a.ts', range: { start: { line: 0, character: 0 } } },
      { uri: 'file:///home/u/b.ts', range: { start: { line: 2, character: 1 } } },
    ];
    expect(parseDefinition(arr)).toEqual([
      { path: '/home/u/a.ts', line: 1, column: 1 },
      { path: '/home/u/b.ts', line: 3, column: 2 },
    ]);
  });
  it('LocationLink uses targetSelectionRange', () => {
    const link = {
      targetUri: 'file:///home/u/c.ts',
      targetRange: { start: { line: 5, character: 0 } },
      targetSelectionRange: { start: { line: 5, character: 9 } },
    };
    expect(parseDefinition(link)).toEqual([{ path: '/home/u/c.ts', line: 6, column: 10 }]);
  });
  it('LSP error envelope → empty', () => {
    expect(parseDefinition({ __lsp_error: { code: -32601, message: 'x' } })).toEqual([]);
  });
});

describe('parseHover', () => {
  it('null → null', () => {
    expect(parseHover(null)).toBeNull();
  });
  it('MarkupContent { kind, value }', () => {
    expect(parseHover({ contents: { kind: 'markdown', value: '**foo**: string' } })).toEqual({
      markdown: '**foo**: string',
    });
  });
  it('MarkedString { language, value } → fenced code', () => {
    expect(parseHover({ contents: { language: 'typescript', value: 'const x: number' } })).toEqual({
      markdown: '```typescript\nconst x: number\n```',
    });
  });
  it('plain string contents', () => {
    expect(parseHover({ contents: 'hello' })).toEqual({ markdown: 'hello' });
  });
  it('MarkedString[] joined with rule', () => {
    expect(parseHover({ contents: ['a', { language: 'ts', value: 'b' }] })).toEqual({
      markdown: 'a\n\n---\n\n```ts\nb\n```',
    });
  });
  it('empty contents → null', () => {
    expect(parseHover({ contents: '' })).toBeNull();
  });
  it('LSP error envelope → null', () => {
    expect(parseHover({ __lsp_error: { code: -1, message: 'x' } })).toBeNull();
  });
});
