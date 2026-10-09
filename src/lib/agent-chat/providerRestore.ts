// M4.2 — Provider restore helper (provider-truth round-trip guard).
//
// Wraps the JSON snapshot serialization so the Desktop UI cannot drift
// to a second source of provider truth. ProviderSpec is the only carrier
// of provider/model/effort across a save → load cycle.

export interface PersistedSessionShape {
  session_id: string;
  provider: {
    kind: 'mock' | 'anthropic' | 'open_ai' | 'opencode_acp';
    model: { id: string; tier: string; effort?: 'low' | 'medium' | 'high' };
    config: Record<string, unknown>;
  };
}

export function createSnapshot(session: PersistedSessionShape): string {
  return JSON.stringify(session);
}

export function restoreFromSnapshot(json: string): PersistedSessionShape {
  return JSON.parse(json) as PersistedSessionShape;
}
