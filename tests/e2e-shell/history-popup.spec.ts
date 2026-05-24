/**
 * Regression guard for the shell-history popup ("时灵时不灵" round —
 * 2026-05-21). Locks the contract laid down by
 * `~/.claude/plans/shell-history-popup-bug-rosy-perlis.md`:
 *
 *   - ArrowUp at a fresh / empty prompt OPENS the popup. Pre-fix the
 *     popup would silently no-op when `terminalHistoryStore.fetch()`
 *     hadn't resolved yet (the `$effect` in TerminalHistoryPopup
 *     auto-closed on empty `filteredHistory`).
 *   - PTY output that happens to contain `\n` / `\r` (every prompt
 *     redraw, every command echo, any async background line) MUST NOT
 *     close the popup. Pre-fix `ptyBridge.ts` broadcast a window-wide
 *     `ridge:pty-newline` event on every byte chunk that included a
 *     newline; every RidgePane listened and dismissed its popup —
 *     so the popup got slammed shut even by its own pane's redraws,
 *     and definitely by any other pane's output.
 *   - Enter (real keystroke, dispatched through `onContainerKeyDown`
 *     → `dispatchBufferEvent` → `case 'clear'`) DOES close it. This
 *     is the only legitimate "user wants to submit the line" signal.
 *   - Escape closes it without writing to the PTY.
 *
 * Note on test driving: we send keys through `container.dispatchEvent`
 * rather than `browser.keys()` because the OS-level key path on
 * tauri-driver + WebView2 is flaky for non-character keys (the IME
 * helper textarea sometimes swallows ArrowUp before the container's
 * onkeydown fires). The dispatchEvent route lands directly on the
 * same listener the production code path uses, so the assertions
 * still cover the real `onContainerKeyDown` → `openHistoryOverlay`
 * → wasm-side `setHistoryOverlay` chain (§1.34, 2026-05-22 — popup
 * migrated from Svelte DOM to wasm canvas overlay).
 */
// @ts-nocheck
import { browser, expect } from '@wdio/globals';
import { waitForAppReady, firstPaneId } from './helpers';

/** Dispatch a KeyboardEvent at the pane container so it travels through
 *  the exact `onkeydown` handler the production code uses. */
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

/** Wait briefly for Svelte's microtask flush + render to commit. */
async function settle(ms = 60): Promise<void> {
  await new Promise((r) => setTimeout(r, ms));
}

/** True iff the wasm shell-history overlay is currently being painted
 *  for the given pane. Reads `__windE2E.historyOverlayState(paneId).open`,
 *  which mirrors the most-recent `manager.setHistoryOverlay` call
 *  (§1.34, 2026-05-22 — popup moved from Svelte DOM to wasm canvas
 *  overlay; the prior `.rg-history-popup` element no longer exists). */
async function popupVisible(paneId: string): Promise<boolean> {
  return (await browser.execute((id: string) => {
    const w = window as { __windE2E?: { historyOverlayState: (p: string) => { open: boolean } } };
    return w.__windE2E?.historyOverlayState(id).open ?? false;
  }, paneId)) as boolean;
}

describe('shell-history popup — show / hide reliability', () => {
  let paneId: string;

  before(async () => {
    await waitForAppReady();
    paneId = await firstPaneId();
    // Give the post-attach storm (initial PTY prompt echo, theme push,
    // history fetch) a beat to land. Without this the very first
    // ArrowUp can race the prompt redraw.
    await settle(500);
  });

  beforeEach(async () => {
    // Ensure no popup leaks between specs — Escape is a no-op when
    // closed, so this is safe to fire unconditionally.
    await pressKey(paneId, 'Escape');
    await settle();
  });

  it('ArrowUp at the fresh prompt opens the popup', async () => {
    await pressKey(paneId, 'ArrowUp');
    await settle();
    expect(await popupVisible(paneId)).toBe(true);
  });

  it('PTY output containing newlines does NOT close the popup', async () => {
    // Open the popup first.
    await pressKey(paneId, 'ArrowUp');
    await settle();
    expect(await popupVisible(paneId)).toBe(true);

    // Feed PTY bytes that simulate a noisy prompt redraw / background
    // log line. `feedPty` short-circuits the Rust producer and pushes
    // bytes straight into the kernel — exactly what `pty-output-*`
    // delivers in production, but synchronously testable.
    await browser.execute((id: string) => {
      const w = window as { __windE2E?: { feedPty: (p: string, d: string) => void } };
      w.__windE2E!.feedPty(id, 'background noise\r\nmore noise\r\n');
    }, paneId);
    // Give the kernel + render loop a couple frames to process. If the
    // pre-fix `ridge:pty-newline` regression sneaks back in, the popup
    // closes inside the same microtask the feed fires on.
    await settle(150);

    expect(await popupVisible(paneId)).toBe(true);
  });

  it('Escape closes the popup', async () => {
    await pressKey(paneId, 'ArrowUp');
    await settle();
    expect(await popupVisible(paneId)).toBe(true);

    await pressKey(paneId, 'Escape');
    await settle();
    expect(await popupVisible(paneId)).toBe(false);
  });

  it('Enter keystroke closes the popup (user-intent signal)', async () => {
    await pressKey(paneId, 'ArrowUp');
    await settle();
    expect(await popupVisible(paneId)).toBe(true);

    // Enter goes through the same `onContainerKeyDown` path. The popup
    // first consumes it via `historyPopupEl.handleKeyDown` (which
    // either onSelect / onClose depending on selection) and the close
    // path drops to dispatchBufferEvent's `case 'clear'`. Either path
    // ends with the popup closed.
    await pressKey(paneId, 'Enter');
    await settle();
    expect(await popupVisible(paneId)).toBe(false);
  });
});
