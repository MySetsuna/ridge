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
import { acquireClipboardImagePath, imagePathFromClipboardEvent } from '$lib/terminal/clipboardImage';
import { t, tr } from '$lib/i18n';
import { activePaneId, activeWorkspaceId, setPaneCwd, paneOscTitleStore, terminalTitles, splitPane, closePane } from '$lib/stores/paneTree';
import type { KernelEvent } from '$lib/terminal/manager';
import { ensurePtyBridge, enableDeltaModeThenFit } from '$lib/terminal/ptyBridge';
import { pushTerminalThemeNow } from '$lib/terminal/themeBridge';
import { settingsStore } from '$lib/stores/settings';
import { remoteRunning, cloudHostOnline } from '$lib/stores/remoteStatus';
import { showContextMenu } from '$lib/stores/contextMenu';
import { get } from 'svelte/store';
import { TerminalManager } from '$lib/terminal/manager';
import { isTuiActive, hasLiveTuiSignal, TUI_STICKY_MS_DEFAULT } from '$lib/terminal/tuiGate';
import {
	deriveBufferEvent,
	updateInputBuffer,
	computeReplaySequence,
	EMPTY_INPUT_BUFFER,
	type InputBufferState,
} from './inputBufferTracker';
import { terminalHistoryStore, dedupKeepFirst, filterByPrefix, nextHistorySelection } from '$lib/stores/terminalHistory';
import { activeBgImage } from '$lib/stores/themes';

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

// P4.4 (2026-05-21) — removed the parserBackend live-switch state.
// The Rust path is now unconditional; `set_pane_delta_mode(true)` is
// still called on attach (see onMount IIFE below) so the backend
// `delta_mode` AtomicBool is in the expected state for the channel
// path. No more `backendSwitching` fade — there is no other backend
// to switch to.

// PTY listener subscriptions used to live here as ptyUnlisten /
// ptyClosedUnlisten. Both moved to `$lib/terminal/ptyBridge` (TASKS §5.1)
// so listener lifetime tracks the wasm kernel lifetime (manager.attach →
// manager.detach) rather than this Svelte component's mount cycle —
// otherwise PTY bytes emitted during the unmount window of a split /
// reparent would be silently dropped.

const manager = TerminalManager.instance();

// §web-remote: compile-time flag for the desktop-in-browser SPA build
// (`RIDGE_WEB_REMOTE=1 vite build`, defined in vite.config.js). When true,
// this pane is a CONTROLLER viewing the host's PTY over the LAN-WS shim, so
// the per-pane "re-claim my size" affordance must be available even though
// the host-side `remoteRunning` store is false on the controller. See
// tauriShim/core.ts — browser-only surfaces gate on this flag, not isTauri().
const WEB_REMOTE = import.meta.env.RIDGE_WEB_REMOTE === true;

// §1.32 (2026-05-20) Wave C: state is now `{ text, cursorCol }` so
// ArrowLeft / Home / Delete / mid-line edits preserve the buffer
// instead of clearing it. See `inputBufferTracker.ts` for the rules.
let currentInputBuffer = $state<InputBufferState>(EMPTY_INPUT_BUFFER);

// §1.34 (2026-05-22) — shell-history overlay state. The popup renders
// directly on the wasm canvas via `manager.setHistoryOverlay(...)` (not
// a Svelte DOM element) so it inherits pane focus, theme, cell metrics
// and DPR for free. See
// `packages/ridge-term/src/render/renderer.rs::HistoryOverlay` for the
// renderer state and `webgpu.rs::draw_history_overlay` for the paint.
let historyOverlayOpen = $state(false);
// FULL filtered candidate list, newest-first. The renderer is fed a WINDOW
// (slice) of this; `historyOverlaySelected` indexes into the full list.
let historyOverlayItems = $state<string[]>([]);
let historyOverlaySelected = $state(-1);
let historyOverlayAbove = $state(true);
let historyOverlayAnchor = $state<{ row: number; col: number } | null>(null);
// §history-scroll — window start within the full list + window height.
let historyOverlayFirstVisible = $state(0);
let historyOverlayWindow = $state(12);

// Hard ceiling on the visible window. The popup also can't exceed the space
// above/below the anchor; everything beyond is reachable by scrolling (a
// scrollbar shows position). The wasm renderer independently caps to 40.
const HISTORY_OVERLAY_MAX_WINDOW = 16;
// Cap the in-memory candidate list so a huge shell history doesn't bloat each
// push; 500 recent matches is far more than anyone scrolls through.
const HISTORY_OVERLAY_MAX_ITEMS = 500;

function snapshotHistoryItems(query: string): string[] {
	const all = dedupKeepFirst(get(terminalHistoryStore));
	return filterByPrefix(all, query).slice(0, HISTORY_OVERLAY_MAX_ITEMS);
}

// Window height = as many rows as fit above/below the anchor, capped — a
// Warp-style "show many, scroll for the rest" popup instead of a fixed 10.
function computeHistoryWindow(anchorRow: number, placeAbove: boolean): number {
	const rows = manager.rows(paneId) || 24;
	const avail = placeAbove ? anchorRow : Math.max(0, rows - anchorRow - 1);
	return Math.max(3, Math.min(HISTORY_OVERLAY_MAX_WINDOW, avail));
}

