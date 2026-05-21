<!--
  RidgePane.svelte — terminal pane backed by ridge-term wasm.

  The sole terminal renderer for Ridge. Owns a paneId + workspaceId pair;
  provides a div container; TerminalManager creates a canvas inside and
  wires PTY bytes:
      backend pty-output-{ws}-{p}  ──►  manager.feed(paneId, bytes)
      manager.onData (key encoder) ──►  invoke('write_to_pty', ...)
      ResizeObserver (in manager)  ──►  invoke('resize_pane', ...)

  Known gap: parkTerminal/restoreTerminal across split — splitting a pane
  currently tears down the wasm kernel and re-spawns the shell. PTY
  backend survives; in-buffer state does not.
-->
<script lang="ts">
import { onMount, onDestroy } from 'svelte';
import { invoke, isTauri } from '@tauri-apps/api/core';
import { readText, writeText } from '@tauri-apps/plugin-clipboard-manager';
import { activePaneId, setPaneCwd, paneOscTitleStore, terminalTitles, splitPane, closePane } from '$lib/stores/paneTree';
import type { KernelEvent } from '$lib/terminal/manager';
import { ensurePtyBridge, setPaneDeltaMode } from '$lib/terminal/ptyBridge';
import { pushTerminalThemeNow } from '$lib/terminal/themeBridge';
import { settingsStore } from '$lib/stores/settings';
import { showContextMenu } from '$lib/stores/contextMenu';
import { get } from 'svelte/store';
import TerminalHistoryPopup from './TerminalHistoryPopup.svelte';
import { terminalHistoryStore } from '$lib/stores/terminalHistory';
import { TerminalManager } from '$lib/terminal/manager';
import { isTuiActive } from '$lib/terminal/tuiGate';
import { computePopupPosition } from './historyPopupPosition';
import {
	deriveBufferEvent,
	updateInputBuffer,
	computeReplaySequence,
	EMPTY_INPUT_BUFFER,
	type InputBufferState,
} from './inputBufferTracker';

interface Props {
	paneId: string;
	workspaceId: string;
}

let { paneId, workspaceId }: Props = $props();

let container: HTMLElement;
let alive = true;
// `$state` so the focus + padding `$effect`s re-run when `attached` flips
// from false → true inside the async onMount IIFE. Without reactivity the
// effects only ran once (at mount with attached=false), leaving the new
// pane's wasm renderer at its default `focused=true` → both panes blink
// after a split until the next activePaneId change.
let attached = $state(false);

// P3.9 (2026-05-20) — parserBackend live switch with a 200ms fade so
// the user never sees a partial-mirror flash. Initial sync happens in
// ensurePtyBridge; this effect only fires on subsequent changes.
let prevParserBackend = $state<'wasm' | 'rust' | null>(null);
let backendSwitching = $state(false);

// PTY listener subscriptions used to live here as ptyUnlisten /
// ptyClosedUnlisten. Both moved to `$lib/terminal/ptyBridge` (TASKS §5.1)
// so listener lifetime tracks the wasm kernel lifetime (manager.attach →
// manager.detach) rather than this Svelte component's mount cycle —
// otherwise PTY bytes emitted during the unmount window of a split /
// reparent would be silently dropped.

const manager = TerminalManager.instance();

// History popup state
let historyPopupOpen = $state(false);
// §1.32 (2026-05-20) Wave C: state is now `{ text, cursorCol }` so
// ArrowLeft / Home / Delete / mid-line edits preserve the buffer
// instead of clearing it. See `inputBufferTracker.ts` for the rules.
let currentInputBuffer = $state<InputBufferState>(EMPTY_INPUT_BUFFER);
let historyPopupPosition = $state({ x: 0, y: 0, inputH: 20 });
let historyPopupEl: { handleKeyDown: (e: KeyboardEvent) => boolean } | undefined = $state(undefined);

// §1.32 (2026-05-20): popup placement helper + live resize tracking.
// Three previously-deferred lifecycle bugs collapse into this:
//   - Bug #2: when `manager.inputAnchorPixelPosition` returns null
//     (cell metrics not ready, pane unknown, alt-screen race) the old
//     code fell back to `{ x: 0, y: 0, cellH: 20 }` and stranded the
//     popup at the viewport corner. `computePopupPosition` now
//     returns null and we flip `historyPopupOpen` back to false.
//   - Bug #13: ArrowUp → Esc → ArrowUp must re-anchor on each open;
//     every popup-open path calls this function so the anchor is
//     always fresh.
//   - Bug #1: while the popup is open, a pane resize re-runs this
//     function via the ResizeObserver $effect below so the popup
//     tracks the new cell origin.
function repositionPopup(): boolean {
	if (!container) return false;
	const pos = computePopupPosition(
		manager.inputAnchorPixelPosition(paneId),
		container.getBoundingClientRect(),
	);
	if (!pos) {
		historyPopupOpen = false;
		return false;
	}
	historyPopupPosition = pos;
	return true;
}

// Observe the pane container while the popup is open. The effect's
// cleanup auto-disconnects the observer when the popup closes or the
// component unmounts; no leak even if multiple popup-open/close
// cycles happen during the component's lifetime.
$effect(() => {
	if (!historyPopupOpen || !container) return;
	const ro = new ResizeObserver(() => { repositionPopup(); });
	ro.observe(container);
	return () => ro.disconnect();
});

// §1.32 (2026-05-20) Wave B: paste + key dispatch helpers route every
// path that mutates the shell line through the unit-tested
// `inputBufferTracker` state machine so `currentInputBuffer` stays in
// sync with the real shell line for all common operations
// (Ctrl+U / Ctrl+W / Ctrl+K kills, paste, printable chars, backspace).
function pasteIntoPane(text: string): void {
	// §1.32 Wave F: paste is "input started" too — mark before writing
	// so the snapshot has a valid baseline. markInputStart is idempotent.
	manager.markInputStart(paneId);
	manager.paste(paneId, text);
	currentInputBuffer = updateInputBuffer(currentInputBuffer, { type: 'paste', text });
}

function dispatchBufferEvent(e: KeyboardEvent): void {
	const ev = deriveBufferEvent(e);
	if (!ev) return;
	// §1.32 Wave F: keep the keystroke mirror updated for the popup's
	// live filter, AND drive the snapshot's input-start lifecycle so
	// `readShellInputSnapshot` can read the actual shell line at
	// history-pick time.
	currentInputBuffer = updateInputBuffer(currentInputBuffer, ev);
	switch (ev.type) {
		case 'char':
		case 'paste':
		case 'tab':
			// First content-producing event after a fresh prompt:
			// remember WHERE on the grid the input begins.
			manager.markInputStart(paneId);
			break;
		case 'clear':
		case 'killLine':
			// Enter / Ctrl+U: shell line ends or fully clears; the
			// next input is a fresh start.
			manager.clearInputStart(paneId);
			break;
		// Other events (backspace / cursor moves / killWord / killToEol)
		// don't change the input's start position — leave the marker.
	}
}
// Search state
let termSearchOpen = $state(false);
let searchQuery = $state('');
let searchCaseSensitive = $state(false);
let searchInfo = $state<{ count: number; activeIndex: number }>({ count: 0, activeIndex: -1 });
let searchInputEl: HTMLInputElement | undefined = $state(undefined);

// Bell visual flash — toggles a CSS class for ~120ms when the kernel emits
// a Bell event. No audio (would need Audio API + permission); a visual
// flash is enough to draw the eye when bg processes complete (e.g. `ls
// && tput bel`). 120ms is short enough not to be annoying.
let bellFlash = $state(false);
let bellFlashTimer: ReturnType<typeof setTimeout> | null = null;
function triggerBellFlash() {
	bellFlash = true;
	if (bellFlashTimer !== null) clearTimeout(bellFlashTimer);
	bellFlashTimer = setTimeout(() => { bellFlash = false; bellFlashTimer = null; }, 120);
}

// IME (CJK input method) — browsers require a focused <input>/<textarea>
// for the IME candidate window to appear. We add an invisible helper
// textarea pinned to the bottom-left of the pane (out of the way of
// scrolling output), redirect focus to it on pane click, and forward
// keystrokes through the same `onContainerKeyDown` handler as before.
// Composition events: compositionstart sets the guard, compositionend
// takes the composed string (e.data) and writes it to the PTY via
// `manager.write`. While composing, normal key handling is suppressed
// so partial composition keys don't reach the shell.
let imeHelper: HTMLTextAreaElement | undefined = $state(undefined);
let isComposing = $state(false);
/** Latest preedit string handed to the wasm renderer for this
 *  composition session. Sent verbatim to `manager.setPreedit`; the
 *  renderer paints it on top of the cell grid at
 *  `composingAnchor.{row,col}` as the last pass each frame, leaving
 *  kernel cells untouched. Reset on compositionstart / compositionend. */
