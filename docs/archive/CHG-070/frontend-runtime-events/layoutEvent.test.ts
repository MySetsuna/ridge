/**
 * layoutEvent.test.ts — Tests for the `teammate-layout-changed` envelope parser.
 * Following TDD: tests written FIRST before implementation.
 *
 * The backend emits a discriminated envelope `{ kind, ...payload }`. The
 * front-end must normalize it into a typed `LayoutChange` and fall back to a
 * generic re-sync for any unrecognized / legacy shape so the handler never
 * breaks on an unexpected payload.
 */
import { describe, it, expect } from 'vitest';
import { parseLayoutChange } from './layoutEvent';
import golden from './layoutChange.golden.json';

describe('parseLayoutChange', () => {
  it('parses a split envelope and maps trace_id → traceId', () => {
    const change = parseLayoutChange({ kind: 'split', trace_id: 'trace-1' });
    expect(change).toEqual({ kind: 'split', traceId: 'trace-1' });
  });

  it('parses a reused envelope and maps pane_id → paneId', () => {
    const change = parseLayoutChange({ kind: 'reused', pane_id: 'pane-uuid' });
    expect(change).toEqual({ kind: 'reused', paneId: 'pane-uuid' });
  });

  it('parses a detached envelope', () => {
    const change = parseLayoutChange({ kind: 'detached', pane_id: 'pane-uuid' });
    expect(change).toEqual({ kind: 'detached', paneId: 'pane-uuid' });
  });

  it('parses a removed envelope carrying both pane_id and trace_id', () => {
    const change = parseLayoutChange({
      kind: 'removed',
      pane_id: 'pane-uuid',
      trace_id: 'trace-9',
    });
    expect(change).toEqual({ kind: 'removed', paneId: 'pane-uuid', traceId: 'trace-9' });
  });

  it('parses a removed envelope with no payload fields', () => {
    const change = parseLayoutChange({ kind: 'removed' });
    expect(change).toEqual({ kind: 'removed' });
  });

  it('parses a bare state envelope', () => {
    expect(parseLayoutChange({ kind: 'state' })).toEqual({ kind: 'state' });
  });

  it('falls back to state for a null payload (legacy `()` / `null` emit)', () => {
    expect(parseLayoutChange(null)).toEqual({ kind: 'state' });
    expect(parseLayoutChange(undefined)).toEqual({ kind: 'state' });
  });

  it('falls back to state for an unknown kind', () => {
    expect(parseLayoutChange({ kind: 'totally-new-kind' })).toEqual({ kind: 'state' });
  });

  it('falls back to state for a non-object payload', () => {
    expect(parseLayoutChange('split')).toEqual({ kind: 'state' });
    expect(parseLayoutChange(42)).toEqual({ kind: 'state' });
  });

  it('ignores non-string payload fields rather than propagating bad types', () => {
    const change = parseLayoutChange({ kind: 'reused', pane_id: 123 });
    expect(change).toEqual({ kind: 'reused' });
  });

  it('omits the pane id for a split (front-end self-determines fit timing)', () => {
    // Split intentionally carries only trace_id; pane_id is not part of the
    // contract so 5b cannot regress into backend-driven fit timing.
    const change = parseLayoutChange({ kind: 'split', trace_id: 't', pane_id: 'p' });
    expect(change.paneId).toBeUndefined();
    expect(change).toEqual({ kind: 'split', traceId: 't' });
  });
});

describe('parseLayoutChange — shared golden envelopes (M1 cross-end contract)', () => {
  // Same fixture the Rust serializer round-trips (layout_event.rs
  // golden_envelopes_round_trip). This is the RUNNABLE half of the cross-end
  // contract check, since the Rust lib test harness can't launch on every host.
  const entries = Object.entries(golden).filter(([key]) => !key.startsWith('_')) as Array<
    [string, { kind: string; pane_id?: string; trace_id?: string }]
  >;

  it('covers every variant', () => {
    expect(entries.length).toBeGreaterThanOrEqual(6);
  });

  for (const [name, wire] of entries) {
    it(`parses golden case "${name}" to the right kind + camelCase fields`, () => {
      const change = parseLayoutChange(wire);
      expect(change.kind).toBe(wire.kind);

      // pane_id → paneId for reused/detached/removed; split never carries one.
      if (wire.kind === 'reused' || wire.kind === 'detached' || wire.kind === 'removed') {
        expect(change.paneId).toBe(wire.pane_id);
      } else {
        expect(change.paneId).toBeUndefined();
      }

      // trace_id → traceId for split/removed.
      if (wire.trace_id !== undefined) {
        expect(change.traceId).toBe(wire.trace_id);
      } else {
        expect(change.traceId).toBeUndefined();
      }
    });
  }
});