function pushHistoryOverlay(): void {
	if (!historyOverlayOpen || !historyOverlayAnchor) return;
	const total = historyOverlayItems.length;
	const win = Math.min(historyOverlayWindow, total);
	// Clamp the window start so it never runs past the end of the list.
	const first = Math.max(0, Math.min(historyOverlayFirstVisible, Math.max(0, total - win)));
	historyOverlayFirstVisible = first;
	const slice = historyOverlayItems.slice(first, first + win);
	const sliceSelected = historyOverlaySelected >= 0 ? historyOverlaySelected - first : -1;
	manager.setHistoryOverlay(
		paneId,
		slice,
		sliceSelected,
		historyOverlayAnchor.row,
		historyOverlayAnchor.col,
		historyOverlayAbove,
		total,
		first,
	);
}

function openHistoryOverlay(): boolean {
	const anchor = manager.inputAnchorResolved?.(paneId);
	if (!anchor) return false;
	const items = snapshotHistoryItems(currentInputBuffer.text);
	if (items.length === 0) return false;
	historyOverlayItems = items;
	// §方向一致 (2026-06-11): pre-select the NEWEST entry (index 0, painted at
	// the top) so the popup opens with the most recent command highlighted —
	// one Enter repeats it. Arrow keys then move the highlight in screen
	// direction (↑ newer / ↓ older) via `nextHistorySelection`.
	historyOverlaySelected = 0;
	historyOverlayFirstVisible = 0;
	historyOverlayAnchor = { row: anchor.row, col: anchor.col };
	const rows = manager.rows(paneId);
	historyOverlayAbove = anchor.row >= rows / 2;
	historyOverlayWindow = computeHistoryWindow(anchor.row, historyOverlayAbove);
	historyOverlayOpen = true;
	pushHistoryOverlay();
	return true;
}

function closeHistoryOverlay(): void {
	if (!historyOverlayOpen) return;
	historyOverlayOpen = false;
	historyOverlaySelected = -1;
	historyOverlayItems = [];
	historyOverlayFirstVisible = 0;
	historyOverlayAnchor = null;
	manager.clearHistoryOverlay(paneId);
}

function commitHistorySelection(execute: boolean): void {
	if (historyOverlaySelected < 0
		|| historyOverlaySelected >= historyOverlayItems.length) {
		closeHistoryOverlay();
		return;
	}
	const cmd = historyOverlayItems[historyOverlaySelected];
	terminalHistoryStore.add(cmd);
	// Wipe whatever the user already typed on the line before injecting
	// the picked command. Prefer the kernel-cell snapshot (robust to Tab
	// completion / alias expansion); fall back to the keystroke mirror.
	const snapshot = manager.readShellInputSnapshot(paneId);
	const replay = computeReplaySequence(snapshot ?? currentInputBuffer);
	if (replay) manager.write(paneId, replay);
	manager.write(paneId, execute ? cmd + '\r' : cmd);
	currentInputBuffer = EMPTY_INPUT_BUFFER;
	closeHistoryOverlay();
	imeHelper?.focus();
}

function moveHistorySelection(delta: number): void {
	if (!historyOverlayOpen) return;
	const n = historyOverlayItems.length;
	if (n === 0) {
		closeHistoryOverlay();
		return;
	}
	// §方向一致 (2026-06-11): the list is newest-first (index 0 = newest,
	// painted at the TOP). Arrow keys move the highlight in SCREEN direction:
	// ArrowUp (delta<0) → smaller index → newer; ArrowDown (delta>0) → larger
	// index → older. Boundaries clamp (no wrap / no auto-dismiss). See
	// `nextHistorySelection` for the full contract + tests.
	historyOverlaySelected = nextHistorySelection(historyOverlaySelected, n, delta);
	// §history-scroll — keep the selection inside the visible window.
	const win = Math.min(historyOverlayWindow, n);
	if (historyOverlaySelected < historyOverlayFirstVisible) {
		historyOverlayFirstVisible = historyOverlaySelected;
	} else if (historyOverlaySelected >= historyOverlayFirstVisible + win) {
		historyOverlayFirstVisible = historyOverlaySelected - win + 1;
	}
	pushHistoryOverlay();
}

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
	// §TUI (2026-06-01): keep the TUI gate alive after a paste so the
	// sticky-window doesn't decay while the user was interacting with
	// the context menu (inline-TUI heuristic decays in ~2s). Without
	// this, a right-click → paste sequence can silently exit TUI mode
	// and the next arrow key opens shell history instead of going to
	// the running TUI application (claude code / less / vim).
	touchTuiSticky();
	// Restore focus to the IME helper after paste (fall back to the container
	// only when the helper isn't mounted). The context menu (or async clipboard
	// readText) can steal focus from the pane; without this, IME composition and
	// subsequent keystrokes never reach the pane.
	(imeHelper ?? container)?.focus();
}