let preeditSentToPty = '';
// §P5.IME (2026-05-21): preeditStartCell removed — the cell coordinates
// for the wasm preedit overlay now live on `composingAnchor.{row,col}`,
// the SAME object that drives the textarea pixel rect. Read each on
// `compositionupdate`; never let them disagree.

// §1.28 (2026-05-19) + §P5.IME (2026-05-21): anchor snapshot used by
// BOTH the textarea DOM rect AND the wasm preedit overlay, so they can
// never drift apart by even a cell (single source via
// `manager.inputAnchorResolved`).
//
// Lock policy:
//   - Alt-screen / inline-TUI: snapshot frozen at compositionstart and
//     held for the whole session. Re-resolving mid-update lets Ink-style
//     log-update spinner walks (claude code, opencode) drag the preedit
//     across the pane — the original "IME 输入域到处乱跑" symptom.
//   - Plain shell (cmd / PowerShell / bash / zsh / fish): re-resolve on
//     every compositionupdate. The resolver chain in shell mode is
//     stable (imeAnchor reflects post-keystroke cursor; no spinner to
//     drag it) so following genuine input movement — line wrap, async
//     prompt re-emit — keeps preedit + textarea pinned to the visible
//     input cell.
type ImeAnchor = {
	row: number;
	col: number;
	x: number;
	y: number;
	cellW: number;
	cellH: number;
	fontSizePx: number;
};
let composingAnchor: ImeAnchor | null = null;

// Sticky inline-TUI gate. The kernel's inline-TUI heuristic
// (grid.rs::INLINE_TUI_DECAY_MS) decays after 2 s without abs/redraw
// CSI activity so that returning to a normal shell prompt immediately
// re-enables host shortcuts. The trade-off bites in TUIs that draw
// once and then idle waiting for input — claude code's `/theme`
// menu is exactly that. After a wheel scroll the TUI consumes no
// fresh CSI, the 2 s window expires, and the next ArrowUp suddenly
// pops the shell-history overlay instead of navigating the menu.
//
// Sticky: once we see any live TUI signal, treat the pane as TUI
// for up to TUI_STICKY_MS — but only while the cursor stays hidden.
// Shell prompts always run with the cursor visible, so the moment
// the user is actually back at a prompt the sticky bit can no
// longer apply and host shortcuts re-enable as before.
const TUI_STICKY_MS = 60_000;
let lastTuiActiveTs = 0;
// §1.31 (2026-05-19): delegate the decision logic to the pure helper in
// `$lib/terminal/tuiGate` so it can be unit-tested as a truth table.
// We retain the stateful `lastTuiActiveTs` refresh here because the
// gate function is intentionally stateless. The new DECCKM branch
// (`isAppCursorKeys`) lives inside `isTuiActive` and dominates every
// other signal — once an app sets DECCKM the shell-history popup is
// unreachable, which is exactly what the user asked for.
function isTuiSticky(): boolean {
	const live = manager.isAltScreen(paneId)
		|| manager.isInlineTuiActive(paneId)
		|| manager.isMouseReporting(paneId);
	const now = performance.now();
	if (live) lastTuiActiveTs = now;
	return isTuiActive({
		isAltScreen: manager.isAltScreen(paneId),
		isInlineTuiActive: manager.isInlineTuiActive(paneId),
		isMouseReporting: manager.isMouseReporting(paneId),
		isAppCursorKeys: manager.isAppCursorKeys(paneId),
		cursorVisible: manager.isCursorVisible(paneId),
		lastTuiActiveTs,
		now,
		stickyMs: TUI_STICKY_MS,
	});
}

// Host-priority shortcuts that should fire BEFORE TUI key forwarding.
// Convention shared by gnome-terminal / kitty / iTerm2 / wezterm /
// Windows Terminal: a small fixed set of modifier+Shift or platform-
// native combinations is always handled by the host so users can
// paste / copy / fullscreen even inside a TUI that captures everything
// else (claude code, opencode, etc.). Plain Ctrl+V / Ctrl+C still flow
// through to the TUI as bytes — those are TUI semantics (SYN / SIGINT).
// Returns true when the host claimed the event.
function handleHostPriorityShortcut(e: KeyboardEvent): boolean {
	const isMac = /Mac|iPhone|iPod|iPad/.test(navigator.platform || '');
	const isWin = /Win/i.test(navigator.platform || '');
	const mod = e.ctrlKey || (isMac && e.metaKey);

	// Ctrl+Shift+V / Cmd+Shift+V — host paste, always wins on every
	// platform. Conservative POSIX users can reach the TUI's SYN byte
	// ("literal next" in readline) via Ctrl+Q instead.
	if (mod && e.shiftKey && !e.altKey && (e.key === 'v' || e.key === 'V')) {
		void readText().then((text) => { if (text) pasteIntoPane(text); });
		e.preventDefault();
		return true;
	}

	// macOS Cmd+V (no Shift) — host paste, matches every other macOS app.
	if (isMac && e.metaKey && !e.ctrlKey && !e.shiftKey && !e.altKey
			&& (e.key === 'v' || e.key === 'V')) {
		void readText().then((text) => { if (text) pasteIntoPane(text); });
		e.preventDefault();
		return true;
	}

	// Windows plain Ctrl+V — host paste, matches the default Windows
	// Terminal / PowerShell / ConHost behaviour where users expect
	// Ctrl+V to insert clipboard contents into stdin (the host pastes
	// before the byte ever reaches the running process). POSIX
	// platforms still send SYN to the TUI on plain Ctrl+V; that's the
	// xterm / gnome-terminal / iTerm2 convention.
	if (isWin && e.ctrlKey && !e.shiftKey && !e.metaKey && !e.altKey
			&& (e.key === 'v' || e.key === 'V')) {
		void readText().then((text) => { if (text) pasteIntoPane(text); });
		e.preventDefault();
		return true;
	}

	// Ctrl+Shift+C — host copy when a selection exists. Falls through
	// otherwise so a TUI that wants Ctrl+Shift+C as its own hotkey can
	// still receive it.
	if (mod && e.shiftKey && !e.altKey && (e.key === 'c' || e.key === 'C')) {
		const sel = manager.getSelectionText(paneId);
		if (sel) {
			void writeText(sel);
			e.preventDefault();
			return true;
		}
	}

	// F11 fullscreen / Ctrl+, settings — OS / app-shell concerns, let
	// the browser handle them regardless of TUI state. Returning true
	// without preventDefault lets the event bubble up to the document.
	if (e.key === 'F11' && !mod && !e.altKey && !e.shiftKey) {
		return true;
	}
	if (mod && !e.shiftKey && !e.altKey && e.key === ',') {
		return true;
	}

	return false;
}

// Reverse-scrollback bridge state (TASKS §2.1).
//
// `oldestSeq` is the backend `seq` (monotonic byte counter) of the first
// byte the kernel's scrollback currently holds. Initial value comes from
// the `get_pane_scrollback_tail` response at mount; each successful
// `get_pane_scrollback_before` fetch updates it to the older chunk's
// `start_seq`.
//
// `atOldest` flips true when the backend tells us we've drained its
// retention window; further fetches are no-ops.
//
// `pendingScrollbackFetch` guards against piling up overlapping fetches
// when the user mashes Shift+PageUp.
let oldestSeq = 0;
let atOldest = false;
let pendingScrollbackFetch = false;

// §1.23 (2026-05-15): Auto-hide scrollbar logic.
// `isScrolling` toggles when user interacts via wheel or keyboard.
// `scrollHideTimer` resets on each action; 1.5s delay before hiding.
let isScrolling = $state(false);
let scrollHideTimer: ReturnType<typeof setTimeout> | null = null;
function showScrollbarTemporarily() {
	isScrolling = true;
	if (scrollHideTimer) clearTimeout(scrollHideTimer);
	scrollHideTimer = setTimeout(() => { isScrolling = false; }, 1500);
}

async function fetchOlderScrollback(): Promise<void> {
	if (!alive || !attached) return;
	if (atOldest || pendingScrollbackFetch) return;
	pendingScrollbackFetch = true;
	try {
		const chunk = await invoke<{
			bytes: string;
			start_seq: number;
			at_oldest: boolean;
		}>('get_pane_scrollback_before', {
			paneId,
			beforeSeq: oldestSeq,
			maxBytes: 128 * 1024,
		});
		if (!alive) return;
		if (chunk.bytes) {
			manager.prependScrollback(paneId, chunk.bytes);
			oldestSeq = chunk.start_seq;
		}
		if (chunk.at_oldest) {
			atOldest = true;
		}
	} catch (err) {
		// Backend may not support before-seq paging on older builds; treat
		// as "no more history" rather than spamming console for every key.
		atOldest = true;
		if (import.meta.env?.DEV) {
			console.debug('[ridge-pane] get_pane_scrollback_before failed', err);
		}
	} finally {
		pendingScrollbackFetch = false;
	}
}

/** Trigger a backend fetch when the viewport is near the top of the
 *  in-kernel scrollback. Called from Shift+PageUp / wheel-up paths. */
