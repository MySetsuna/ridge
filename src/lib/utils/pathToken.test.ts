import { describe, it, expect } from 'vitest';
import { pathTokenAt } from './pathToken';

describe('pathTokenAt', () => {
  it('extracts a relative path inside import quotes', () => {
    //              col→     1234567890123456789012345
    const line = "import x from './lib/foo.ts';";
    const col = line.indexOf('foo') + 1; // 光标落在 foo 上
    expect(pathTokenAt(line, col)).toEqual({ path: './lib/foo.ts' });
  });

  it('parses :line suffix', () => {
    const line = 'see src/app.ts:42 for details';
    const col = line.indexOf('app') + 1;
    expect(pathTokenAt(line, col)).toEqual({ path: 'src/app.ts', line: 42 });
  });

  it('parses :line:col suffix', () => {
    const line = '  at foo (src/x.ts:10:5)';
    const col = line.indexOf('x.ts') + 1;
    expect(pathTokenAt(line, col)).toEqual({ path: 'src/x.ts', line: 10, col: 5 });
  });

  it('keeps Windows drive colon, only strips trailing :line', () => {
    const line = 'open C:\\proj\\main.rs:7 now';
    const col = line.indexOf('main') + 1;
    expect(pathTokenAt(line, col)).toEqual({ path: 'C:\\proj\\main.rs', line: 7 });
  });

  it('does not mistake URL port for a line number', () => {
    const line = 'visit http://localhost:5173/x';
    const col = line.indexOf('localhost') + 1;
    expect(pathTokenAt(line, col)).toEqual({ path: 'http://localhost:5173/x' });
  });

  it('strips trailing sentence punctuation', () => {
    const line = 'edited ./README.md.';
    const col = line.indexOf('README') + 1;
    expect(pathTokenAt(line, col)).toEqual({ path: './README.md' });
  });

  it('returns null on whitespace / non-path', () => {
    expect(pathTokenAt('const a = 1;', 6)).not.toBeNull(); // 'a' 是合法 token（交给 LSP/resolver 判）
    expect(pathTokenAt('   ', 2)).toBeNull();
    expect(pathTokenAt('', 1)).toBeNull();
  });

  it('handles cursor at right edge of token', () => {
    const line = './foo.ts';
    // 光标在末尾（column = len+1）
    expect(pathTokenAt(line, line.length + 1)).toEqual({ path: './foo.ts' });
  });
});