// §clipboard-image: 主动粘贴入口——先尝试剪贴板里的图片（落盘成临时 PNG，把绝对路径作为文本
// 粘入；终端里的 TUI 如 Claude Code 会把图片路径识别为图片附件），没有图片再 fallback 到文本
// 粘贴。所有「host 主动粘贴」入口（Ctrl+Shift+V / Cmd+V / Win Ctrl+V / 右键菜单）都走这里。
// 背景见 $lib/terminal/clipboardImage 与 src-tauri 的 commands/clipboard_image.rs。
async function pasteFromClipboard(): Promise<void> {
	try {
		const imgPath = await acquireClipboardImagePath();
		if (imgPath) {
			pasteIntoPane(imgPath);
			return;
		}
	} catch (err) {
		console.error('[clipboard-image] image paste failed, falling back to text', err);
	}
	const text = await readText().catch(() => null);
	if (!text) return;
	// §clipboard-image:「复制为路径 / Copy as path」场景——文本可能是带引号的图片文件路径。
	// 若它确实指向一个存在的图片文件，去引号后粘**裸**路径（CLI 才识别为图片）；否则普通粘文本。
	try {
		const imgPath = await invoke<string | null>('resolve_pasted_image_path', { text });
		if (imgPath) {
			pasteIntoPane(imgPath);
			return;
		}
	} catch (err) {
		console.error('[clipboard-image] resolve pasted path failed', err);
	}
	pasteIntoPane(text);
}

/** Refresh the TUI sticky timestamp when any signal suggests the TUI
 *  is still alive, preventing the inline-TUI heuristic decay from
 *  silently exiting TUI mode during user interaction with host UI
 *  elements (context menu, search bar, Ctrl+C grace window, etc.).
 *
 *  Safe to call anywhere inside the pane component.  Checks:
 *  - Live protocol signals: alt-screen, inline-TUI heuristic, mouse
 *    reporting — these are unambiguous.
 *  - DECCKM (app cursor keys): the TUI has explicitly claimed the
 *    arrow keys.
 *  - Hidden cursor: most TUIs hide the cursor while rendering; a
 *    hidden cursor with recent activity is strong evidence the TUI
 *    is still running (even if `noteCtrlCSent` suppressed the
 *    inline heuristic for the Ctrl+C grace window). */
function touchTuiSticky(): void {
	if (hasLiveTuiSignal({
		isAltScreen: manager.isAltScreen(paneId),
		isInlineTuiActive: manager.isInlineTuiActive(paneId),
		isMouseReporting: manager.isMouseReporting(paneId),
		isAppCursorKeys: manager.isAppCursorKeys(paneId),
		cursorVisible: manager.isCursorVisible(paneId),
	})) {
		lastTuiActiveTs = performance.now();
	}
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
			// Close the history popup on Enter / Ctrl+U — the real
			// "user intent to submit / abandon" signal. Driving close
			// from the keystroke side keeps it per-pane by construction
			// (vs. a global pty-newline event closing popups across
			// panes on every prompt redraw).
			closeHistoryOverlay();
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
let compositionEndTimer: ReturnType<typeof setTimeout> | null = null;
let imeFollowRaf: number | null = null;
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
// for up to TUI_STICKY_MS_DEFAULT — but only while the cursor stays hidden.
// Shell prompts always run with the cursor visible, so the moment
// the user is actually back at a prompt the sticky bit can no
// longer apply and host shortcuts re-enable as before.
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
		stickyMs: TUI_STICKY_MS_DEFAULT,
	});
}