function maybePrefetchOlder(): void {
	if (atOldest || pendingScrollbackFetch) return;
	const { offset, total } = manager.scrollState(paneId);
	// Fire when the user is within one viewport of the top — gives the
	// fetch time to land before they actually hit the boundary, so heavy
	// scrolling doesn't stutter visibly.
	const rows = manager.rows(paneId);
	if (total - offset <= rows) {
		void fetchOlderScrollback();
	}
}

function repositionImeHelper() {
	if (!imeHelper) return;
	// §1.28 + §P5.IME: during active composition use the snapshot
	// `composingAnchor` (already maintained by the lock-or-follow rule
	// in `onCompositionUpdate` — see below). Outside composition pull
	// directly from the unified resolver so the textarea, the history
	// popup, and the wasm overlay all see the same cell.
	const pos: { x: number; y: number; cellW: number; cellH: number } | null =
		isComposing && composingAnchor
			? composingAnchor
			: (manager.inputAnchorResolved?.(paneId) ?? manager.inputAnchorPixelPosition(paneId));
	if (!pos) return;
	// Anchor the (invisible) IME textarea exactly on the cursor cell.
	// The OS IME's candidate-popup will dock below this rect — same
	// place a native input field surfaces its candidates, which is the
	// familiar interaction model for every CJK / IBus / IME user. No
	// font / baseline / preedit rendering on our side; the OS handles
	// it. Standard pattern across xterm.js, VS Code terminal, wezterm-web.
	imeHelper.style.left = `${pos.x}px`;
	imeHelper.style.top = `${pos.y}px`;
	imeHelper.style.bottom = 'auto';
	// Cell dimensions drive the textarea's own `width / height` (set
	// in CSS via `var(--rg-ime-cell-w)`), so the OS IME has a real
	// cell-sized rect to anchor its candidate window against.
	imeHelper.style.setProperty('--rg-ime-cell-w', `${pos.cellW}px`);
	imeHelper.style.setProperty('--rg-ime-cell-h', `${pos.cellH}px`);
}

// §1.27 (2026-05-07): RIDGE_DIAG-gated IME composition trace. The dim/IME
// residue investigation needs concrete evidence about when composition
// starts, what data lands on `compositionend`, and what the cursor cell
// state looks like around it — so we can tell IME-overlay rendering bugs
// (textarea shrink leaving stale canvas pixels) apart from grid-state
// bugs (DIM-attr cells leaking from prior writes). Gate is sampled once
// per pane mount so `localStorage.RIDGE_DIAG='1'; location.reload()`
// flips it without runtime overhead in normal use.
const dimDiagEnabled = (() => {
	if (typeof window === 'undefined') return false;
	try {
		return window.localStorage?.RIDGE_DIAG === '1';
	} catch {
		return false;
	}
})();

function diagLogIme(event: string, extra?: Record<string, unknown>) {
	if (!dimDiagEnabled) return;
	console.log('[ime]', event, {
		paneId,
		isComposing,
		imeValue: imeHelper?.value,
		...extra,
	});
}

function onCompositionStart() {
	isComposing = true;
	preeditSentToPty = '';
	// §P5.IME: single-source anchor. Same `(row, col)` powers the
	// wasm preedit overlay AND the textarea pixel rect — they cannot
	// disagree about where the user's caret is.
	composingAnchor = manager.inputAnchorResolved?.(paneId) ?? null;
	repositionImeHelper();
	diagLogIme('start');
}


	function onCompositionUpdate(e: CompositionEvent) {
		// Renderer-side preedit overlay: the wasm renderer paints the
		// preedit text on top of the cell grid as a final pass each
		// frame. Cells are NOT modified, so a TUI redrawing its frame
		// mid-composition can't corrupt the preedit, AND the preedit
		// can't corrupt the TUI's cells. Works identically in shell
		// mode and alt-screen TUIs (vim, less, claude code, opencode).
		const next = e.data ?? '';

		// §P5.IME (2026-05-21): re-resolve the anchor INSIDE shell mode
		// so the preedit + textarea follow genuine input movement
		// (line wrap, async prompt re-emit). In alt-screen / inline-TUI
		// keep §1.28 lock — the resolver can hop to spinner / status-bar
		// rows mid-frame, which dragged the preedit before the lock
		// existed. Re-resolve happens SAME-FRAME (no RAF) so the OS IME
		// candidate popup tracks the cursor without a one-frame lag.
		if (composingAnchor) {
			const inTui = manager.isAltScreen(paneId) || manager.isInlineTuiActive(paneId);
			if (!inTui) {
				const fresh = manager.inputAnchorResolved?.(paneId);
				if (
					fresh &&
					(fresh.row !== composingAnchor.row || fresh.col !== composingAnchor.col)
				) {
					composingAnchor = fresh;
					repositionImeHelper();
				}
			}
		}

		if (composingAnchor) {
			manager.setPreedit?.(paneId, next, composingAnchor.row, composingAnchor.col);
			preeditSentToPty = next;
		}
		if (imeHelper && composingAnchor && e.data) {
			const charCount = e.data.length;
			imeHelper.style.width = `${(charCount + 1) * composingAnchor.cellW}px`;
		}
		diagLogIme('update', { dataLen: e.data?.length ?? 0, data: e.data });
	}

	function onImeHelperFocus() {
		// Anchor on focus too, in case the user clicked into the pane and
		// expects the next IME composition to appear near the current cursor.
		repositionImeHelper();
	}

	function onImeHelperPaste(e: ClipboardEvent) {
		const text = e.clipboardData?.getData('text');
		if (text) {
			pasteIntoPane(text);
			e.preventDefault();
		}
	}
	function onCompositionEnd(e: CompositionEvent) {
		isComposing = false;
		composingAnchor = null;
		const data = e.data ?? '';
		// Clear the renderer-side preedit overlay (kernel cells were
		// never touched, no erase needed). Then ship the committed
		// string through the normal PTY write path; the shell / TUI
		// echoes it back at its OWN tracked cursor — which lands in
		// the right cell because we didn't disturb anything.
		manager.clearPreedit?.(paneId);
		preeditSentToPty = '';
		if (data.length > 0) {
			manager.write(paneId, data);
		}
		if (imeHelper) {
			imeHelper.value = '';
			imeHelper.style.width = 'auto';
		}

		// §1.27 fix: force a full-frame redraw so any canvas pixels that
	// were under the now-shrunk `.is-composing` overlay are repainted
	// from kernel cell state. Without this, Canvas2D's per-row hash diff
	// can skip rows whose CELLS are unchanged but whose PIXELS were
	// smeared by the overlay, leaving preedit-shaped residue. WebGPU
	// already redraws every row per tick, so this is effectively a wake
	// there. One frame is cheap; we always-redraw rather than gate on
	// "did the user actually commit" — a cancelled composition can leak
	// just as easily as a committed one.
	manager.forceFullRedraw(paneId);
	// §1.27-tail (2026-05-07): commit echo lag — `manager.write` posts
	// the committed Chinese chars to the PTY immediately, but the
	// shell's echo round-trips the OS scheduler + PTY readline + kernel
	// feed and can land 30–100 ms later. The first `forceFullRedraw`
	// above paints a frame BEFORE the echo lands, so the user briefly
	// sees the prompt without their committed text where the textarea
	// just collapsed. A 120 ms follow-up redraw catches the echoed
	// cells and refreshes the canvas. `alive` guards against the
	// component unmounting (split / close) before the timer fires.
	setTimeout(() => {
		if (!alive) return;
		manager.forceFullRedraw(paneId);
	}, 120);

	// §1.27 diag: log the committed string. The companion cells_at()
	// call to inspect cell state around the cursor lives in the
	// devtools console — see `docs/term-rebuild/REPRO_dim_residue.md`
	// for the recipe. Adding a kernel-access helper to TerminalManager
	// solely for this diagnostic is heavier than the inspector
	// deserves at this stage; calling cellsAt() directly via the
	// kernel handle from devtools is sufficient evidence to drive
	// the §1.27 fix.
	diagLogIme('end', { committed: data });
}

function refreshSearch() {
	if (!alive || !attached) return;
	manager.searchSetQuery(paneId, searchQuery, searchCaseSensitive);
	searchInfo = manager.searchInfo(paneId);
}

function openSearchBar() {
	termSearchOpen = true;
	// Defer focus to next tick so the input element exists.
	queueMicrotask(() => searchInputEl?.focus());
}

function closeSearchBar() {
	termSearchOpen = false;
	searchQuery = '';
	manager.searchClear(paneId);
	searchInfo = { count: 0, activeIndex: -1 };
	// Return focus to the IME helper textarea so keyboard input flows
	// back to the terminal (container has tabindex=-1 now).
	imeHelper?.focus();
}

function onSearchInputKey(e: KeyboardEvent) {
	if (e.key === 'Escape') {
		e.preventDefault();
		closeSearchBar();
		return;
	}
	if (e.key === 'Enter') {
		e.preventDefault();
		if (e.shiftKey) {
			manager.searchPrev(paneId);
		} else {
			manager.searchNext(paneId);
		}
		searchInfo = manager.searchInfo(paneId);
		return;
	}
	// Other keys fall through; refresh runs on input event.
}

