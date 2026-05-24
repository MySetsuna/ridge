/**
 * Regression guard for the kernel-side shell-history popup gate
 * (§1.33, 2026-05-22; updated §1.35 for sticky=0).
 *
 * The bug this locks down: ArrowUp / ArrowDown inside a TUI such as
 * Claude Code OR vim OR htop was opening the host shell-history popup
 * instead of being passed through to the TUI as cursor-arrow bytes.
 * Root cause was that the JS-side `tuiGate` honoured the sticky
 * window only while the cursor was hidden — so any TUI that flashed
 * the cursor visible between menu frames raced the popup open before
 * the next TUI signal landed.
 *
 * The fix lives in `packages/ridge-term/src/lib.rs::should_allow_shell_history`,
 * exposed through `manager.shouldAllowShellHistory(paneId)`:
 *
 *   1. ANY of `app_cursor_keys` (DECCKM `?1`), `alt_screen` (`?1049`
 *      / `?47`), mouse reporting, the inline-TUI heuristic, OR a
 *      hidden cursor (`?25l`) blocks immediately.
 *   2. §1.35: SHELL_HISTORY_STICKY_MS = 0 — the gate opens immediately
 *      once every live signal clears. No extra buffer after TUI exit.
 *
 * Spec strategy: drive the kernel into each known-TUI mode via raw
 * CSI bytes (the same way Claude Code / vim / htop would), then
 * dispatch ArrowUp at the pane container and assert the popup
 * remains hidden. Kernel mode flags are inspected through
 * `__windE2E.kernelDecState` so we can prove the bytes landed where
 * intended before claiming the gate held.
 */
// @ts-nocheck
import { browser, expect } from '@wdio/globals';
import { waitForAppReady, firstPaneId } from './helpers';

/** Dispatch a KeyboardEvent at the pane container so it travels through
 *  the same `onkeydown` handler the production code uses. Mirrors the
 *  helper in `history-popup.spec.ts`. */
async function pressKey(paneId: string, key: string): Promise<void> {
  await browser.execute((id: string, k: string) => {
    const el = document.querySelector(
      `[data-rg-pane-id="${id}"]`,
    ) as HTMLElement | null;
    if (!el) throw new Error(`pane container not found for ${id}`);
    el.dispatchEvent(
      new KeyboardEvent('keydown', {
        key: k,
        bubbles: true,
        cancelable: true,
      }),
    );
  }, paneId, key);
}

async function settle(ms = 60): Promise<void> {
  await new Promise((r) => setTimeout(r, ms));
}

async function popupVisible(paneId: string): Promise<boolean> {
  return (await browser.execute((id: string) => {
    const w = window as { __windE2E?: { historyOverlayState: (p: string) => { open: boolean } } };
    return w.__windE2E?.historyOverlayState(id).open ?? false;
  }, paneId)) as boolean;
}

/** Force the kernel back into a "fresh shell prompt" state between
 *  specs: clear alt screen, drop DECCKM, drop mouse reporting, show
 *  the cursor, exit bracketed paste. §1.35: sticky=0, so the gate
 *  opens immediately — no extra sleep needed. The small settle here
 *  is only for DOM/paint timing, not for sticky decay. */
async function resetToShellPrompt(paneId: string): Promise<void> {
  await browser.execute((id: string) => {
    const w = window as { __windE2E?: { feedPty: (p: string, d: string) => void } };
    w.__windE2E!.feedPty(
      id,
      // Reset every signal the gate reads. With sticky=0 the gate
      // re-opens immediately after all signals clear.
      '\x1b[?1049l\x1b[?1l\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?25h',
    );
  }, paneId);
  await settle(60);
}