// Host-priority shortcuts that should fire BEFORE TUI key forwarding.
// Convention shared by gnome-terminal / kitty / iTerm2 / wezterm /
// Windows Terminal: a small fixed set of modifier+Shift or platform-
// native combinations is always handled by the host so users can
// paste / copy / fullscreen even inside a TUI that captures everything
// else (claude code, opencode, etc.). When `isTui` is true, plain
// Ctrl+V / Ctrl+C flow through to the TUI as bytes — those are TUI
// semantics (SYN / SIGINT). Ctrl+Shift+V / Ctrl+Shift+C remain the
// always-available host escape hatches.
// Returns true when the host claimed the event.
function handleHostPriorityShortcut(e: KeyboardEvent, isTui: boolean): boolean {
	const isMac = /Mac|iPhone|iPod|iPad/.test(navigator.platform || '');
	const isWin = /Win/i.test(navigator.platform || '');
	const mod = e.ctrlKey || (isMac && e.metaKey);

	// Ctrl+Shift+V / Cmd+Shift+V — host paste, always wins on every
	// platform. Conservative POSIX users can reach the TUI's SYN byte
	// ("literal next" in readline) via Ctrl+Q instead.
	if (mod && e.shiftKey && !e.altKey && (e.key === 'v' || e.key === 'V')) {
		void pasteFromClipboard();
		e.preventDefault();
		return true;
	}

	// macOS Cmd+V (no Shift) — host paste, matches every other macOS app.
	// Skip when TUI is active so the TUI receives the byte.
	if (!isTui && isMac && e.metaKey && !e.ctrlKey && !e.shiftKey && !e.altKey
			&& (e.key === 'v' || e.key === 'V')) {
		void pasteFromClipboard();
		e.preventDefault();
		return true;
	}

	// Windows plain Ctrl+V — host paste, matches the default Windows
	// Terminal / PowerShell / ConHost behaviour where users expect
	// Ctrl+V to insert clipboard contents into stdin (the host pastes
	// before the byte ever reaches the running process). Unconditional:
	// even when a TUI is active, Ctrl+V always pastes on Windows —
	// this is the invariant every Windows terminal user expects.
	// POSIX platforms still send SYN to the TUI on plain Ctrl+V; that's
	// the xterm / gnome-terminal / iTerm2 convention.
	if (isWin && e.ctrlKey && !e.shiftKey && !e.metaKey && !e.altKey
			&& (e.key === 'v' || e.key === 'V')) {
		void pasteFromClipboard();
		e.preventDefault();
		return true;
	}

	// Windows plain Ctrl+C — copy when a selection exists (then clear it so the
	// next Ctrl+C reverts to interrupt), otherwise fall through so ^C reaches
	// the program. Matches Windows Terminal / ConHost's "copy on selection,
	// else interrupt" default. POSIX keeps the xterm convention (Ctrl+C is
	// always SIGINT; copy lives on Ctrl+Shift+C).
	if (isWin && e.ctrlKey && !e.shiftKey && !e.metaKey && !e.altKey
			&& (e.key === 'c' || e.key === 'C')) {
		const sel = manager.getSelectionText(paneId);
		if (sel) {
			void writeText(sel);
			manager.clearSelection(paneId);
			e.preventDefault();
			return true;
		}
		// no selection → fall through → ^C flows to the program / TUI
	}

	// Ctrl+Shift+C — host copy when a selection exists. Falls through
	// otherwise so a TUI that wants Ctrl+Shift+C as its own hotkey can
	// still receive it.
	if (mod && e.shiftKey && !e.altKey && (e.key === 'c' || e.key === 'C')) {
		const sel = manager.getSelectionText(paneId);
		if (sel) {
			void writeText(sel);
			manager.clearSelection(paneId);
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
	// During active composition use `pixelPositionFromCell` to recompute
	// pixel position from the locked grid anchor + current scroll offset,
	// so the OS IME candidate popup follows the cursor when the viewport
	// scrolls. The grid row stays locked in `composingAnchor` for the wasm
	// preedit overlay; only the pixel y tracks scrollOffset changes.
	const pos: { x: number; y: number; cellW: number; cellH: number } | null =
		isComposing && composingAnchor
			? (manager.pixelPositionFromCell?.(paneId, composingAnchor.row, composingAnchor.col) ?? composingAnchor)
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
	// §缺陷A: textarea 宽度固定 1px（见 CSS），不再随 cell 宽变化——故不再
	// 设 `--rg-ime-cell-w`。仅把 cell 高度喂给 CSS（`var(--rg-ime-cell-h)`）
	// 让候选框有一个 cell 高的竖直锚点矩形。
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
		// §缺陷A (2026-06-18): NOT widen the hidden textarea during composition.
		// 以前这里把 textarea 宽度撑成 `(charCount+1)*cellW`，col 0 时它 `left:0`
		// 贴分区左边界又被聚焦 → WebView2/Chromium 对 overflow:hidden 祖先隐式设
		// scrollLeft，把整屏内容左移（「最左侧顶偏」）。preedit 文本本就由 wasm
		// 渲染器画在 cell 网格上、候选框只需一个 cell 大小的锚点矩形，textarea
		// 无需随输入加宽。固定窄宽（见 .rg-ime-helper CSS）即可，借鉴 remote 端
		// hidden-input 固定 1px 的做法。
		diagLogIme('update', { dataLen: e.data?.length ?? 0, data: e.data });
	}

	function onImeHelperFocus() {
		// Anchor on focus too, in case the user clicked into the pane and
		// expects the next IME composition to appear near the current cursor.
		repositionImeHelper();
	}

	function onImeHelperPaste(e: ClipboardEvent) {
		// §clipboard-image: 优先处理粘贴进来的图片（截图等）。clipboardData 在桌面 webview 和
		// 远程浏览器都带图片项；落盘成临时 PNG 后把路径粘入，由 TUI 识别为图片。没有图片再走文本。
		const items = e.clipboardData?.items;
		let hasImage = false;
		if (items) {
			for (let i = 0; i < items.length; i++) {
				if (items[i].kind === 'file' && items[i].type.startsWith('image/')) {
					hasImage = true;
					break;
				}
			}
		}
		if (hasImage) {
			e.preventDefault();
			void imagePathFromClipboardEvent(e)
				.then((path) => { if (path) pasteIntoPane(path); })
				.catch((err) => console.error('[clipboard-image] paste-event image failed', err));
			return;
		}
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
			// §缺陷A: textarea 宽度由 CSS 固定为 1px，composition 期不再内联
			// 撑宽，故这里也不再需要恢复宽度——清空可能残留的内联 width（防御
			// 历史样式），让其回落到 CSS 固定值。
			imeHelper.style.width = '';
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
	compositionEndTimer = setTimeout(() => {
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

	// §1.34 — the wasm overlay tracks its own anchor cell; no JS
	// repositioning needed across IME commits. Anchor was captured
	// at open time and stays put through the kernel rasterizer.
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
	return invoke('resize_pane', { workspaceId, paneId, rows, cols, isAlt, isInlineTui }).then(
		() => undefined,
		(err) => {
			console.error('resize_pane', err);
		},
	);
}

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

	// Populate the shell-history store (Rust merges PowerShell / bash /
	// zsh history files). Idempotent + cheap; the store keeps a single
	// shared snapshot so per-pane mounts just refresh it.
	terminalHistoryStore.fetch();

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
		//
		// ORDERING CONTRACT (5b): this `ensurePtyBridge` MUST run BEFORE the
		// `enableDeltaModeThenFit` call in step 7 — it registers the pty-delta
		// Channel that setPaneDeltaMode + the deterministic post-fit Resize delta
		// depend on. enableDeltaModeThenFit asserts `hasPtyBridge` and warns if
		// this ordering is ever broken.
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
		// P4.4 — Rust path is the only path; unconditionally enable
		// delta_mode on attach. The backend defaults `delta_mode` to
		// false so the initial bytes use the legacy text path; this
		// call flips the gate after the pane has activated, at which
		// point the channel (registered by ptyBridge) starts
		// receiving delta frames.
		// 5b — deterministic fit AFTER the pty-delta Channel gate opens. P4.4
		// routes kernel grid resize solely through apply_delta(Resize), gated on
		// the Channel (registered by ensurePtyBridge above) + delta_mode. Awaiting
		// setPaneDeltaMode before fitPaneNow closes the attach-rAF-fit race that
		// left teammate panes stuck at 80×24 (they don't go through GUI split's
		// scheduleForceFitAfterSplit). 0×0/hidden workspaces still fall back to the
		// becomes-visible re-fit + kernel-grid self-heal. See bug_split_kernel_race.
		if (alive) {
			void enableDeltaModeThenFit(paneId, () => {
				if (alive) manager.fitPaneNow(paneId);
			});
		}

		// `pane-pty-closed` rebuild now lives in ptyBridge and persists
		// across this component's mount cycle, so we don't subscribe
		// here. See ptyBridge.ts.
	})();
});

