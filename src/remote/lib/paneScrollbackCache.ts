/**
 * paneScrollbackCache.ts
 *
 * Pure (DOM-free) core of the mobile remote's per-pane scrollback cache:
 * the byte buffers + the prune (GC) and replay-reconcile DECISIONS. Extracted
 * from MainApp.svelte so the cross-workspace prune fix (方案1) and the
 * "never shrink a longer cache" reconcile fix (方案2) are unit-testable
 * without a host or DOM globals (sessionStorage/btoa mirroring stays in the
 * Svelte shell, which drives this module by id sets it returns).
 *
 * §scrollback-cache background: the mobile view uses a single shared terminal
 * kernel, so switching panes wipes it (resetForSwitch) and repaints from this
 * cache, while the host replays ≤64KB scrollback on (re)subscribe.
 */

export const PANE_BUF_CAP = 256 * 1024;

/** True if `hay` ends with the byte sequence `tail`. */
export function bytesEndsWith(hay: Uint8Array, tail: Uint8Array): boolean {
  if (tail.length === 0) return true;
  if (tail.length > hay.length) return false;
  const off = hay.length - tail.length;
  for (let i = 0; i < tail.length; i++) if (hay[off + i] !== tail[i]) return false;
  return true;
}

/** Decision returned by reconcileReplay: keep the local cache (drop the host
 *  replay — already pre-painted) or repaint the kernel from `buffer`. */
export interface ReconcileResult {
  action: 'keep' | 'repaint';
  buffer: Uint8Array;
}

export class PaneScrollbackCache {
  private buffers = new Map<string, Uint8Array>();
  // Which workspace each cached pane belongs to. Drives the prune fix (方案1):
  // we only GC a pane that vanished from ITS OWN workspace's list, so switching
  // to another workspace (whose list-panes omits this pane) never deletes it.
  private paneWorkspace = new Map<string, string>();

  constructor(private readonly cap: number = PANE_BUF_CAP) {}

  has(id: string): boolean { return this.buffers.has(id); }
  get(id: string): Uint8Array | undefined { return this.buffers.get(id); }
  /** The ids of every pane currently cached (across all workspaces). */
  liveIds(): string[] { return [...this.buffers.keys()]; }

  /** Set the full buffer for a pane (trimmed to cap), optionally tagging its
   *  workspace. Used for the host-replay repaint path and tests. */
  set(id: string, data: Uint8Array, workspaceId?: string): void {
    this.buffers.set(id, data.length > this.cap ? data.slice(data.length - this.cap) : data.slice());
    if (workspaceId) this.paneWorkspace.set(id, workspaceId);
  }

  /** Append live output to a pane's buffer, keeping only the last `cap` bytes.
   *  Optionally tag the pane's workspace so an untagged live pane is still
   *  protected from cross-workspace prune before its next `panes` list. */
  append(id: string, data: Uint8Array, workspaceId?: string): void {
    const prev = this.buffers.get(id);
    let next: Uint8Array;
    if (!prev) { next = data.slice(); }
    else { next = new Uint8Array(prev.length + data.length); next.set(prev); next.set(data, prev.length); }
    if (next.length > this.cap) next = next.slice(next.length - this.cap);
    this.buffers.set(id, next);
    if (workspaceId) this.paneWorkspace.set(id, workspaceId);
  }

  /**
   * 方案1 prune(子方案 B):a fresh `panes` list arrived for the CURRENT active
   * workspace. Its ids all belong to `activeWsId`. We:
   *  - delete only caches tagged as `activeWsId` that are NOT in `livePaneIds`
   *    (i.e. panes truly closed inside this workspace) → memory GC kept;
   *  - (re)tag every live pane as `activeWsId`;
   *  - leave OTHER workspaces' caches untouched → cross-workspace switch-back
   *    never loses scrollback (the core bug).
   *
   * Returns the surviving pane ids (across ALL workspaces) so the caller can
   * hand the same authoritative live-set to ws.pruneOutputs (which would
   * otherwise over-prune cross-workspace output buffers).
   */
  pruneCurrentWorkspace(activeWsId: string, livePaneIds: string[]): { survivingIds: string[] } {
    const live = new Set(livePaneIds);
    for (const id of [...this.buffers.keys()]) {
      // Only a pane that belongs to the active workspace yet vanished from its
      // list was truly closed → release it. Panes of other workspaces stay.
      if (this.paneWorkspace.get(id) === activeWsId && !live.has(id)) {
        this.buffers.delete(id);
        this.paneWorkspace.delete(id);
      }
    }
    // Tag every live pane as belonging to the current workspace.
    for (const id of livePaneIds) this.paneWorkspace.set(id, activeWsId);
    return { survivingIds: [...this.buffers.keys()] };
  }

  /**
   * 方案1 fallback:a workspace was closed (its id dropped from list-workspaces).
   * Release caches of panes tagged to any workspace that no longer exists —
   * those panes can never reappear, so they'd otherwise leak. Untagged panes
   * are left alone (no premature GC). Returns the removed pane ids so the
   * caller can clear their sessionStorage mirrors too.
   */
  pruneClosedWorkspaces(liveWorkspaceIds: string[]): string[] {
    const liveWs = new Set(liveWorkspaceIds);
    const removed: string[] = [];
    for (const id of [...this.buffers.keys()]) {
      const ws = this.paneWorkspace.get(id);
      if (ws !== undefined && !liveWs.has(ws)) {
        this.buffers.delete(id);
        this.paneWorkspace.delete(id);
        removed.push(id);
      }
    }
    return removed;
  }

  /**
   * 方案2 reconcile:the first chunk after (re)subscribe is the host's on-subscribe
   * scrollback replay (≤64KB tail). Decide whether to keep our (≤256KB) local
   * cache or repaint from the replay:
   *  - no cache            → repaint from replay (authoritative; write it in);
   *  - cache tail-matches  → keep (we already pre-painted it);
   *  - cache LONGER than replay → keep (local has more history; the 64KB tail
   *    must not overwrite/shrink it — the original shrink bug);
   *  - otherwise (cache not longer and no tail-match → pane changed) → repaint.
   * On 'repaint' the returned buffer is also written back as the new cache.
   */
  reconcileReplay(id: string, replay: Uint8Array): ReconcileResult {
    const cached = this.buffers.get(id);
    if (cached && (bytesEndsWith(cached, replay) || cached.length > replay.length)) {
      return { action: 'keep', buffer: cached };
    }
    const buf = replay.length > this.cap ? replay.slice(replay.length - this.cap) : replay.slice();
    this.buffers.set(id, buf);
    return { action: 'repaint', buffer: buf };
  }
}