// UUID v4 shape check. Backend's `parse_pane_id` requires a UUID;
// split node ids ("split-N" from engine_node_to_layout) are not valid
// pane targets. RidgePane should NEVER be mounted with a split-id —
// SplitContainer renders Pane only on `node.type === 'leaf'` branches —
// but a tracked-down report (TASKS 2026-05-03) shows split-1 reaching
// resize_pane somehow. Guard here surfaces the offending paneId at
// mount time so the next reproduction has full context, and prevents
// the IPC spam (every drag → backend rejection → console.error).
const PANE_ID_RE = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
function isValidPaneId(id: string): boolean {
	return PANE_ID_RE.test(id);
}

// Handler closures hoisted to component scope so every RidgePane
// instance owns its own `alive` / `triggerBellFlash` capture.
//
// Why this matters: TerminalManager preserves dataHandler /
// resizeHandler / eventHandler across park (kernel survives the
// unmount window). When a SplitContainer split / reparent forces a
// RidgePane re-mount, the OLD component's onDestroy sets alive=false;
// if those handlers were registered with closures that captured the
// OLD `alive`, every keystroke through the new pane goes through
// `manager.handleKeyDown → entry.dataHandler(bytes) → if (!alive) return;`
// and is silently dropped — even though a fresh component is sitting
// at the keyboard. By hoisting and re-registering on every mount
// (both first attach and unpark branches), each instance always gets
// the live `alive` flag of the currently-attached component.
// (TASKS §1.17.)
function onPtyData(bytes: Uint8Array) {
	if (!alive) return;
	// Tauri's write_to_pty currently expects a JS string. We send
	// the raw bytes through TextDecoder. NOTE: this is lossy for
	// non-UTF-8 byte sequences — the encoder never produces those
	// (key sequences are all ASCII), but a future binary tunneling
	// path may want a base64 alternative.
	const s = new TextDecoder().decode(bytes);
	void invoke('write_to_pty', { paneId, data: s }).catch((err) => {
		console.error('write_to_pty', err);
	});
}

function onPtyResize(
	rows: number,
	cols: number,
	isAlt: boolean,
	isInlineTui: boolean,
): Promise<void> {
	// Defensive: should be impossible after the onMount UUID guard
	// below, but leaving the cheap check in catches any future
	// path that smuggles a bad id past attach.
	if (!isValidPaneId(paneId)) {
		if (import.meta.env?.DEV) {
			console.warn('[ridge-pane] resize skipped — non-UUID paneId:', paneId);
		}
		return Promise.resolve();
	}
	// §1.24 / §A.3: `isAlt` and `isInlineTui` both let the backend skip
	// the ConPTY resize-silence window so the foreground app's
	// SIGWINCH-driven redraw isn't dropped. See manager.ts and
	// src-tauri/src/commands/terminal.rs::resize_pane_inner.
	//
	// We return the invoke promise so `manager.ts::fitPane` can `await`
	// it on plain primary — the kernel grid only narrows AFTER the
	// backend ConPTY resize completes, eliminating the in-flight byte
	// race that used to cause border characters to wrap on shrink.
	return invoke('resize_pane', { paneId, rows, cols, isAlt, isInlineTui }).then(
		() => undefined,
		(err) => {
			console.error('resize_pane', err);
		},
	);
}

function onPtyNewline() { historyPopupOpen = false; }

function onKernelEvent(ev: KernelEvent) {
	switch (ev.type) {
		case 'CwdChanged':
			setPaneCwd(workspaceId, paneId, ev.value);
			break;
		case 'TitleChanged':
		case 'IconNameChanged':
			// Mirror Pane.svelte's policy: OSC title takes priority
			// over the polled foreground process name. Write both
			// stores so SplitContainer's `$terminalTitles[paneId]`
			// shows the new title immediately.
			//
			// Identity-preserving early return (§1.21): shells re-emit
			// OSC 0/1/2 on every prompt redraw — without the equality
			// guard, every Enter creates a new store object with the
			// same content, and Explorer's `$effect` re-runs (calling
			// `syncWithPaneCwds` → new column refs → FileTree re-eval).
			// Same pattern paneCwdStore::setPaneCwd uses (paneTree.ts).
			paneOscTitleStore.update((s) => s[paneId] === ev.value ? s : ({ ...s, [paneId]: ev.value }));
			terminalTitles.update((m) => m[paneId] === ev.value ? m : ({ ...m, [paneId]: ev.value }));
			break;
		case 'Bell':
			triggerBellFlash();
			break;
	}
}

onMount(() => {
	if (!isTauri()) {
		console.warn('[ridge-pane] requires Tauri');
		return;
	}
    
    // 获取所有可用 shell 的历史记录（Rust 端自动合并多个历史文件）
    terminalHistoryStore.fetch();

    // 终端换行时自动关闭历史弹层
    window.addEventListener('ridge:pty-newline', onPtyNewline);

	if (!isValidPaneId(paneId)) {
		console.error(
			'[ridge-pane] mounted with non-UUID paneId:',
			paneId,
			'workspaceId:',
			workspaceId,
			'— refusing to attach. This indicates SplitContainer rendered Pane on a non-leaf node or a malformed leaf. Please grab a stack trace and the paneTreeStore snapshot.',
		);
		return;
	}

	void (async () => {
		await manager.ready();
		if (!alive) return;

		// Branch on parking state (TASKS §5.1).
		//
		// If the manager already holds a parked entry for this paneId,
		// this is a re-mount across a split / reparent. The kernel is
		// alive with the prior viewport / scrollback / selection /
		// search state; the PTY bridge has been forwarding bytes into
		// it during the unmount window. Just bind a fresh canvas and
		// rejoin the render loop.
		if (manager.isParked(paneId)) {
			await manager.unpark(paneId, container);
			if (!alive) return;
			attached = true;
			window.dispatchEvent(new CustomEvent('ridge:pane-attached'));
			manager.setFocused(paneId, get(activePaneId) === paneId);
			manager.setPadding(paneId, get(settingsStore).terminalPaddingPx);
			// Re-register handlers so this fresh component owns the
			// closures (the previous instance's `alive` is now false
			// and would silently drop every keystroke). Manager's
			// onData/onResize/onEvent replace any prior callback for
			// the same paneId.
			manager.onData(paneId, onPtyData);
			manager.onResize(paneId, onPtyResize);
			manager.onEvent(paneId, onKernelEvent);
			return;
		}

		// First attach: create kernel + canvas, register handlers,
		// start backend PTY, replay scrollback, activate stream.
		// §A.8 — pass workspaceId so the manager binds this pane to
		// the correct per-workspace SurfaceHost / canvas.
		await manager.attach(paneId, container, workspaceId);
		if (!alive) return;
		attached = true;
		// Force-push the current CSS-derived theme onto the fresh kernel.
		// `setupTerminalThemeBridge` runs once at app boot and only
		// re-pushes when the settings store changes, so the very first
		// pane to attach AFTER bootup races the bridge's initial RAF —
		// if attach wins, `opts.theme` is null and the kernel keeps its
		// compile-time defaults until the next settings tick (which may
		// never come). This force-push closes that window: every attach
		// sees the live `--rg-*` CSS vars on documentElement and applies
		// them synchronously.
		pushTerminalThemeNow();
		window.dispatchEvent(new CustomEvent('ridge:pane-attached'));

		// Sync focus state immediately so a freshly-split pane doesn't draw
		// a phantom cursor between attach and the next $effect tick. The
		// renderer defaults to `focused=true`; for a non-active pane we must
		// explicitly tell it false BEFORE the rAF loop paints its first frame.
		// Apply the user's preferred padding for the same reason.
		manager.setFocused(paneId, get(activePaneId) === paneId);
		manager.setPadding(paneId, get(settingsStore).terminalPaddingPx);

		// 1) Outbound: keyboard → PTY.
		// 2) Resize → PTY: backend syncs SIGWINCH.
		// 2b) Typed kernel events: cwd, title, hyperlinks, bell.
		// All three handlers live at script scope and capture this
		// component's `alive` / `triggerBellFlash`; see the comment
		// block above each function for details.
		manager.onData(paneId, onPtyData);
		manager.onResize(paneId, onPtyResize);
		manager.onEvent(paneId, onKernelEvent);

		// 3) PTY backend lifecycle
		try {
			await invoke('create_pane', {
				paneId,
				shell: get(settingsStore).defaultShell || null,
			});
		} catch (e) {
			console.error('create_pane failed', e);
			return;
		}
		if (!alive) return;

		// 4) PTY output + closed-event listeners. ensurePtyBridge owns
		// these subscriptions for the kernel's lifetime — they survive
		// split / reparent unmount so PTY bytes during the unmount
		// window are fed into the parked kernel rather than dropped.
		// `pane-pty-closed` rebuild (create_pane + activate_pane_pty)
		// also lives in the bridge.
		await ensurePtyBridge(paneId, workspaceId);
		if (!alive) return;

		// 5) Replay scrollback. Seed `oldestSeq` / `atOldest` from the
		// tail chunk so subsequent `Shift+PageUp` past the kernel buffer
		// boundary can page further into the backend's 4 MiB store.
		try {
			const chunk = await invoke<{
				bytes: string;
				start_seq: number;
				at_oldest: boolean;
			}>('get_pane_scrollback_tail', { paneId, maxBytes: 256 * 1024 });
			if (alive && chunk.bytes) {
				manager.feed(paneId, chunk.bytes);
			}
			if (alive) {
				oldestSeq = chunk.start_seq;
				atOldest = chunk.at_oldest;
			}
		} catch {
			if (alive) atOldest = true;
		}

		// 6) Activate PTY now that listener is wired and history replayed
		try {
			await invoke('activate_pane_pty', {
				workspaceId,
				paneId,
				rows: manager.rows(paneId),
				cols: manager.cols(paneId),
			});
		} catch (e) {
			const msg = String(e);
			if (!msg.includes('Pane not found')) {
				console.error('activate_pane_pty failed', e);
			}
		}

		// 7) Sync the backend delta_mode to the user's current Settings
		// preference. MUST come after `activate_pane_pty` — `create_pane`
		// only registers a `PendingSpawn`, the live pane handle that
		// `set_pane_delta_mode` looks up in `ws.terminals` doesn't land
		// until `activate_pane_pty` runs. Pre-fix, this call was inside
		// `ensurePtyBridge` and fired before activation → "pane not found"
		// warning on every cold boot. Fire-and-forget here is safe; if it
		// fails the user just stays on whatever the backend's default
		// delta_mode is.
		if (alive) {
			const backend = get(settingsStore).parserBackend;
			void setPaneDeltaMode(paneId, backend === 'rust');
		}

		// `pane-pty-closed` rebuild now lives in ptyBridge and persists
		// across this component's mount cycle, so we don't subscribe
		// here. See ptyBridge.ts.
	})();
});

