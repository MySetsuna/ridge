import { describe, expect, it } from 'vitest';
import { shouldRequestPaneList } from './cdp-pty-state.mjs';

describe('CDP PTY pane polling state', () => {
  it('polls while no pane exists and creation has not started', () => {
    expect(shouldRequestPaneList({ pane: null }, { createRequested: false })).toBe(true);
  });

  it('stops polling after pane subscription or create request', () => {
    expect(shouldRequestPaneList({ pane: 'pane-1' }, { createRequested: false })).toBe(false);
    expect(shouldRequestPaneList({ pane: null }, { createRequested: true })).toBe(false);
  });
});