// P4.4 (2026-05-21) — removed the parserBackend live-switch effect.
// With Rust path unconditional, the onMount IIFE's `enableDeltaModeThenFit`
// (which enables delta_mode then fits) is the only call site needed. No more
// 200ms fade mask — there is no backend to switch to.

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

// Continuous IME cursor following during composition. While IME is active
// the textarea position is re-synced every frame so the OS candidate popup
// follows the cursor when the viewport scrolls or cursor moves due to PTY
// output. The rAF loop starts/stops reactively with `isComposing`.
$effect(() => {
	if (!isComposing || !alive || !attached) return;
	const track = () => {
		if (!isComposing || !alive) return;
		repositionImeHelper();
		imeFollowRaf = requestAnimationFrame(track);
	};
	imeFollowRaf = requestAnimationFrame(track);
	return () => {
		if (imeFollowRaf !== null) {
			cancelAnimationFrame(imeFollowRaf);
			imeFollowRaf = null;
		}
	};
});

onDestroy(() => {
	alive = false;
	// Lift this pane's active scrollbar-drag text-selection guard so a pane that
	// unmounts mid-drag can't leave the whole app stuck at user-select:none.
	if (scrollbarDragGuardActive) endScrollbarDrag();
	if (bellFlashTimer !== null) {
		clearTimeout(bellFlashTimer);
		bellFlashTimer = null;
	}
	if (compositionEndTimer !== null) {
		clearTimeout(compositionEndTimer);
		compositionEndTimer = null;
	}
	if (imeFollowRaf !== null) {
		cancelAnimationFrame(imeFollowRaf);
		imeFollowRaf = null;
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
	// Close the history popup when this pane loses focus — otherwise an
	// inactive pane's overlay lingers on screen after the user clicks
	// into another pane. Keystrokes already can't reach it (its container
	// isn't focused), but the visual residue confuses.
	if (!isActive && historyOverlayOpen) closeHistoryOverlay();
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

		// 1. §1.34 — shell-history overlay (wasm canvas) takes the
		// highest priority while open. Modifier-free ↑ ↓ Enter → ←
		// Esc are consumed; any other printable / Backspace / Tab key
		// closes the overlay and falls through so the user's typing
		// still flows to the shell.
		if (historyOverlayOpen) {
			if (!e.ctrlKey && !e.metaKey && !e.altKey && !e.shiftKey) {
				if (e.key === 'ArrowUp') {
					moveHistorySelection(-1);
					e.preventDefault();
					return;
				}
				if (e.key === 'ArrowDown') {
					moveHistorySelection(1);
					e.preventDefault();
					return;
				}
				if (e.key === 'Enter') {
					commitHistorySelection(true);
					e.preventDefault();
					return;
				}
				if (e.key === 'ArrowRight') {
					if (historyOverlaySelected >= 0) {
						commitHistorySelection(false);
						e.preventDefault();
						return;
					}
					closeHistoryOverlay();
					// Fall through so the shell sees ArrowRight.
				}
				if (e.key === 'ArrowLeft') {
					closeHistoryOverlay();
					e.preventDefault();
					return;
				}
				if (e.key === 'Escape') {
					closeHistoryOverlay();
					e.preventDefault();
					return;
				} else if (e.key.length === 1
					|| e.key === 'Backspace'
					|| e.key === 'Tab') {
					closeHistoryOverlay();
				}
			}
		}

		if (isComposing || e.isComposing) return;

		// Compute TUI state before host shortcuts so plain Ctrl+C/V
		// can be forwarded to the TUI instead of being intercepted
		// for host copy/paste. Ctrl+Shift+C/V remain host escape
		// hatches regardless of TUI state.
		const isTui = isTuiSticky();

		// 2. Host-priority shortcuts (paste / copy-with-selection /
		// fullscreen / settings). When isTui is true, plain Ctrl+V
		// (Windows) / Cmd+V (macOS) and plain Ctrl+C copy are
		// skipped so the TUI receives the raw bytes. Ctrl+Shift+V
		// and Ctrl+Shift+C remain always-available host escape
		// hatches. See handleHostPriorityShortcut for the full table.
		if (handleHostPriorityShortcut(e, isTui)) return;

		// 3. TUI 模式下，优先透传给终端，TUI 未消费则继续执行
		// 注意: TUI 启用鼠标模式 (isMouseReporting) 也意味着键盘应优先给 TUI
		// 使用 isTuiSticky() 而非直接 OR，避免 claude /theme 这类静态
		// 菜单在 inline-TUI 2s decay 过期后误判出 TUI 模式。
		if (isTui) {
			if (manager.handleKeyDown(paneId, e, isTui)) {
				e.preventDefault();
				// §TUI (2026-06-01): every TUI key press refreshes the sticky
				// timestamp so the inline-TUI heuristic doesn't decay while
				// the user is actively interacting with the TUI application.
				// Without this, `noteCtrlCSent` (called on Ctrl+C inside
				// manager.handleKeyDown) suppresses the inline heuristic for
				// a grace window, and the very next key press sees no live
				// TUI signal → isTuiSticky → false → key goes to host.
				touchTuiSticky();
				return;
			}
		}

		// §1.34: ArrowUp / ArrowDown → open shell-history overlay
		// (rendered on the wasm canvas). The gate decision lives in the
		// wasm kernel via `should_allow_shell_history` so any TUI signal
		// — DECCKM / alt screen / mouse reporting / inline-TUI heuristic
		// / hidden cursor / 2 s sticky after any of those — short-circuits
		// to false and the arrow key falls through to the kernel encoder.
		if (
			!historyOverlayOpen
			&& (e.key === 'ArrowUp' || e.key === 'ArrowDown')
			&& !e.ctrlKey && !e.metaKey && !e.altKey && !e.shiftKey
			&& manager.shouldAllowShellHistory(paneId)
		) {
			if (openHistoryOverlay()) {
				e.preventDefault();
				return;
			}
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
			// §TUI: keep the TUI gate alive after a wheel event so the
			// sticky window doesn't decay during scroll-heavy interaction.
			touchTuiSticky();
			return;
		}

		// ★ Alternate-scroll fallback: alt-screen TUI that DIDN'T enable
		// mouse reporting (less / man / git log / fzf / claude /theme menu).
		// handleWheel returned false above (no mouse mode); there's no host
		// scrollback on alt screen, so without this the wheel is dead. Send
		// arrow-key presses instead — the xterm `alternateScroll` default.
		if (manager.wheelAltScroll(paneId, e)) {
			e.preventDefault();
			touchTuiSticky();
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
	// §TUI: refresh sticky timestamp BEFORE showing the context menu.
	// While the menu is open no keyboard/wheel events reach the pane,
	// so the inline-TUI heuristic (2 s decay) can expire during menu
	// interaction. Bumping lastTuiActiveTs here gives the user the
	// full sticky window to browse + close the menu without losing
	// TUI mode.
	touchTuiSticky();
	e.preventDefault();
	const sel = manager.getSelectionText(paneId);
	showContextMenu(e.clientX, e.clientY, [
		...(sel
			? [{ id: 'term-copy', label: tr('workspace.ctxCopy'), action: () => { void writeText(sel); } }]
			: []),
		{ id: 'term-paste', label: tr('workspace.ctxPaste'), action: () => {
			void pasteFromClipboard();
		}},
		{ id: 'term-sep1', divider: true },
		{ id: 'term-select-all', label: tr('workspace.ctxSelectAll'), action: () => manager.selectAll(paneId) },
		{ id: 'term-clear', label: tr('workspace.ctxClear'), action: () => {
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
		{ id: 'term-split-right', label: tr('workspace.ctxSplitRight'), action: () => {
			void splitPane(paneId, 'horizontal');
		}},
		{ id: 'term-split-down', label: tr('workspace.ctxSplitDown'), action: () => {
			void splitPane(paneId, 'vertical');
		}},
		{ id: 'term-sep3', divider: true },
		{ id: 'term-close', label: tr('workspace.ctxClosePanel'), action: () => {
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

// §multi-size: when remote control is on, the desktop and remote devices
// share ONE PTY size. A remote device that claims/refreshes can shrink this
// pane's grid; this button re-claims the PTY at THIS pane's size and forces a
// full repaint. Shown on the host while the remote server runs ($remoteRunning)
// AND on the desktop-in-browser controller (WEB_REMOTE) — there fitPaneNow's
// resize_pane tunnels over the LAN-WS shim, so it is exactly how a browser pane
// tells the host PTY its own dimensions.
function refreshForRemote() {
	if (!alive || !attached) return;
	// Select this pane's workspace in the sidebar/WorkspaceTree so the
	// active terminal and workspace tree stay in sync after a refresh.
	activeWorkspaceId.set(workspaceId);
	// §shared-remote: CLAIM the shared PTY at this viewer's size. On the
	// browser controller (sharedRemoteMode) passive fits no longer resize the
	// PTY, so this explicit claim is the only path that pushes this pane's
	// dimensions to the host; on the host it's an idempotent re-fit. The
	// broadcast Resize delta then re-letterboxes every viewer.
	manager.claimPaneSize(paneId);
	manager.forceFullRedraw(paneId);
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
let scrollbarDragGuardActive = false;

// Unconditionally clear the window-wide text-selection suppression. Reset to ''
// so any app-wide CSS rule keeps owning the property.
function clearBodySelectGuard(): void {
	document.body.style.userSelect = '';
	(document.body.style as CSSStyleDeclaration & { webkitUserSelect?: string }).webkitUserSelect = '';
}

// End a scrollbar-thumb drag from ANY source — the thumb's own pointerup, OR a
// window-level pointerup/cancel/blur. The latter is the safety net: the thumb
// lives under `{#if scrollbarVisible}`, so a resize that drops scrollback to 0
// (more frequent now that remote control re-fits on interaction) can unmount it
// mid-drag — its pointerup then never fires, and the body would stay
// `user-select:none` forever, disabling selection across the whole app.
function endScrollbarDrag(): void {
	if (scrollbarDragGuardActive) {
		window.removeEventListener('pointerup', endScrollbarDrag, true);
		window.removeEventListener('pointercancel', endScrollbarDrag, true);
		window.removeEventListener('blur', endScrollbarDrag);
		scrollbarDragGuardActive = false;
	}
	dragging = null;
	clearBodySelectGuard();
}

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
	// Safety net so the guard is ALWAYS lifted even if the thumb unmounts
	// mid-drag (its own pointerup never fires). Capture phase so we see the
	// release regardless of where it lands.
	if (!scrollbarDragGuardActive) {
		scrollbarDragGuardActive = true;
		window.addEventListener('pointerup', endScrollbarDrag, true);
		window.addEventListener('pointercancel', endScrollbarDrag, true);
		window.addEventListener('blur', endScrollbarDrag);
	}
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
	try { (e.currentTarget as HTMLElement).releasePointerCapture(e.pointerId); } catch { /* capture already gone */ }
	// Always restore — NOT gated on `dragging`, which an interleaved resize may
	// have already cleared, leaving the body stuck at user-select:none.
	endScrollbarDrag();
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

function onContainerPointerDown(e: PointerEvent) {
	activePaneId.set(paneId);
	// §TUI: refresh sticky timestamp when the user clicks back into
	// the pane (e.g., after closing a context menu or interacting with
	// chrome). Without this, the inline-TUI heuristic may have expired
	// during the time the pane was unfocused, and the next keystroke
	// would hit the host path instead of the TUI.
	touchTuiSticky();
	// Mouse routing is handled by Manager's pointerDownListener
	// (addEventListener on the container): TUI mouse reporting active
	// → encodeMouse → PTY; otherwise → host default (selection, links).
	// This handler only manages focus: the clicked pane's IME helper
	// must receive DOM focus so subsequent keystrokes reach THIS pane's
	// onContainerKeyDown, not the previously-focused pane's.
	// In 'direct' mode the IME helper isn't rendered at all (see below),
	// so focus the container directly — its keydown handler still
	// services every printable key without IME composition.
	if ($settingsStore.terminalImeMode === 'direct') {
		container?.focus();
		return;
	}
	if (imeHelper) {
		imeHelper.focus();
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

// Capture-phase keydown handler: prevent Backspace/Delete at the capture
// phase so WebView2 never enters back-navigation detection mode, which
// delays the initial key repeat on non-input elements (~1-2 s wait before
// holding Backspace starts deleting).
function captureBackspace(node: HTMLElement) {
	function onCapture(e: KeyboardEvent) {
		if (e.key === 'Backspace' || e.key === 'Delete') e.preventDefault();
	}
	node.addEventListener('keydown', onCapture, { capture: true });
	return { destroy() { node.removeEventListener('keydown', onCapture, { capture: true }); } };
}

</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div
	bind:this={container}
	class="rg-pane-container h-full w-full min-h-0 min-w-0 outline-none relative"
	class:bell-flash={bellFlash}
	style="background: var(--rg-term-bg); contain: strict;"
	role="application"
	aria-label={$t('workspace.terminalAriaLabel')}
	tabindex="-1"
	data-rg-pane-id={paneId}
	data-rg-pane-active={false}
	onwheel={onContainerWheel}
	oncontextmenu={onContextMenu}
	onmousedown={onContainerMouseDown}
	onpointerdown={onContainerPointerDown}
	onkeydown={onContainerKeyDown}
	use:captureBackspace
>
	<!-- 终端背景图层：absolute z-index:0，必须是容器的首个子节点，
	     才能稳定排在 wasm canvas（由 manager 后续 append）的 DOM 顺序之前、
	     渲染在其下方。勿在它前面插入其它元素，否则层叠会错乱。 -->
	{#if $activeBgImage.url}
		<div
			class="rg-pane-bgimg"
			style="background-image: url('{$activeBgImage.url}'); opacity: {$activeBgImage.opacity};"
			aria-hidden="true"
		></div>
	{/if}

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
			aria-label={$t('workspace.terminalInputAriaLabel')}
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
			title={$t('workspace.scrollToBottom')}
			onclick={jumpToBottom}
			aria-label={$t('workspace.scrollToBottom')}
		>
			<svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true">
				<path d="M3 5l5 5 5-5" stroke="currentColor" stroke-width="2" fill="none" stroke-linecap="round" stroke-linejoin="round"/>
				<path d="M3 10l5 5 5-5" stroke="currentColor" stroke-width="2" fill="none" stroke-linecap="round" stroke-linejoin="round" opacity="0.55"/>
			</svg>
		</button>
	{/if}

	<!-- §multi-size: re-claim PTY at this pane's size + repaint. Shown when this
	     desktop hosts a live LAN remote server ($remoteRunning) OR is serving the
	     public cloud remote ($cloudHostOnline) OR when this IS the desktop-in-
	     browser controller (WEB_REMOTE) — in all cases multiple viewers share one
	     PTY and a viewer must be able to lock it to its own size. A lone local
	     pane needs no button (fitPane already owns the size). On the browser
	     controller this is the only way to push the pane's dimensions to the
	     host PTY. -->
	{#if $remoteRunning || $cloudHostOnline || WEB_REMOTE}
		<button
			type="button"
			class="rg-remote-refresh"
			title={$t('workspace.refreshForRemote')}
			onclick={refreshForRemote}
			aria-label={$t('workspace.refreshForRemoteLabel')}
		>
			<svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true">
				<path d="M13.5 8a5.5 5.5 0 1 1-1.6-3.9" stroke="currentColor" stroke-width="1.6" fill="none" stroke-linecap="round"/>
				<path d="M13.5 2.4V5.1H10.8" stroke="currentColor" stroke-width="1.6" fill="none" stroke-linecap="round" stroke-linejoin="round"/>
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

<!-- §1.34 (2026-05-22) — shell-history popup moved to wasm canvas
     overlay; driver fns: openHistoryOverlay / closeHistoryOverlay /
     moveHistorySelection / commitHistorySelection live in <script>. -->

{#if termSearchOpen}
	<div class="rg-search-bar">
		<input
			bind:this={searchInputEl}
			class="rg-search-input"
			type="text"
			placeholder={$t('workspace.searchPlaceholder')}
			bind:value={searchQuery}
			oninput={refreshSearch}
			onkeydown={onSearchInputKey}
		/>
		<span class="rg-search-count">
			{#if searchQuery.length === 0}
				—
			{:else if searchInfo.count === 0}
				{$t('workspace.searchNoMatch')}
			{:else}
				{searchInfo.activeIndex + 1}/{searchInfo.count}
			{/if}
		</span>
		<button
			class="rg-search-btn"
			class:active={searchCaseSensitive}
			title={$t('workspace.searchCaseSensitive')}
			onclick={() => { searchCaseSensitive = !searchCaseSensitive; refreshSearch(); }}
		>Aa</button>
		<button
			class="rg-search-btn"
			title={$t('workspace.searchPrev')}
			onclick={() => { manager.searchPrev(paneId); searchInfo = manager.searchInfo(paneId); }}
		>↑</button>
		<button
			class="rg-search-btn"
			title={$t('workspace.searchNext')}
			onclick={() => { manager.searchNext(paneId); searchInfo = manager.searchInfo(paneId); }}
		>↓</button>
		<button
			class="rg-search-btn"
			title={$t('workspace.searchClose')}
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
	/* P4.4 (2026-05-21) — removed the `.rg-backend-switching` fade rule.
	 * With Rust path unconditional, there is no backend switch to mask. */

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
		 * Pixel rect 用一个「极窄、一个 cell 高」的盒子锚定在光标格上：
		 * 候选框只需要一个左上角锚点 + 行高，宽度无需贴合输入文字长度。
		 *
		 * §缺陷A (2026-06-18): 宽度固定为 1px（借鉴 remote 端 hidden-input
		 * 的 width:1px 做法）。以前 base 用一个 cell 宽、composition 期还被
		 * onCompositionUpdate 内联撑成 (charCount+1)*cellW，当光标在 col 0 时
		 * textarea `left:0` 贴分区左边界且被聚焦，WebView2/Chromium 会对
		 * overflow:hidden 的祖先隐式设 scrollLeft，把整屏内容整体左移（即
		 * 「最左侧输入顶偏」）。固定 1px 宽 + 不再随输入加宽，textarea 永远
		 * 不会被撑出可滚动内容，scrollLeft 恒为 0，顶偏消失。高度保留一个
		 * cell（var(--rg-ime-cell-h)）供 IME 候选框竖直锚定。 */
		position: absolute;
		left: 0;
		top: 0;
		width: 1px;
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
		 * by an overlay. The textarea itself stays invisible.
		 *
		 * §缺陷A: 宽度同样固定 1px——composition 期绝不加宽，避免 col 0
		 * 时触发祖先 scrollLeft 导致整屏左移。 */
		width: 1px;
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

	.rg-remote-refresh {
		/* §multi-size — floating "re-claim my size" button, top-right so it
		 * never collides with the bottom-right jump-to-bottom affordance.
		 * Only mounted while remote control is enabled. */
		position: absolute;
		right: 14px;
		top: 14px;
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
		opacity: 0.7;
		box-shadow: 0 4px 12px rgba(0, 0, 0, 0.3);
		transition: opacity 120ms ease-out, background 120ms ease-out, transform 120ms ease-out;
		z-index: 21;
	}
	.rg-remote-refresh:hover {
		opacity: 1;
		background: var(--rg-accent, #4a8cff);
		color: #fff;
		transform: translateY(-1px);
	}
	.rg-remote-refresh:focus {
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
	.rg-pane-bgimg {
		position: absolute;
		inset: 0;
		background-size: cover;
		background-position: center;
		background-repeat: no-repeat;
		pointer-events: none;
		z-index: 0;
	}
</style>