// P3.9 — parserBackend live switch. Initial value is synced by the
// onMount IIFE right after `activate_pane_pty` returns (the pane
// handle must be in `ws.terminals` for `set_pane_delta_mode` to
// resolve, otherwise it errors with "pane not found"). Subsequent
// runs of this effect detect a real change in
// `$settingsStore.parserBackend` and flip backends on the fly, with
// a 200ms opacity mask to hide the brief mirror transition.
$effect(() => {
	const backend = $settingsStore.parserBackend;
	if (!attached) {
		prevParserBackend = backend;
		return;
	}
	if (prevParserBackend === backend) return;
	prevParserBackend = backend;
	backendSwitching = true;
	(async () => {
		try {
			await setPaneDeltaMode(paneId, backend === 'rust');
		} finally {
			// Hold the mask for ~200ms so the first frame after the
			// switch arrives before the fade-out. This is the
			// architecture-level mitigation for R4 — the user sees a
			// smooth transition rather than a partial mirror flash.
			setTimeout(() => {
				if (alive) backendSwitching = false;
			}, 200);
		}
	})();
});

// §1.23 (2026-05-05) → P1.3 (2026-05-19): the side scrollbar's thumb
// used to be kept in sync by a 4Hz `setInterval(refreshScrollState, 250)`
// per attached pane — pure polling so that async PTY-driven scrollback
// growth was reflected even when no keystroke / wheel handler fired.
// Multiplied across panes it was a measurable chunk of the idle CPU
// floor. The manager now diffs `kernel.scrollOffset` / `scrollbackLen`
// on the RAF tick and notifies subscribers only on change (and fires
// an immediate baseline emit on subscription), so we get strictly
// better latency (16 ms worst-case vs 250 ms) at zero idle cost.
$effect(() => {
	if (!attached) return;
	return manager.onScrollState(paneId, refreshScrollState);
});

onDestroy(() => {
	alive = false;
	// 取消终端换行监听
	window.removeEventListener('ridge:pty-newline', onPtyNewline);
	// Without this, a Bell received within 120ms of pane close leaves a
	// dangling setTimeout that writes `bellFlash` on a torn-down component.
	if (bellFlashTimer !== null) {
		clearTimeout(bellFlashTimer);
		bellFlashTimer = null;
	}
	// P1.3 (2026-05-19): no scrollbar poll timer to tear down — the
	// $effect that wired `manager.onScrollState` handles unsubscription
	// via its cleanup return, and `manager.park` clears the handler
	// slot on the pane entry below.
	// Park instead of detach (TASKS §5.1). We don't know in onDestroy
	// whether this is a transient unmount (split / reparent) or a real
	// close — parking is cheap to reverse via unpark, and the PTY bridge
	// keeps feeding bytes into the kernel during the unmount window.
	// Real cleanup (manager.detach + teardownPtyBridge + title-store
	// purge) happens in `paneTree.closePane` after the backend close_pane
	// IPC succeeds.
	if (attached) {
		manager.park(paneId);
	}
	// NOTE: title stores are intentionally NOT cleared here. The kernel
	// is still alive, the bridge is still parsing OSC events into them,
	// and clearing on transient unmount would cause title flicker.
	// closePane handles the real removal.
});

// Active-pane tracking — drives the data attribute (used by CSS targeting)
// AND tells the wasm renderer whether to draw the cursor. Only the focused
// pane should blink; unfocused panes hide the cursor entirely. The renderer's
// `setFocused` is idempotent, so emitting on every effect run is safe.
$effect(() => {
	if (!container) return;
	const isActive = $activePaneId === paneId;
	container.dataset.rgPaneActive = String(isActive);
	if (attached) {
		manager.setFocused(paneId, isActive);
	}
});

// Apply the user's preferred terminal padding. The setter is clamped + a
// no-op when the value is unchanged, so re-running on every settings tick
// is cheap. `attached` gate prevents a transient style write before the
// canvas exists.
$effect(() => {
	if (!attached) return;
	const px = $settingsStore.terminalPaddingPx;
	manager.setPadding(paneId, px);
});

	function onContainerKeyDown(e: KeyboardEvent) {
		if (!alive || !attached) return;

		// 1. 先处理弹层和 IME（这些是高优先级的应用逻辑，不是系统快捷键）
		if (historyPopupOpen && historyPopupEl?.handleKeyDown(e)) {
			e.preventDefault();
			return;
		}
		if (isComposing || e.isComposing) return;

		// 2. Host-priority shortcuts (paste / copy-with-selection /
		// fullscreen / settings). These bypass TUI key forwarding so
		// users can always paste into claude / opencode / vim — the
		// TUI never sees Ctrl+Shift+V because the host intercepts
		// first. See handleHostPriorityShortcut for the full table.
		if (handleHostPriorityShortcut(e)) return;

		// 3. TUI 模式下，优先透传给终端，TUI 未消费则继续执行
		// 注意: TUI 启用鼠标模式 (isMouseReporting) 也意味着键盘应优先给 TUI
		// 使用 isTuiSticky() 而非直接 OR，避免 claude /theme 这类静态
		// 菜单在 inline-TUI 2s decay 过期后误判出 TUI 模式。
		const isTui = isTuiSticky();
		if (isTui) {
			if (manager.handleKeyDown(paneId, e)) {
				e.preventDefault();
				return;
			}
		}

		// 3. ArrowUp/ArrowDown → 唤起历史弹窗，非 TUI 模式下才处理。
		// §1.31 (2026-05-19): belt-and-suspenders DECCKM check. `isTui`
		// already factors in `isAppCursorKeys` via tuiGate, but a separate
		// direct gate here guarantees that if `isTuiSticky` is ever
		// refactored away from DECCKM the popup STILL can't hijack arrow
		// keys from a program that explicitly owns them. Cheap query;
		// worth the defense-in-depth.
		if (!isTui && !manager.isAppCursorKeys(paneId)
			&& !historyPopupOpen && (e.key === 'ArrowUp' || e.key === 'ArrowDown')
			&& !e.ctrlKey && !e.metaKey && !e.altKey && !e.shiftKey) {
			e.preventDefault();
			historyPopupOpen = true;
			// §1.32: `repositionPopup()` flips `historyPopupOpen` back to
			// false on null anchor (Bug #2) — return early in that case
			// instead of leaving a popup-open state with no valid position.
			if (!repositionPopup()) return;
			return;
		}

		const isMac = /Mac|iPhone|iPod|iPad/.test(navigator.platform || '');
		const mod = e.ctrlKey || (isMac && e.metaKey);

		// 4. Non-TUI-only 快捷键。Host-priority 集合（粘贴 / 复制选中 /
		// F11 / Ctrl+,）已在 step 2 提前处理；这里只剩下与 TUI 行为冲突、
		// 必须避让 TUI 的 host 快捷键（in-pane 搜索、scrollback 翻页）。
		if (!isTui) {
			// Ctrl+F — open/close in-pane search bar. TUI 里 Ctrl+F 通常
			// 是 page down (vim/less)，所以非 TUI 才拦截。
			if (mod && !e.shiftKey && !e.altKey && (e.key === 'f' || e.key === 'F')) {
				if (termSearchOpen) {
					closeSearchBar();
				} else {
					openSearchBar();
				}
				e.preventDefault();
				return;
			}

			// PageUp/Down for scrollback navigation. Modifier required so we don't
			// hijack programs like less that use bare PageUp.
			if (e.shiftKey && !e.ctrlKey && !e.altKey && e.key === 'PageUp') {
				manager.scrollUp(paneId, manager.rows(paneId) - 1);
				maybePrefetchOlder();
				refreshScrollState();
				showScrollbarTemporarily();
				e.preventDefault();
				return;
			}
			if (e.shiftKey && !e.ctrlKey && !e.altKey && e.key === 'PageDown') {
				manager.scrollDown(paneId, manager.rows(paneId) - 1);
				refreshScrollState();
				showScrollbarTemporarily();
				e.preventDefault();
				return;
			}
		}

		// 5. Default: pass through to kernel's key encoder (非 TUI 下)
		if (!isTui && manager.handleKeyDown(paneId, e)) {
			e.preventDefault();
			refreshScrollState();

			// §1.32 (2026-05-20) Wave B: route every buffer-affecting key
			// through the unit-tested `inputBufferTracker` state machine.
			// Adds Ctrl+U / Ctrl+W / Ctrl+K (readline kills, Bug #4) on
			// top of the original char-append / backspace / cursor-clear
			// behaviour. Enter is now treated as `clear` too (was
			// previously not handled, leaving a stale buffer after each
			// command).
			dispatchBufferEvent(e);
		}
	}

	function onContainerWheel(e: WheelEvent) {
		if (!alive || !attached) return;

		// ★ TUI 模式下: 将滚轮编码为 SGR 鼠标滚动事件转发给 PTY
		// 利用 handleWheel 的返回值——只有 TUI 启用了 mouse reporting
		// 且字节真的发出去时才 preventDefault。否则（如 claude code 这
		// 类启用了 cursor hidden 让 sticky=true 但不接管鼠标的 inline-TUI）
		// 落到下方的 scrollback 分支，用户仍能向上翻页 host 历史。
		if (isTuiSticky() && manager.handleWheel(paneId, e)) {
			e.preventDefault();
			return;
		}

		// Only intercept when there's actually scrollback to scroll through.
		const { total } = manager.scrollState(paneId);
		if (total === 0) return;

		const delta = e.deltaY;
		const lines = Math.max(1, Math.round(Math.abs(delta) / 30));
		if (delta < 0) {
			manager.scrollUp(paneId, lines);
		} else {
			manager.scrollDown(paneId, lines);
		}

		refreshScrollState();
		showScrollbarTemporarily();
		e.preventDefault();
	}


