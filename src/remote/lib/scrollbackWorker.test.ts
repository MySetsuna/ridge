import { describe, expect, it } from 'vitest';
import { decodeScrollback } from './scrollbackWorker';

describe('scrollback worker protocol', () => {
  it('decodes bounded page without owning pane state', () => {
    const result = decodeScrollback({ type: 'decode', requestId: 7, workspaceId: 'ws', paneId: 'p', startSeq: 1, endSeq: 2, bytes: new TextEncoder().encode('ok').buffer });
    expect(result?.text).toBe('ok');
    expect(result?.requestId).toBe(7);
  });
  it('rejects invalid or stale seq range', () => {
    expect(decodeScrollback({ type: 'decode', requestId: 1, workspaceId: 'ws', paneId: 'p', startSeq: 2, endSeq: 2, bytes: new ArrayBuffer(0) })).toBeNull();
  });
});
