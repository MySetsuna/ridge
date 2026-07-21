// §tree-persist（工作区树形选择器 store 接入 + 持久化）: the set of EXPANDED
// workspace ids in the mobile workspace/terminal tree, kept in a shared reactive
// store and mirrored to localStorage so a refresh / reconnect restores which
// workspaces the user had expanded — instead of collapsing everything on every
// remount ("每次都要重新载入").
//
// §data-realtime: ONLY the UI *preference* (which rows are expanded) is
// persisted. The actual workspace list + each workspace's panes stay LIVE —
// fetched from the host on connect and refreshed by the tree's poll — so nothing
// stale is ever shown. Persisting a preference is safe; persisting host data
// would risk showing terminals that no longer exist.

const LS_KEY_EXPANDED = 'rg-remote-tree-expanded';
const LS_KEY_SEEN = 'rg-remote-tree-seen';

function load(key: string): Set<string> {
  try {
    const raw = localStorage.getItem(key);
    if (!raw) return new Set();
    const arr: unknown = JSON.parse(raw);
    return Array.isArray(arr)
      ? new Set(arr.filter((x): x is string => typeof x === 'string'))
      : new Set();
  } catch {
    return new Set();
  }
}

// Shared reactive store. Reassign the sets (never mutate in place) so template
// reads like `treeState.expanded.has(id)` re-run on change.
//   • expanded — which workspace rows are open (persisted preference).
//   • seen     — workspace ids we've auto-expanded once on first appearance, so a
//                later manual collapse survives refresh instead of being re-seeded.
export const treeState = $state<{ expanded: Set<string>; seen: Set<string> }>({
  expanded: load(LS_KEY_EXPANDED),
  seen: load(LS_KEY_SEEN),
});

function persistExpanded(): void {
  try {
    localStorage.setItem(LS_KEY_EXPANDED, JSON.stringify([...treeState.expanded]));
  } catch {
    /* quota exceeded / storage disabled — keep the in-memory set */
  }
}
function persistSeen(): void {
  try {
    localStorage.setItem(LS_KEY_SEEN, JSON.stringify([...treeState.seen]));
  } catch {
    /* ignore */
  }
}

export function isWsExpanded(id: string): boolean {
  return treeState.expanded.has(id);
}

export function setWsExpanded(id: string, expanded: boolean): void {
  if (treeState.expanded.has(id) === expanded) return;
  const next = new Set(treeState.expanded);
  if (expanded) next.add(id);
  else next.delete(id);
  treeState.expanded = next;
  persistExpanded();
}

/** Toggle a workspace's expanded state; returns the NEW state. */
export function toggleWsExpanded(id: string): boolean {
  const now = !treeState.expanded.has(id);
  setWsExpanded(id, now);
  return now;
}

/** Auto-expand a workspace the FIRST time it's ever seen active, then never
 *  again — so the convenience default doesn't fight a later manual collapse
 *  across a refresh. Idempotent per id (guarded by the persisted `seen` set). */
export function seedActiveWorkspace(id: string): void {
  if (!id || treeState.seen.has(id)) return;
  treeState.seen = new Set(treeState.seen).add(id);
  persistSeen();
  setWsExpanded(id, true);
}

/** Drop expanded/seen ids for workspaces that no longer exist, so the persisted
 *  sets stay bounded and can't resurrect a closed workspace's row. */
export function pruneExpanded(liveIds: Set<string>): void {
  const keep = (src: Set<string>): { next: Set<string>; changed: boolean } => {
    let changed = false;
    const next = new Set<string>();
    for (const id of src) {
      if (liveIds.has(id)) next.add(id);
      else changed = true;
    }
    return { next, changed };
  };
  const e = keep(treeState.expanded);
  if (e.changed) { treeState.expanded = e.next; persistExpanded(); }
  const s = keep(treeState.seen);
  if (s.changed) { treeState.seen = s.next; persistSeen(); }
}