function onContextMenu(e: MouseEvent) {
	if (!alive || !attached) return;
	// TUI 鼠标上报模式下，右键由 TUI 处理，不显示 RidgePane 右键菜单
	if (manager.isMouseReporting(paneId)) return;
	e.preventDefault();
	const sel = manager.getSelectionText(paneId);
	showContextMenu(e.clientX, e.clientY, [
		...(sel
			? [{ id: 'term-copy', label: '复制', action: () => { void writeText(sel); } }]
			: []),
		{ id: 'term-paste', label: '粘贴', action: () => {
			void readText().then((t) => { if (t) pasteIntoPane(t); });
		}},
		{ id: 'term-sep1', divider: true },
		{ id: 'term-select-all', label: '全选', action: () => manager.selectAll(paneId) },
		{ id: 'term-clear', label: '清空', action: () => {
			// §B.2 (2026-05-08) — full physical clear: grid + scrollback +
			// cursor home, all in-kernel without a PTY round trip. Pre-fix
			// this sent only Ctrl+L which the shell translated into ED 2 +
			// cursor home — visible grid cleared but pageUp resurrected
			// everything the user wanted gone (documented "clear 不能完全
			// 清理" symptom). The new path:
			//   1. `\x1b[H\x1b[2J` — cursor home + clear visible grid
			//      (sent to PTY so the prompt redraws cleanly above the
			//      blank rows; without this the shell still thinks the
			//      cursor is on the old row).
			//   2. `manager.clearScrollback(paneId)` — physical drop of
			//      the in-memory ring buffer + viewport snap to live.
			if (isTauri()) {
				void invoke('write_to_pty', { paneId, data: '\x1b[H\x1b[2J' }).catch(() => {});
			}
			manager.clearScrollback(paneId);
		}},
		// §1.23 (2026-05-05): split + close options restored to right-click
		// menu. Pre-xterm-removal Pane.svelte never carried these; user
		// asked for a richer menu now that splits are a primary affordance.
		{ id: 'term-sep2', divider: true },
		// @ridge/split convention: direction='horizontal' lays panes
		// side-by-side (sets RgPane `width` → flex-row), so a "向右拆分"
		// click should pass 'horizontal'; direction='vertical' stacks
		// them (sets `height` → flex-col), so "向下拆分" passes 'vertical'.
		// The previous mapping was inverted — see RgPane.svelte's
		// `dim = direction === 'horizontal' ? 'width' : 'height'`.
		{ id: 'term-split-right', label: '向右拆分', action: () => {
			void splitPane(paneId, 'horizontal');
		}},
		{ id: 'term-split-down', label: '向下拆分', action: () => {
			void splitPane(paneId, 'vertical');
		}},
		{ id: 'term-sep3', divider: true },
		{ id: 'term-close', label: '关闭面板', action: () => {
			void closePane(paneId);
		}},
	], 'terminal', paneId, workspaceId);
}

// §1.23 (2026-05-05): scroll-to-bottom affordance + side scrollbar.
// When user pages back into history, the floating button gives a
// one-click jump to the live grid bottom. The scrollbar visualises
// the current scroll position over the combined scrollback + viewport
// span, with a draggable thumb. Both update from the same scroll-state
// mirror to stay consistent.
//
// `scrollOffset` and `scrollTotal` mirror `manager.scrollState(paneId)`.
// `offset` is lines BACK from live grid (0 = at bottom). `total` is the
// scrollback line count. Combined visible span is `total + rows`.
let isAtBottom = $state(true);
let scrollOffset = $state(0);
let scrollTotal = $state(0);
function refreshScrollState() {
	if (!alive || !attached) return;
	const s = manager.scrollState(paneId);
	if (s.offset !== scrollOffset) scrollOffset = s.offset;
	if (s.total !== scrollTotal) scrollTotal = s.total;
	const next = s.offset === 0;
	if (next !== isAtBottom) isAtBottom = next;
}
function jumpToBottom() {
	if (!alive || !attached) return;
	manager.scrollToBottom(paneId);
	refreshScrollState();
	imeHelper?.focus();
}

// Scrollbar geometry, derived from current state. Both thumb top and
// thumb height are FRACTIONS of the pane container's height so CSS can
// express them as `top: x%; height: y%`.
let scrollbarVisible = $derived(scrollTotal > 0);
let scrollbarThumbHeightPct = $derived.by(() => {
	if (!scrollbarVisible) return 100;
	const r = manager.rows(paneId);
	const span = scrollTotal + r;
	if (span <= 0) return 100;
	// Minimum 4% so very-deep scrollback keeps the thumb grabbable.
	return Math.max(4, (r / span) * 100);
});
let scrollbarThumbTopPct = $derived.by(() => {
	if (!scrollbarVisible) return 0;
	const r = manager.rows(paneId);
	const span = scrollTotal + r;
	if (span <= 0) return 0;
	// Top of viewport in absolute lines = total - offset
	// (offset=0 → top of viewport = total = bottom of scroll-able range)
	const raw = ((scrollTotal - scrollOffset) / span) * 100;
	// Clamp so `top + height` ≤ 100%. On a very short pane the 4%-min
	// thumb height takes a large fraction of the track, so an unclamped
	// raw value near 95-100% would push the thumb's bottom past the
	// track end and visually overhang the cell content area.
	return Math.max(0, Math.min(raw, 100 - scrollbarThumbHeightPct));
});

