/**
 * layoutEvent.ts — typed parser for the `teammate-layout-changed` Tauri event.
 *
 * The Rust backend (`src-tauri/src/teammate/layout_event.rs`) emits a single
 * discriminated envelope `{ kind, ...payload }` for every layout change. This
 * module normalizes that wire shape into a front-end `LayoutChange` so the
 * `+page.svelte` handler can dispatch deterministically on `kind` instead of
 * blindly re-syncing on every event.
 *
 * Any unrecognized / legacy payload (the historical `()` / `null` / ad-hoc
 * shapes) degrades to a generic `state` re-sync, so the handler can never
 * break on an unexpected payload.
 */

/**
 * Discriminant shared with the backend envelope (`teammate::layout_event`).
 *
 * `state` is the GENERIC catch-all bucket: every notification that has no
 * pane-specific payload (agent register/release, rename, new-window, summon) —
 * and every legacy / unrecognized payload (via the fallback) — maps to `state`,
 * meaning "something changed, re-sync the whole layout". The other kinds carry
 * a pane id and/or trace id so consumers can specialize (e.g. fit-on-new-pane).
 */
export type LayoutChangeKind = 'split' | 'reused' | 'detached' | 'removed' | 'state';

/** Normalized front-end view of a `teammate-layout-changed` notification. */
export interface LayoutChange {
  readonly kind: LayoutChangeKind;
  /** Affected pane id (reused / detached / removed). */
  readonly paneId?: string;
  /** Split / activation trace id (split / removed). */
  readonly traceId?: string;
}

const KNOWN_KINDS: ReadonlySet<LayoutChangeKind> = new Set([
  'split',
  'reused',
  'detached',
  'removed',
  'state',
]);

/** Generic re-sync fallback used for legacy or unrecognized payloads. */
const STATE_FALLBACK: LayoutChange = { kind: 'state' };

function asString(value: unknown): string | undefined {
  return typeof value === 'string' ? value : undefined;
}

function isKnownKind(value: unknown): value is LayoutChangeKind {
  return typeof value === 'string' && KNOWN_KINDS.has(value as LayoutChangeKind);
}

/**
 * Parse a raw `teammate-layout-changed` payload into a typed `LayoutChange`.
 *
 * - `split` carries only `traceId`; a stray `pane_id` is intentionally dropped
 *   so fit-timing stays front-end-driven (see design §3 / 5b).
 * - `reused` / `detached` carry `paneId`.
 * - `removed` may carry `paneId` and/or `traceId`.
 * - Anything else (null, non-object, unknown kind) → generic `state` re-sync.
 */
export function parseLayoutChange(payload: unknown): LayoutChange {
  if (typeof payload !== 'object' || payload === null) {
    return STATE_FALLBACK;
  }

  const record = payload as Record<string, unknown>;
  if (!isKnownKind(record.kind)) {
    return STATE_FALLBACK;
  }

  const paneId = asString(record.pane_id);
  const traceId = asString(record.trace_id);

  switch (record.kind) {
    case 'split':
      return traceId ? { kind: 'split', traceId } : { kind: 'split' };
    case 'reused':
      return paneId ? { kind: 'reused', paneId } : { kind: 'reused' };
    case 'detached':
      return paneId ? { kind: 'detached', paneId } : { kind: 'detached' };
    case 'removed':
      return { kind: 'removed', ...(paneId ? { paneId } : {}), ...(traceId ? { traceId } : {}) };
    case 'state':
    default:
      return STATE_FALLBACK;
  }
}