describe('shell-history popup — TUI gate (kernel-side, §1.33)', () => {
  let paneId: string;

  before(async () => {
    await waitForAppReady();
    paneId = await firstPaneId();
    // Post-attach storm settle, matching history-popup.spec.ts so the
    // first ArrowUp doesn't race the prompt redraw.
    await settle(500);
  });

  beforeEach(async () => {
    await pressKey(paneId, 'Escape');
    await settle();
  });

  it('alt-screen (?1049h) blocks ArrowUp popup; gate re-opens immediately on exit', async () => {
    // Sanity precondition.
    await resetToShellPrompt(paneId);
    await pressKey(paneId, 'ArrowUp');
    await settle();
    expect(await popupVisible(paneId)).toBe(true);
    await pressKey(paneId, 'Escape');
    await settle();

    // Enter alt screen — vim / less / htop convention.
    await browser.execute((id: string) => {
      const w = window as { __windE2E?: { feedPty: (p: string, d: string) => void } };
      w.__windE2E!.feedPty(id, '\x1b[?1049h');
    }, paneId);
    const dec = await browser.execute((id: string) => {
      const w = window as { __windE2E?: { kernelDecState: (p: string) => unknown } };
      return w.__windE2E!.kernelDecState(id);
    }, paneId);
    expect((dec as { isAltScreen: boolean }).isAltScreen).toBe(true);

    await pressKey(paneId, 'ArrowUp');
    await settle();
    expect(await popupVisible(paneId)).toBe(false);

    // Leave alt screen — §1.35: with sticky=0 the gate re-opens
    // immediately. No extra buffer window.
    await browser.execute((id: string) => {
      const w = window as { __windE2E?: { feedPty: (p: string, d: string) => void } };
      w.__windE2E!.feedPty(id, '\x1b[?1049l');
    }, paneId);
    await pressKey(paneId, 'ArrowUp');
    await settle();
    expect(await popupVisible(paneId)).toBe(true);
  });

  it('DECCKM (?1h) blocks ArrowUp popup', async () => {
    await resetToShellPrompt(paneId);
    await browser.execute((id: string) => {
      const w = window as { __windE2E?: { feedPty: (p: string, d: string) => void } };
      w.__windE2E!.feedPty(id, '\x1b[?1h');
    }, paneId);
    const dec = await browser.execute((id: string) => {
      const w = window as { __windE2E?: { kernelDecState: (p: string) => unknown } };
      return w.__windE2E!.kernelDecState(id);
    }, paneId);
    expect((dec as { isAppCursorKeys: boolean }).isAppCursorKeys).toBe(true);

    await pressKey(paneId, 'ArrowUp');
    await settle();
    expect(await popupVisible(paneId)).toBe(false);
  });

  it('mouse reporting (?1000h) blocks ArrowUp popup', async () => {
    await resetToShellPrompt(paneId);
    await browser.execute((id: string) => {
      const w = window as { __windE2E?: { feedPty: (p: string, d: string) => void } };
      w.__windE2E!.feedPty(id, '\x1b[?1000h');
    }, paneId);
    const dec = await browser.execute((id: string) => {
      const w = window as { __windE2E?: { kernelDecState: (p: string) => unknown } };
      return w.__windE2E!.kernelDecState(id);
    }, paneId);
    expect((dec as { mouseReportingModes: number }).mouseReportingModes).toBeGreaterThan(0);

    await pressKey(paneId, 'ArrowUp');
    await settle();
    expect(await popupVisible(paneId)).toBe(false);
  });

  it('hidden cursor (?25l) blocks ArrowUp popup — the Claude-Code regression case', async () => {
    // This is the exact symptom that motivated §1.33: a TUI that
    // hides the cursor while rendering and otherwise leaves no other
    // signal asserted (alt-screen off, DECCKM off, mouse off, inline-
    // TUI heuristic stale).
    await resetToShellPrompt(paneId);
    await browser.execute((id: string) => {
      const w = window as { __windE2E?: { feedPty: (p: string, d: string) => void } };
      w.__windE2E!.feedPty(id, '\x1b[?25l');
    }, paneId);
    const dec = await browser.execute((id: string) => {
      const w = window as { __windE2E?: { kernelDecState: (p: string) => unknown } };
      return w.__windE2E!.kernelDecState(id);
    }, paneId);
    expect((dec as { isCursorVisible: boolean }).isCursorVisible).toBe(false);

    await pressKey(paneId, 'ArrowUp');
    await settle();
    expect(await popupVisible(paneId)).toBe(false);
  });

  it('gate opens immediately once cursor is visible again (sticky=0)', async () => {
    // §1.35: SHELL_HISTORY_STICKY_MS = 0, so the gate re-opens
    // as soon as every live signal clears. A TUI that flickered
    // cursor hidden→visible must NOT gate the popup afterwards.
    await resetToShellPrompt(paneId);
    await browser.execute((id: string) => {
      const w = window as { __windE2E?: { feedPty: (p: string, d: string) => void } };
      w.__windE2E!.feedPty(id, '\x1b[?25l\x1b[?25h');
    }, paneId);
    const dec = await browser.execute((id: string) => {
      const w = window as { __windE2E?: { kernelDecState: (p: string) => unknown } };
      return w.__windE2E!.kernelDecState(id);
    }, paneId);
    expect((dec as { isCursorVisible: boolean }).isCursorVisible).toBe(true);

    await pressKey(paneId, 'ArrowUp');
    await settle();
    expect(await popupVisible(paneId)).toBe(true);
  });
});
