import { describe, expect, it } from 'vitest';
import { summarizeExplorerPaste, type ExplorerPasteOutcome } from './explorerPaste';

const success = (source: string, target: string): ExplorerPasteOutcome => ({
  status: 'succeeded',
  source,
  target,
});

const failure = (source: string, target: string, error: string): ExplorerPasteOutcome => ({
  status: 'failed',
  source,
  target,
  error,
});

describe('summarizeExplorerPaste', () => {
  it('returns a success DTO when every operation succeeds', () => {
    expect(summarizeExplorerPaste([success('C:/a.txt', 'D:/a.txt')])).toEqual({
      status: 'succeeded',
      attempted: 1,
      succeeded: 1,
      failed: 0,
      failedPaths: [],
      failures: [],
    });
  });

  it('retains failed source/target/error details for a partial paste', () => {
    expect(
      summarizeExplorerPaste([
        success('C:/ok.txt', 'D:/ok.txt'),
        failure('C:/denied.txt', 'D:/denied.txt', 'os error 5'),
      ]),
    ).toEqual({
      status: 'partial',
      attempted: 2,
      succeeded: 1,
      failed: 1,
      failedPaths: ['C:/denied.txt'],
      failures: [{ source: 'C:/denied.txt', target: 'D:/denied.txt', error: 'os error 5' }],
    });
  });

  it('marks an all-failed paste and preserves retry paths in order', () => {
    expect(
      summarizeExplorerPaste([
        failure('C:/one.txt', 'D:/one.txt', 'denied'),
        failure('C:/two.txt', 'D:/two.txt', 'locked'),
      ]).failedPaths,
    ).toEqual(['C:/one.txt', 'C:/two.txt']);
    expect(
      summarizeExplorerPaste([
        failure('C:/one.txt', 'D:/one.txt', 'denied'),
        failure('C:/two.txt', 'D:/two.txt', 'locked'),
      ]).status,
    ).toBe('failed');
  });
});
