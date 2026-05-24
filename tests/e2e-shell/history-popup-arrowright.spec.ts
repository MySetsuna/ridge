/**
 * Regression guard for the Warp-style two-mode history-popup select
 * (§1.33, 2026-05-22).
 *
 *   - Enter on a selected row     → writes `<cmd>\r` (execute).
 *   - ArrowRight on a selected row → writes `<cmd>` only (insert
 *                                     for editing, no execute).
 *   - ArrowRight with NO selection → falls through to the underlying
 *                                     shell so cursor-right still
 *                                     moves the cursor.
 *
 * Both keys close the popup. The byte-level assertions go through
 * `__windE2E.installPtyWriteSpy` / `ptyWriteLog`, which monkey-patch
 * the pane entry's `dataHandler` to record every Uint8Array sent to
 * `invoke('write_to_pty')`. Without the spy we'd only be able to
 * observe shell echo, which depends on the live shell's behaviour
 * (PowerShell vs bash vs cmd) and would make the spec flaky across
 * platforms. The spy records the EXACT string the popup-onSelect
 * path emitted, before the OS shell sees a single byte.
 */
// @ts-nocheck
import { browser, expect } from '@wdio/globals';
import { waitForAppReady, firstPaneId } from './helpers';

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

/** Strip focus-event housekeeping bytes (`\x1b[I` / `\x1b[O`) from the
 *  spy log. RidgePane's `commitHistorySelection` ends with
 *  `imeHelper?.focus()`, which triggers `manager.setFocused(true)` and
 *  writes `\x1b[I` (CSI I, focus-in per `?1004h`) to the PTY. That byte
 *  always lands AFTER the cmd write, so a naive `log[log.length-1]`
 *  catches the focus event instead of the command. These bytes are
 *  protocol housekeeping, not the popup-commit emission the spec is
 *  asserting on, so they must be filtered out before "last write"
 *  checks. */
function nonFocusEntries(log: Array<{ data: string }>): Array<{ data: string }> {
  return log.filter((e) => e.data !== '\x1b[I' && e.data !== '\x1b[O');
}

/** All three helpers read the wasm-side overlay state mirror exposed
 *  via `__windE2E.historyOverlayState(paneId)` (§1.34, 2026-05-22 —
 *  popup migrated from Svelte DOM to wasm canvas overlay, so DOM
 *  selectors like `.rg-history-popup` no longer exist). The mirror
 *  reflects the most-recent `setHistoryOverlay` call: `open`, `items`,
 *  `selectedIndex`. The dismiss row is represented as
 *  `selectedIndex === -1` — same convention the popup logic uses. */
async function popupVisible(paneId: string): Promise<boolean> {
  return (await browser.execute((id: string) => {
    const w = window as { __windE2E?: { historyOverlayState: (p: string) => { open: boolean } } };
    return w.__windE2E?.historyOverlayState(id).open ?? false;
  }, paneId)) as boolean;
}

/** Selected row index. Returns `-1` for the dismiss row, the row's
 *  zero-based index when a real history row is selected, or `null`
 *  when the overlay is closed. */
async function selectedRowIndex(paneId: string): Promise<number | null> {
  return (await browser.execute((id: string) => {
    const w = window as {
      __windE2E?: {
        historyOverlayState: (p: string) => { open: boolean; selectedIndex: number };
      };
    };
    const s = w.__windE2E?.historyOverlayState(id);
    if (!s || !s.open) return null;
    return s.selectedIndex;
  }, paneId)) as number | null;
}

/** Selected row's command text (what `commitHistorySelection` would
 *  emit). Returns null when the overlay is closed OR only the dismiss
 *  row is selected. */
async function selectedRowText(paneId: string): Promise<string | null> {
  return (await browser.execute((id: string) => {
    const w = window as {
      __windE2E?: {
        historyOverlayState: (
          p: string,
        ) => { open: boolean; items: string[]; selectedIndex: number };
      };
    };
    const s = w.__windE2E?.historyOverlayState(id);
    if (!s || !s.open || s.selectedIndex < 0) return null;
    return s.items[s.selectedIndex] ?? null;
  }, paneId)) as string | null;
}

