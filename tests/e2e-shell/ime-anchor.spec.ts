/**
 * Regression guard for IME preedit/textarea alignment — §P5.IME
 * (2026-05-21).
 *
 * Two invariants this spec locks down:
 *
 *   1. Single anchor source. The DOM textarea pixel rect AND the wasm
 *      preedit overlay cell are derived from the SAME
 *      `manager.inputAnchorResolved(paneId)` result. They can never
 *      disagree about which cell the user is typing into. The probe
 *      `__windE2E.lastPreeditCall(paneId)` returns the (row, col) JS
 *      sent to wasm; `__windE2E.inputAnchorResolved(paneId)` returns
 *      the resolver's canonical (row, col) at the same moment.
 *
 *   2. Same-frame follow in shell mode, lock in TUI mode. With no
 *      alt-screen / inline-TUI active, the resolver re-runs on every
 *      `compositionupdate` and the preedit cell follows genuine
 *      cursor movement (line wrap, async prompt re-emit) without the
 *      one-RAF lag the locked path used to have. The §1.28 lock is
 *      retained for alt-screen / inline-TUI panes — Ink-style spinner
 *      walks would otherwise drag the preedit (the "IME 输入域到处乱跑"
 *      regression).
 *
 * We drive the composition lifecycle from JS via
 * `dispatchEvent(new CompositionEvent(...))` on the live `.rg-ime-helper`
 * textarea so the spec is decoupled from any specific OS IME, then
 * sample the JS-side mirrors. Visible-glyph alignment is implicitly
 * verified because both sides come from the same probe.
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
    const v = await pred();
    if (v) return v as T;
    await browser.pause(intervalMs);
  }
  throw new Error(`waitFor timed out after ${timeoutMs}ms`);
}

/** Dispatch a synthetic `compositionstart` / `update` / `end` against
 *  the pane's IME helper textarea. Mirrors what the OS IME would do
 *  inline-mode without requiring an actual CJK keyboard layout on the
 *  CI box. The textarea is the SAME element the OS IME targets, so
 *  every code path Ridge has under composition runs identically. */
async function fireComposition(
  paneId: string,
  phase: 'start' | 'update' | 'end',
  data: string,
): Promise<void> {
  await browser.execute(
    (id, p, d) => {
      const root = document.querySelector(
        `[data-rg-pane-id="${id}"]`,
      ) as HTMLElement | null;
      if (!root) throw new Error(`pane ${id} not in DOM`);
      const ta = root.querySelector('textarea.rg-ime-helper') as
        | HTMLTextAreaElement
        | null;
      if (!ta) throw new Error('rg-ime-helper textarea missing');
      // Focus first so the composition events target the textarea
      // (matches OS IME behaviour — IMEs only fire compositionstart
      // against a focused input).
      ta.focus();
      const type = `composition${p}`;
      const ev = new CompositionEvent(type, { data: d, bubbles: true });
      ta.dispatchEvent(ev);
    },
    paneId,
    phase,
    data,
  );
}

describe('IME anchor — preedit overlay and textarea share one source', () => {
  before(async () => {
    await waitForAppReady();
  });

  it('lastPreeditCall is null before any composition fires', async () => {
    const paneId = await firstPaneId();
    const last = await browser.execute((id) => {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      return (window as any).__windE2E.lastPreeditCall(id);
    }, paneId);
    expect(last).toBeNull();
  });

  it('compositionstart + update lands preedit at the resolver cell', async () => {
    const paneId = await firstPaneId();
    // Plant some ASCII so the cursor isn't at (0, 0) — that would
    // false-positive any "anchor is zero" code path.
    await browser.execute((id) => {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      void (window as any).__windE2E.writePty(id, 'echo ime-anchor ');
    }, paneId);
    // Wait for the echo so the kernel cursor reflects the typed text.
    await waitFor(async () => {
      const text = await browser.execute((id) => {
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        return (window as any).__windE2E.visibleText(id).join('\n');
      }, paneId);
      return text.includes('ime-anchor') ? text : null;
    });

    await fireComposition(paneId, 'start', '');
    await fireComposition(paneId, 'update', 'ni');

    const probe = await browser.execute((id) => {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      const e2e = (window as any).__windE2E;
      return {
        anchor: e2e.inputAnchorResolved(id),
        preedit: e2e.lastPreeditCall(id),
      };
    }, paneId);

    // §1: textarea anchor cell == overlay cell. Identical resolver,
    // identical (row, col).
    expect(probe.anchor).not.toBeNull();
    expect(probe.preedit).not.toBeNull();
    expect(probe.preedit.text).toBe('ni');
    expect(probe.preedit.row).toBe(probe.anchor.row);
    expect(probe.preedit.col).toBe(probe.anchor.col);

    await fireComposition(paneId, 'end', '');
  });

  it('clearPreedit nulls the mirror after compositionend', async () => {
    const paneId = await firstPaneId();
    await fireComposition(paneId, 'start', '');
    await fireComposition(paneId, 'update', 'a');
    await fireComposition(paneId, 'end', '');
    const after = await browser.execute((id) => {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      return (window as any).__windE2E.lastPreeditCall(id);
    }, paneId);
    expect(after).toBeNull();
  });

  it('preedit cell follows cursor movement in shell mode (no RAF lag)', async () => {
    const paneId = await firstPaneId();
    // 1. Stake out an anchor by composing a single char.
    await fireComposition(paneId, 'start', '');
    await fireComposition(paneId, 'update', 'n');
    const before = await browser.execute((id) => {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      return (window as any).__windE2E.lastPreeditCall(id);
    }, paneId);
    expect(before).not.toBeNull();

    // 2. Push the kernel cursor to a new row via direct feed (CSI CUP
    //    to row=before.row+3, col=1 in 1-based VT coords). This is the
    //    same kind of cursor movement a wrap or async prompt redraw
    //    emits. `feedPty` lands the bytes in the kernel WITHOUT going
    //    through the PTY round-trip so the spec doesn't depend on
    //    shell timing.
    await browser.execute((id, targetRowOneBased) => {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      (window as any).__windE2E.feedPty(id, `\x1b[${targetRowOneBased};1H`);
    }, paneId, before.row + 3);

    // 3. Drive a compositionupdate; in shell mode the anchor MUST
    //    re-resolve same-frame and follow to the new cell.
    await fireComposition(paneId, 'update', 'ni');

    const after = await browser.execute((id) => {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      const e2e = (window as any).__windE2E;
      return {
        decState: e2e.kernelDecState(id),
        anchor: e2e.inputAnchorResolved(id),
        preedit: e2e.lastPreeditCall(id),
      };
    }, paneId);

    // Skip the assertion if the shell happened to flip to a TUI mode
    // between the two updates (very unlikely in this default-shell
    // bring-up but the test must not false-fail in that case).
    const inTui =
      after.decState && (after.decState.isAltScreen || after.decState.isInlineTuiMode);
    if (!inTui) {
      expect(after.anchor).not.toBeNull();
      expect(after.preedit).not.toBeNull();
      // Same source: preedit (row, col) MUST equal the resolver.
      expect(after.preedit.row).toBe(after.anchor.row);
      expect(after.preedit.col).toBe(after.anchor.col);
      // Followed the cursor: anchor row advanced (we pushed CUP forward).
      expect(after.anchor.row).toBeGreaterThan(before.row);
    }

    await fireComposition(paneId, 'end', '');
  });
});
