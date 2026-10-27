import { describe, expect, it, vi } from 'vitest';
import { CommuneHistoryPages, type CommuneHistoryRequest } from './communeHistoryPages';
import type { AgentHistoryReply } from '../../../packages/remote/src/shared/transport/wsRemote';

const row = (workspaceId: string, timestamp: number): AgentHistoryReply => ({
  workspaceId, timestamp, id: String(timestamp), sequence: timestamp, memberId: 'same', sessionId: 'same',
  kind: 'agentMessage', agent: 'same', title: 'assistant', text: String(timestamp), cwd: '',
});

describe('global Commune history pagination', () => {
  it('orders same-millisecond sequence numbers numerically and isolates discarded page candidates', async () => {
    const pages = new CommuneHistoryPages(['a'], async () => [9, 10, 100].map(sequence => ({ ...row('a', 1), sequence })));
    expect((await pages.next(1))[0].sequence).toBe(100);
    const abandoned = pages.fork();
    expect((await abandoned.next(1))[0].sequence).toBe(10);
    expect((await pages.next(2)).map(item => item.sequence)).toEqual([10, 9]);
  });
  it('merges independently paged sources without dropping buffered rows or sharing opaque cursors', async () => {
    const sources = { a: Array.from({ length: 151 }, (_, i) => row('a', 302 - i * 2)), b: Array.from({ length: 151 }, (_, i) => row('b', 301 - i * 2)) };
    const load = vi.fn(async ({ workspaceId, cursor, limit = 50 }: CommuneHistoryRequest) => {
      expect(!cursor || cursor.startsWith(workspaceId + ':')).toBe(true);
      const offset = Number(cursor?.split(':')[1] ?? 0);
      const all = sources[workspaceId as keyof typeof sources];
      const page = all.slice(offset, offset + limit).map(value => ({ ...value }));
      if (offset + limit < all.length) page[page.length - 1].cursor = workspaceId + ':' + (offset + limit);
      return page;
    });
    const pages = new CommuneHistoryPages(['b', 'a'], load);
    const all: AgentHistoryReply[] = [];
    while (pages.hasMore) all.push(...await pages.next(100));
    expect(all.map(item => item.timestamp)).toEqual(Array.from({ length: 302 }, (_, i) => 302 - i));
    expect(new Set(all.map(item => item.workspaceId + ':' + item.id)).size).toBe(302);
    expect(load.mock.calls.every(([request]) => request.limit === 50)).toBe(true);
  });

  it('returns confirmed rows and retries the failed cursor without duplicates', async () => {
    let fail = true;
    const pages = new CommuneHistoryPages(['a'], async request => {
      if (request.cursor) {
        if (fail) throw Error('temporarily offline');
        return [row('a', 1)];
      }
      return [{ ...row('a', 3) }, { ...row('a', 2), cursor: 'a:2' }];
    });
    expect((await pages.next(3)).map(item => item.timestamp)).toEqual([3, 2]);
    expect(pages.failures.get('a')).toBe('temporarily offline');
    fail = false;
    expect((await pages.next(3)).map(item => item.timestamp)).toEqual([1]);
    expect(pages.failures.size).toBe(0);
    expect(pages.hasMore).toBe(false);
  });

  it('refuses rows from another source', async () => {
    const pages = new CommuneHistoryPages(['a'], async () => [row('b', 1)]);
    expect(await pages.next()).toEqual([]);
    expect(pages.failures.get('a')).toMatch('source mismatch');
    expect(pages.hasMore).toBe(true);
  });

  it('keeps healthy sources live while one source is unavailable, then recovers it', async () => {
    let offline = true;
    const load = vi.fn(async ({ workspaceId }: CommuneHistoryRequest) => {
      if (workspaceId === 'a' && offline) throw Error('offline');
      return [row(workspaceId, workspaceId === 'a' ? 3 : 2)];
    });
    const pages = new CommuneHistoryPages(['a', 'b'], load);
    expect((await pages.next()).map(item => item.timestamp)).toEqual([2]);
    expect(pages.failures.get('a')).toBe('offline');
    expect(load).toHaveBeenCalledTimes(2);
    offline = false;
    expect((await pages.next()).map(item => item.timestamp)).toEqual([3]);
    expect(pages.failures.size).toBe(0);
    expect(pages.hasMore).toBe(false);
  });
});