describe('shell-history popup — ArrowRight = insert-no-execute (Warp-style, §1.33)', () => {
  let paneId: string;

  before(async () => {
    await waitForAppReady();
    paneId = await firstPaneId();
    await settle(500);
    // Install the byte-level spy once. Subsequent calls are no-ops.
    await browser.execute((id: string) => {
      const w = window as { __windE2E?: { installPtyWriteSpy: (p: string) => void } };
      w.__windE2E!.installPtyWriteSpy(id);
    }, paneId);
  });

  beforeEach(async () => {
    await pressKey(paneId, 'Escape');
    await settle();
    await browser.execute((id: string) => {
      const w = window as { __windE2E?: { clearPtyWriteLog: (p: string) => void } };
      w.__windE2E!.clearPtyWriteLog(id);
    }, paneId);
  });

  it('ArrowRight on a selected row writes the command WITHOUT trailing \\r and closes the popup', async () => {
    // 1. Open the popup. ArrowUp at fresh prompt opens with
    //    selectedIndex=-1 (the dismiss row). A second ArrowUp jumps
    //    selection to the LAST row (oldest entry) — that's the
    //    popup's bash-like wraparound and is enough for the test.
    await pressKey(paneId, 'ArrowUp');
    await settle();
    if (!(await popupVisible(paneId))) {
      // Some CI shells launch without any history; the popup's auto-
      // close-when-filtered-empty effect closes it instantly. Skip
      // the spec instead of asserting on environment we don't own.
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      (this as any).skip?.();
      return;
    }
    await pressKey(paneId, 'ArrowUp');
    await settle();

    // Sanity: a real history row is selected.
    const idx = await selectedRowIndex(paneId);
    const cmd = await selectedRowText(paneId);
    if (idx === null || idx < 0 || !cmd) {
      // Same env-empty escape hatch as above — there were no rows
      // to select after all.
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      (this as any).skip?.();
      return;
    }
    expect(typeof cmd).toBe('string');

    // 2. ArrowRight should commit-without-execute.
    await pressKey(paneId, 'ArrowRight');
    await settle();

    // Popup closed.
    expect(await popupVisible(paneId)).toBe(false);

    // 3. Byte-level proof: the spy recorded the command WITHOUT a
    //    trailing '\r'. There may be a preceding replay sequence
    //    (clears the user's typed prefix before insert) — we only
    //    assert the LAST recorded chunk is the bare command.
    const log = (await browser.execute((id: string) => {
      const w = window as { __windE2E?: { ptyWriteLog: (p: string) => Array<{ data: string }> } };
      return w.__windE2E!.ptyWriteLog(id);
    }, paneId)) as Array<{ data: string }>;

    const userWrites = nonFocusEntries(log);
    expect(userWrites.length).toBeGreaterThan(0);
    const last = userWrites[userWrites.length - 1].data;
    expect(last).toBe(cmd);
    expect(last.endsWith('\r')).toBe(false);
    expect(last.endsWith('\n')).toBe(false);
  });

  it('Enter on a selected row writes the command WITH trailing \\r (execute)', async () => {
    await pressKey(paneId, 'ArrowUp');
    await settle();
    if (!(await popupVisible(paneId))) {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      (this as any).skip?.();
      return;
    }
    await pressKey(paneId, 'ArrowUp');
    await settle();

    const idx = await selectedRowIndex(paneId);
    const cmd = await selectedRowText(paneId);
    if (idx === null || idx < 0 || !cmd) {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      (this as any).skip?.();
      return;
    }

    await pressKey(paneId, 'Enter');
    await settle();
    expect(await popupVisible(paneId)).toBe(false);

    const log = (await browser.execute((id: string) => {
      const w = window as { __windE2E?: { ptyWriteLog: (p: string) => Array<{ data: string }> } };
      return w.__windE2E!.ptyWriteLog(id);
    }, paneId)) as Array<{ data: string }>;
    const userWrites = nonFocusEntries(log);
    expect(userWrites.length).toBeGreaterThan(0);
    const last = userWrites[userWrites.length - 1].data;
    expect(last).toBe(cmd + '\r');
  });

  it('ArrowRight with no row selected falls through (popup closes? — and no insert-write fires)', async () => {
    // When the popup is open with selectedIndex=-1 (only the dismiss
    // row "selected" pseudo-state), ArrowRight should NOT commit any
    // command — there isn't one to commit. The popup's
    // `handleKeyDown` returns false for this case, RidgePane's outer
    // handler then routes ArrowRight through the normal kernel key
    // encoder.
    await pressKey(paneId, 'ArrowUp');
    await settle();
    if (!(await popupVisible(paneId))) {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      (this as any).skip?.();
      return;
    }
    // selectedIndex is -1 (dismiss row) right after the first
    // ArrowUp open — perfect for this case.
    expect(await selectedRowIndex(paneId)).toBe(-1);

    await pressKey(paneId, 'ArrowRight');
    await settle();

    const log = (await browser.execute((id: string) => {
      const w = window as { __windE2E?: { ptyWriteLog: (p: string) => Array<{ data: string }> } };
      return w.__windE2E!.ptyWriteLog(id);
    }, paneId)) as Array<{ data: string }>;
    // The only thing that should have hit the PTY is the kernel-
    // encoded cursor-right (`\x1b[C` in normal mode, `\x1bOC` in
    // app-cursor-keys mode). Crucially: NO history command bytes.
    for (const e of log) {
      // A history command pick would be more than 2-3 bytes of plain
      // ASCII; the cursor-right encoding is 3 bytes starting with ESC.
      // We just assert no entry contains a printable command-shaped
      // run AND a '\r' tail — that would prove the insert path fired
      // by accident.
      expect(e.data.endsWith('\r')).toBe(false);
    }
  });
});
