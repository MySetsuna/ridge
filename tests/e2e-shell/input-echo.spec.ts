/**
 * Regression guard for keyboard -> PTY -> shell output -> cursor state.
 *
 * Submit a complete, cross-shell command because line disciplines do not
 * promise incremental echo for unsubmitted input.
 */
// @ts-nocheck
import { browser, expect } from '@wdio/globals';
import { waitForAppReady, firstPaneId } from './helpers';

async function waitFor<T>(
  pred: () => Promise<T | null | false | 0 | ''>,
  timeoutMs = 4_000,
  intervalMs = 80,
): Promise<T> {
  const start = Date.now();
  while (Date.now() - start < timeoutMs) {
    const value = await pred();
    if (value) return value as T;
    await browser.pause(intervalMs);
  }
  throw new Error(`waitFor timed out after ${timeoutMs}ms`);
}

async function writePty(paneId: string, data: string): Promise<void> {
  const result = await browser.executeAsync((id, text, done) => {
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    (window as any).__windE2E.writePty(id, text).then(
      () => done({ ok: true }),
      (error: unknown) => done({ ok: false, detail: String(error) }),
    );
  }, paneId, data);
  if (!result?.ok) throw new Error(`writePty failed: ${result?.detail ?? 'unknown error'}`);
}

describe('input reaches the PTY and leaves consistent terminal state', () => {
  before(async () => {
    await waitForAppReady();
  });

  it('renders command output and keeps the cursor inside the grid', async () => {
    const paneId = await firstPaneId();
    const initialRows = await waitFor(async () => {
      const rows = await browser.execute((id) => {
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        return (window as any).__windE2E.visibleText(id) as string[];
      }, paneId);
      return rows.some((row) => row.trim().length > 0) ? rows : null;
    });
    if (process.platform === 'win32') {
      const prompts = initialRows.join('\n').match(/PS [A-Za-z]:\\[^>\r\n]*>/g) ?? [];
      expect(prompts).toHaveLength(1);
    }

    const suffix = Math.random().toString(36).slice(2, 8);
    const probe = `ridge-e2e-${suffix}`;
    await writePty(paneId, `echo ${probe}\r`);

    let found: string;
    try {
      found = await waitFor(async () => {
        const rows = await browser.execute((id) => {
          // eslint-disable-next-line @typescript-eslint/no-explicit-any
          return (window as any).__windE2E.visibleText(id) as string[];
        }, paneId);
        const joined = rows.join('\n');
        return joined.includes(probe) ? joined : null;
      }, 10_000, 100);
    } catch (error) {
      const snapshot = await browser.execute(async (id) => {
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        const e2e = (window as any).__windE2E;
        const pane = document.querySelector(`[data-rg-pane-id="${id}"]`);
        const workspaceId = pane?.closest('[data-rg-ws-pane-host]')?.getAttribute('data-rg-ws-pane-host');
        let tail: unknown = null;
        try {
          // eslint-disable-next-line @typescript-eslint/no-explicit-any
          tail = await (window as any).__TAURI_INTERNALS__.invoke('get_pane_scrollback_tail', {
            paneId: id,
            workspaceId,
            maxBytes: 4096,
          });
        } catch (tailError) {
          tail = { error: String(tailError) };
        }
        return {
          backend: e2e.backendName(id),
          cursor: e2e.kernelCursor(id),
          rows: e2e.rows(id),
          cols: e2e.cols(id),
          scrollback: e2e.scrollbackLen(id),
          visible: e2e.visibleText(id).filter((row: string) => row.trim()).slice(-8),
          tail,
        };
      }, paneId);
      throw new Error(`${String(error)}; terminal snapshot=${JSON.stringify(snapshot)}`);
    }
    expect(found).toContain(probe);

    const state = await browser.execute((id) => {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      const e2e = (window as any).__windE2E;
      return { cursor: e2e.kernelCursor(id), rows: e2e.rows(id), cols: e2e.cols(id) };
    }, paneId);
    expect(state.cursor.row).toBeGreaterThanOrEqual(0);
    expect(state.cursor.row).toBeLessThan(state.rows);
    expect(state.cursor.col).toBeGreaterThanOrEqual(0);
    expect(state.cursor.col).toBeLessThanOrEqual(state.cols);
  });
});