// Drag-thumb interaction.
// `dragging` carries the active pointer's start state so move events
// can compute a delta-based new offset without re-measuring the track.
let scrollbarTrackEl: HTMLDivElement | undefined = $state(undefined);
let dragging: { startY: number; startOffset: number; trackH: number } | null = null;
function onScrollbarThumbPointerDown(e: PointerEvent) {
	if (!alive || !attached || !scrollbarTrackEl) return;
	e.stopPropagation();
	e.preventDefault();
	const rect = scrollbarTrackEl.getBoundingClientRect();
	dragging = {
		startY: e.clientY,
		startOffset: scrollOffset,
		trackH: rect.height,
	};
	(e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
	// Suppress text selection across the whole window for the drag's
	// duration. `e.preventDefault()` only blocks selection on the thumb
	// element itself; once setPointerCapture routes movement events past
	// the thumb the browser will otherwise extend a selection across
	// whatever lies under the cursor (terminal canvas, Explorer rows,
	// title bar). Restored on pointerup. Use both the standard property
	// and the WebKit prefix for Tauri's older webview versions.
	document.body.style.userSelect = 'none';
	(document.body.style as CSSStyleDeclaration & { webkitUserSelect?: string }).webkitUserSelect = 'none';
}
function onScrollbarThumbPointerMove(e: PointerEvent) {
	if (!dragging) return;
	const r = manager.rows(paneId);
	const span = scrollTotal + r;
	if (span <= 0 || dragging.trackH <= 0) return;
	// Pixels-per-line on the track:
	const px_per_line = dragging.trackH / span;
	if (px_per_line <= 0) return;
	const dy = e.clientY - dragging.startY;
	// Dragging DOWN reduces offset (closer to bottom), UP increases it.
	const targetOffset = Math.max(
		0,
		Math.min(scrollTotal, Math.round(dragging.startOffset - dy / px_per_line)),
	);
	const delta = targetOffset - scrollOffset;
	if (delta > 0) manager.scrollUp(paneId, delta);
	else if (delta < 0) manager.scrollDown(paneId, -delta);
	refreshScrollState();
}
function onScrollbarThumbPointerUp(e: PointerEvent) {
	if (!dragging) return;
	dragging = null;
	(e.currentTarget as HTMLElement).releasePointerCapture(e.pointerId);
	// Restore window-wide text selection (paired with the userSelect
	// suppression in onScrollbarThumbPointerDown). Reset to '' so any
	// app-wide CSS rule keeps owning the property.
	document.body.style.userSelect = '';
	(document.body.style as CSSStyleDeclaration & { webkitUserSelect?: string }).webkitUserSelect = '';
}

// Click on the empty track jumps the thumb center to the cursor — same
// behaviour as native OS scrollbars when you click outside the thumb.
function onScrollbarTrackClick(e: MouseEvent) {
	if (!alive || !attached || !scrollbarTrackEl) return;
	const rect = scrollbarTrackEl.getBoundingClientRect();
	const r = manager.rows(paneId);
	const span = scrollTotal + r;
	if (span <= 0 || rect.height <= 0) return;
	// Where the click landed as a fraction of the track.
	const fraction = (e.clientY - rect.top) / rect.height;
	// Convert to "line at top of viewport" in the absolute span:
	const viewportTopLine = Math.round(fraction * span);
	// Then offset = total - viewportTopLine (clamped).
	const targetOffset = Math.max(0, Math.min(scrollTotal, scrollTotal - viewportTopLine));
	const delta = targetOffset - scrollOffset;
	if (delta > 0) manager.scrollUp(paneId, delta);
	else if (delta < 0) manager.scrollDown(paneId, -delta);
	refreshScrollState();
}

function onContainerPointerDown() {
	activePaneId.set(paneId);
	// In 'direct' mode the IME helper isn't rendered at all (see below),
	// so focus the container directly — its keydown handler still
	// services every printable key without IME composition.
	if ($settingsStore.terminalImeMode === 'direct') {
		container?.focus();
		return;
	}
	// Focus the IME helper textarea so keystrokes (including IME
	// composition) flow to us. Falling back to container focus if the
	// helper isn't mounted yet (early HMR / SSR edge case).
	if (imeHelper) {
		imeHelper.focus();
		// Reposition AFTER focus so the candidate window (if it appears
		// from a held composition) anchors to the freshly-computed spot.
		repositionImeHelper();
	} else {
		container?.focus();
	}
}

// Mousedown's default moves focus to the click target's nearest
// focusable ancestor. The container has tabindex=-1 (focusable on click),
// so without preventDefault the browser yanks focus back to the container
// AFTER pointerdown moved it to the IME textarea — and IME composition
// events only fire on the textarea. Suppress the default to keep focus
// on the textarea. Plain keydown is now also handled at the container
// level so it works regardless of which child has focus.
function onContainerMouseDown(e: MouseEvent) {
	e.preventDefault();
}

</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div
	bind:this={container}
	class="rg-pane-container h-full w-full min-h-0 min-w-0 outline-none relative"
	class:bell-flash={bellFlash}
	class:rg-backend-switching={backendSwitching}
	style="background: var(--rg-term-bg); contain: strict;"
	role="application"
	aria-label="终端"
	tabindex="-1"
	data-rg-pane-id={paneId}
	data-rg-pane-active={false}
	onwheel={onContainerWheel}
	oncontextmenu={onContextMenu}
	onmousedown={onContainerMouseDown}
	onpointerdown={onContainerPointerDown}
	onkeydown={onContainerKeyDown}
>
	<!-- IME helper textarea. Gated on Settings.terminalImeMode === 'ime'
	     so users who only type ASCII can flip to 'direct' mode and the
	     textarea never enters the DOM — OS IME has no focusable input
	     to attach to, and `onContainerKeyDown` services every keystroke
	     directly with no compositionstart/update/end round-trip. The
	     "history input flickers with cursor" symptom (Microsoft Pinyin /
	     Sogou intercepting plain ASCII as a pinyin composition) goes
	     away in 'direct' mode. -->
	{#if $settingsStore.terminalImeMode === 'ime'}
		<textarea
			bind:this={imeHelper}
			class="rg-ime-helper"
			class:is-composing={isComposing}
			aria-label="终端输入"
			autocomplete="off"
			autocapitalize="off"
			spellcheck="false"
			oncompositionstart={onCompositionStart}
			oncompositionupdate={onCompositionUpdate}
			oncompositionend={onCompositionEnd}
			onfocus={onImeHelperFocus}
			onpaste={onImeHelperPaste}
		></textarea>
	{/if}

	<!-- §1.23 (2026-05-05): floating scroll-to-bottom button.
	     Only shown when the user has paged into history (`isAtBottom`
	     starts true and stays true unless wheel/PageUp triggered a scroll
	     that left scroll_offset > 0). Click jumps the kernel viewport
	     back to the live grid and re-focuses the IME helper for input. -->
	{#if scrollTotal > 0 && scrollOffset > 0}
		<button
			type="button"
			class="rg-jump-bottom"
			title="滚动到最新输出 (End)"
			onclick={jumpToBottom}
			aria-label="滚动到最新输出"
		>
			<svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true">
				<path d="M3 5l5 5 5-5" stroke="currentColor" stroke-width="2" fill="none" stroke-linecap="round" stroke-linejoin="round"/>
				<path d="M3 10l5 5 5-5" stroke="currentColor" stroke-width="2" fill="none" stroke-linecap="round" stroke-linejoin="round" opacity="0.55"/>
			</svg>
		</button>
	{/if}

	<!-- §1.23 (2026-05-05): side scrollbar overlay.
	     Visible only when there is actual scrollback (total > 0). Track
	     covers full pane height; thumb position + height reflect current
	     viewport within (scrollback + viewport) span. Click track to
	     jump; drag thumb for live scrolling. -->
	{#if scrollbarVisible}
		<div
			class="rg-scrollbar-track"
			class:is-active={isScrolling}
			bind:this={scrollbarTrackEl}
			onclick={onScrollbarTrackClick}
			role="presentation"
		>
			<div
				class="rg-scrollbar-thumb"
				role="scrollbar"
				tabindex="-1"
				aria-orientation="vertical"
				aria-controls={`rg-pane-${paneId}`}
				aria-valuemin={0}
				aria-valuemax={scrollTotal}
				aria-valuenow={scrollTotal - scrollOffset}
				style="top: {scrollbarThumbTopPct}%; height: {scrollbarThumbHeightPct}%;"
				onpointerdown={onScrollbarThumbPointerDown}
				onpointermove={onScrollbarThumbPointerMove}
				onpointerup={onScrollbarThumbPointerUp}
				oncontextmenu={(e) => e.stopPropagation()}
			></div>
		</div>
	{/if}
</div>

<TerminalHistoryPopup
    bind:this={historyPopupEl}
    query={currentInputBuffer.text}
    isVisible={historyPopupOpen}
    position={historyPopupPosition}
    onSelect={(cmd) => {
        // 加入前端历史库，供后续弹窗使用
        terminalHistoryStore.add(cmd);
        // 清除 shell 中已键入的筛选文本，然后用选中命令替换
        // §1.32 Wave F: prefer the PTY-derived snapshot for the replay
        // length so completion echoes / $VAR expansion / Ctrl+R redraws
        // don't leave garbage on the line. Fall back to the keystroke
        // mirror if the snapshot is unavailable (no input observed
        // yet, multi-row wrap, prompt redrew underneath).
        const snapshot = manager.readShellInputSnapshot(paneId);
        const replay = computeReplaySequence(snapshot ?? currentInputBuffer);
        if (replay) manager.write(paneId, replay);
        // 写入选中命令 + 回车执行
        manager.write(paneId, cmd + '\r');
        currentInputBuffer = EMPTY_INPUT_BUFFER;
        historyPopupOpen = false;
        imeHelper?.focus();
    }}
    onClose={() => { historyPopupOpen = false; imeHelper?.focus(); }}
/>

{#if termSearchOpen}
	<div class="rg-search-bar">
		<input
			bind:this={searchInputEl}
			class="rg-search-input"
			type="text"
			placeholder="在终端中查找…"
			bind:value={searchQuery}
			oninput={refreshSearch}
			onkeydown={onSearchInputKey}
		/>
		<span class="rg-search-count">
			{#if searchQuery.length === 0}
				—
			{:else if searchInfo.count === 0}
				无匹配
			{:else}
				{searchInfo.activeIndex + 1}/{searchInfo.count}
			{/if}
		</span>
		<button
			class="rg-search-btn"
			class:active={searchCaseSensitive}
			title="区分大小写"
			onclick={() => { searchCaseSensitive = !searchCaseSensitive; refreshSearch(); }}
		>Aa</button>
		<button
			class="rg-search-btn"
			title="上一个 (Shift+Enter)"
			onclick={() => { manager.searchPrev(paneId); searchInfo = manager.searchInfo(paneId); }}
		>↑</button>
		<button
			class="rg-search-btn"
			title="下一个 (Enter)"
			onclick={() => { manager.searchNext(paneId); searchInfo = manager.searchInfo(paneId); }}
		>↓</button>
		<button
			class="rg-search-btn"
			title="关闭 (Esc)"
			onclick={closeSearchBar}
		>×</button>
	</div>
{/if}

<style>
	/* `.rg-pane-container { contain: strict }` is applied inline at the
	 * element (style="contain: strict;") to keep this stylesheet free of
	 * empty rulesets. The strict containment lets the browser skip
	 * layout/paint on unrelated mutations elsewhere — small win in
	 * multi-pane setups. */
	/* P3.9 — backend switch fade. 200ms opacity transition hides the
	 * brief moment between issuing set_pane_delta_mode and the first
	 * frame arriving from the new backend. R4 architectural mitigation. */
	.rg-pane-container {
		transition: opacity 200ms ease-out;
	}
	.rg-pane-container.rg-backend-switching {
		opacity: 0.45;
		pointer-events: none;
	}

	.rg-pane-container.bell-flash {
		/* Brief inset highlight to draw the eye on BEL (0x07). 120ms is
		 * long enough to register, short enough not to be annoying. */
		box-shadow: inset 0 0 0 2px rgba(255, 200, 0, 0.65);
		transition: box-shadow 60ms ease-out;
	}
	.rg-ime-helper {
		/* IME anchor textarea. The OS IME treats this as a visible
		 * focused input field and renders the preedit INSIDE it —
		 * so it does NOT also pop up a separate preedit display that
		 * would cover the canvas overlay we paint ourselves. The
		 * candidate-list popup (你/妳/呢/...) still appears as a
		 * separate OS window — that's the part the user needs to
		 * read and choose from.
		 *
		 * The textarea itself is invisible to the user: `color:
		 * transparent` hides the preedit char glyphs the OS draws
		 * into it; `background: transparent` and `caret-color:
		 * transparent` keep the rest clean. We can NOT use
		 * `opacity: 0` because some OS IMEs (Microsoft Pinyin
		 * notably) treat an opacity:0 input as "hidden" and switch
		 * back to popup-rendered preedit — undoing the whole point.
		 * Pixel rect is one cell-sized box at the kernel cursor so
		 * the OS positions the candidate popup correctly below it. */
		position: absolute;
		left: 0;
		top: 0;
		width: var(--rg-ime-cell-w, 8px);
		height: var(--rg-ime-cell-h, 18px);
		opacity: 1;
		pointer-events: none;
		caret-color: transparent;
		background: transparent;
		color: transparent;
		border: none;
		outline: none;
		padding: 0;
		margin: 0;
		resize: none;
		overflow: hidden;
		font-family: var(--rg-term-font-family, ui-monospace, 'Cascadia Code', Consolas, monospace);
		font-size: var(--rg-term-font-size, 14px);
		line-height: var(--rg-ime-cell-h, 18px);
	}
	.rg-ime-helper.is-composing {
		/* During composition we stream the preedit text directly through
		 * the PTY (see `onCompositionUpdate` in this file) so the shell
		 * echoes it back and the user sees pinyin/kana letters appear
		 * at the cursor cell — drawn by the wasm canvas renderer, not
		 * by an overlay. The textarea itself stays invisible; we only
		 * resize its width slightly so the OS candidate popup anchors
		 * close to the user's typed letters. */
		width: var(--rg-ime-cell-w, 8px);
		height: var(--rg-ime-cell-h, 18px);
		opacity: 0;
		pointer-events: none;
		caret-color: transparent;
		color: transparent;
		background: transparent;
	}
	.rg-jump-bottom {
		/* §1.23 — floating scroll-to-bottom shortcut. Anchored to the
		 * pane's bottom-right corner; only rendered when the user has
		 * paged into scrollback. Pointer-events:auto despite the parent
		 * container blocking some surfaces because it's the user's
		 * primary affordance to return to the live grid. */
		position: absolute;
		right: 14px;
		bottom: 14px;
		display: inline-flex;
		align-items: center;
		justify-content: center;
		width: 30px;
		height: 30px;
		border-radius: 9999px;
		border: 1px solid var(--rg-border, #333);
		background: var(--rg-surface, rgba(30, 30, 30, 0.92));
		color: var(--rg-fg, #ddd);
		cursor: pointer;
		opacity: 0.85;
		box-shadow: 0 4px 12px rgba(0, 0, 0, 0.3);
		transition: opacity 120ms ease-out, transform 120ms ease-out, background 120ms ease-out;
		z-index: 21;
	}
	.rg-jump-bottom:hover {
		opacity: 1;
		background: var(--rg-accent, #4a8cff);
		color: #fff;
		transform: translateY(-1px);
	}
	.rg-jump-bottom:focus {
		outline: none;
		box-shadow: 0 0 0 2px var(--rg-accent, #4a8cff);
	}

	/* §1.23 — side scrollbar (track + thumb). Track is a thin overlay
	 * column on the right edge; thumb is positioned via inline-style
	 * `top` / `height` percentages computed from scroll state. Track
	 * stays transparent so the terminal's last column glyphs show through
	 * if the pane is narrow; the thumb itself is the visible affordance. */
	.rg-scrollbar-track {
		position: absolute;
		top: 0;
		right: 0;
		bottom: 0;
		width: 10px;
		z-index: 20;
		cursor: pointer;
		opacity: 0;
		transition: opacity 150ms ease-out;
		pointer-events: none;
	}
	.rg-scrollbar-track.is-active,
	.rg-pane-container:hover .rg-scrollbar-track {
		opacity: 1;
		pointer-events: auto;
	}
	.rg-scrollbar-thumb {
		position: absolute;
		left: 2px;
		right: 2px;
		min-height: 18px;
		border-radius: 6px;
		background: var(--rg-fg-muted, rgba(180, 180, 180, 0.3));
		opacity: 0.55;
		cursor: grab;
		transition: opacity 120ms ease-out, background 120ms ease-out;
		touch-action: none;
	}
	.rg-scrollbar-track.is-active .rg-scrollbar-thumb,
	.rg-scrollbar-thumb:hover {
		opacity: 0.85;
		background: var(--rg-accent, #4a8cff);
	}
	.rg-scrollbar-thumb:active {
		opacity: 1;
		cursor: grabbing;
		background: var(--rg-accent, #4a8cff);
	}

	.rg-search-bar {
		position: absolute;
		top: 4px;
		right: 12px;
		display: flex;
		align-items: center;
		gap: 4px;
		padding: 4px 6px;
		background: var(--rg-bg, #1e1e1e);
		border: 1px solid var(--rg-border, #333);
		border-radius: 6px;
		box-shadow: 0 4px 12px rgba(0,0,0,.3);
		font-size: 12px;
		z-index: 10;
	}
	.rg-search-input {
		background: transparent;
		border: none;
		outline: none;
		color: var(--rg-fg, #ddd);
		width: 180px;
		font-family: inherit;
		font-size: 12px;
	}
	.rg-search-count {
		color: var(--rg-fg-muted, #888);
		min-width: 50px;
		text-align: center;
	}
	.rg-search-btn {
		background: transparent;
		border: 1px solid transparent;
		color: var(--rg-fg, #ddd);
		padding: 2px 6px;
		border-radius: 4px;
		cursor: pointer;
		font-size: 12px;
		min-width: 22px;
	}
	.rg-search-btn:hover {
		background: var(--rg-hover, rgba(255,255,255,.08));
	}
	.rg-search-btn.active {
		background: var(--rg-accent, #4a8cff);
		color: #fff;
	}
</style>
