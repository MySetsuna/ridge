import type { AgentHistoryReply } from '../../../packages/remote/src/shared/transport/wsRemote';

export interface CommuneHistoryRequest { workspaceId: string; limit?: number; query?: string; cursor?: string; textCursor?: string }
type Source = { rows: AgentHistoryReply[]; cursor?: string; loaded: boolean };
export const compareCommuneHistory = (a: AgentHistoryReply, b: AgentHistoryReply) => b.timestamp - a.timestamp
  || `${b.workspaceId}:${b.memberId}`.localeCompare(`${a.workspaceId}:${a.memberId}`)
  || (b.sequence ?? 0) - (a.sequence ?? 0);

/** Each source owns its buffer/cursor; failed reads never block healthy sources. */
export class CommuneHistoryPages {
  private sources: Map<string, Source>;
  readonly failures = new Map<string, string>();
  constructor(workspaceIds: readonly string[], private load: (request: CommuneHistoryRequest) => Promise<AgentHistoryReply[]>, private query = '') {
    this.sources = new Map([...new Set(workspaceIds.filter(Boolean))].sort().map(id => [id, { rows: [], loaded: false }]));
  }
  get hasMore(): boolean { return [...this.sources.values()].some(source => !source.loaded || source.rows.length > 0 || !!source.cursor); }
  fork(): CommuneHistoryPages {
    const copy = new CommuneHistoryPages([...this.sources.keys()], this.load, this.query);
    copy.sources = new Map([...this.sources].map(([id, source]) => [id, { ...source, rows: [...source.rows] }]));
    return copy;
  }
  async next(limit = 100): Promise<AgentHistoryReply[]> {
    this.failures.clear();
    const sources = new Map([...this.sources].map(([id, source]) => [id, { ...source, rows: [...source.rows] }]));
    const fill = async (workspaceId: string, source: Source) => {
      if (this.failures.has(workspaceId) || source.rows.length || (source.loaded && !source.cursor)) return;
      try {
        const page = await this.load({ workspaceId, limit: 50, query: this.query, cursor: source.cursor });
        if (page.some(row => row.workspaceId !== workspaceId)) throw new Error('Commune history source mismatch');
        const cursor = page.at(-1)?.cursor ?? undefined;
        if (cursor && cursor === source.cursor) throw new Error('Commune history cursor did not advance');
        source.rows = [...page].sort(compareCommuneHistory); source.cursor = cursor; source.loaded = true;
      } catch (error) { this.failures.set(workspaceId, error instanceof Error ? error.message : String(error)); }
    };
    const result: AgentHistoryReply[] = [];
    while (result.length < limit) {
      // Four concurrent reads bound global discovery without serializing every source.
      const entries = [...sources];
      for (let i = 0; i < entries.length; i += 4) await Promise.all(entries.slice(i, i + 4).map(([id, source]) => fill(id, source)));
      const candidate = entries.filter(([, source]) => source.rows.length).sort((a, b) => compareCommuneHistory(a[1].rows[0], b[1].rows[0]))[0];
      if (!candidate) break;
      result.push(candidate[1].rows.shift()!);
    }
    this.sources = sources;
    return result;
  }
}
