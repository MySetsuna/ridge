/**
 * Tab-focus containment for the desktop shell.
 *
 * The terminal's real keyboard target is each pane's hidden
 * `.rg-ime-helper` textarea (IME mode) or, in `direct` mode, the pane
 * container itself (`tabindex="-1"`). Desktop chrome — workspace tabs,
 * file-tree rows, toolbar buttons — is natively focusable, so a bare Tab
 * fired while focus sits on chrome (or `<body>`) walks the chrome focus
 * ring instead of reaching the shell, visually "selecting" elements the
 * user never meant to touch.
 *
 * These helpers let the global keydown handler snap Tab back to the active
 * terminal while leaving genuine text-entry surfaces (search boxes, inline
 * rename, the Monaco editor) on their native Tab behavior. They are pure
 * DOM utilities — no framework state — so they stay trivially testable.
 */

/** Form-control tags whose Tab key must keep native browser behavior. */
const EDITABLE_TAGS: ReadonlySet<string> = new Set(['INPUT', 'TEXTAREA', 'SELECT']);

/**
 * CSS selector for editor surfaces that own their Tab key for indentation,
 * plus an explicit opt-out hook (`data-allow-tab`) any component can set.
 * Monaco's focus sink is a `<textarea>` already covered by EDITABLE_TAGS;
 * the container selector is defensive belt-and-suspenders.
 */
const TAB_OWNING_SELECTOR = '.monaco-editor, [data-allow-tab]';

/**
 * True when `el` is a genuine text-entry / editor surface that legitimately
 * consumes Tab. Used to *exclude* such targets from the terminal redirect.
 *
 * Note: the terminal IME helper is itself a `<textarea>` and therefore
 * matches here — but by the time the window-level handler runs, a focused
 * pane has already `preventDefault`-ed Tab (encoded to the shell), so the
 * redirect path is gated on `!event.defaultPrevented` and never reaches a
 * focused terminal regardless.
 */
export function ownsTabKey(el: Element | null): boolean {
	if (!el) return false;
	if (EDITABLE_TAGS.has(el.tagName)) return true;
	// Duck-typed on purpose: `isContentEditable` lives on HTMLElement, but an
	// `instanceof HTMLElement` guard throws in non-DOM (node) test envs where
	// the constructor is undefined. Reading the property is safe everywhere.
	if ((el as HTMLElement).isContentEditable === true) return true;
	return el.closest(TAB_OWNING_SELECTOR) !== null;
}

/**
 * Move focus to the currently active terminal pane so subsequent keystrokes
 * (including Tab → shell completion) reach the PTY. Prefers the active
 * pane's IME helper textarea; falls back to the focusable container in
 * `direct` mode, and to any present pane when none is marked active.
 *
 * @returns `true` if a terminal pane was found and focused; `false`
 *          otherwise, signalling the caller to leave the key event alone.
 */
export function focusActiveTerminal(): boolean {
	if (typeof document === 'undefined') return false;

	const pane =
		document.querySelector<HTMLElement>('[data-rg-pane-active="true"]') ??
		document.querySelector<HTMLElement>('[data-rg-pane-id]');
	if (!pane) return false;

	const ime = pane.querySelector<HTMLTextAreaElement>('textarea.rg-ime-helper');
	(ime ?? pane).focus();
	return true;
}
