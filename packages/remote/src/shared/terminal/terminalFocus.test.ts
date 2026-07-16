import { afterEach, describe, expect, it, vi } from 'vitest';
import { focusActiveTerminal, ownsTabKey } from './terminalFocus';

/**
 * `ownsTabKey` decides whether a Tab keystroke should keep native browser
 * behavior (real text fields / editor) or be redirected to the active
 * terminal. `focusActiveTerminal` performs that redirect. Both are pure DOM
 * utilities, exercised here with lightweight fakes so the suite runs in the
 * repo's `node` vitest environment without jsdom.
 */

/** Minimal Element-shaped fake — only the fields `ownsTabKey` reads. */
function fakeEl(opts: {
	tagName: string;
	isContentEditable?: boolean;
	closestMatch?: boolean;
}): Element {
	return {
		tagName: opts.tagName,
		isContentEditable: opts.isContentEditable ?? false,
		closest: () => (opts.closestMatch ? ({} as Element) : null),
	} as unknown as Element;
}

describe('ownsTabKey', () => {
	it('returns false for a null target', () => {
		expect(ownsTabKey(null)).toBe(false);
	});

	it.each(['INPUT', 'TEXTAREA', 'SELECT'])('claims Tab for <%s>', (tag) => {
		expect(ownsTabKey(fakeEl({ tagName: tag }))).toBe(true);
	});

	it('claims Tab for a contenteditable element', () => {
		expect(ownsTabKey(fakeEl({ tagName: 'DIV', isContentEditable: true }))).toBe(true);
	});

	it('claims Tab inside an editor surface matched by selector', () => {
		expect(ownsTabKey(fakeEl({ tagName: 'DIV', closestMatch: true }))).toBe(true);
	});

	it('does NOT claim Tab for chrome buttons', () => {
		expect(ownsTabKey(fakeEl({ tagName: 'BUTTON' }))).toBe(false);
	});

	it('does NOT claim Tab for a plain container div', () => {
		expect(ownsTabKey(fakeEl({ tagName: 'DIV' }))).toBe(false);
	});
});

describe('focusActiveTerminal', () => {
	const realDoc = (globalThis as { document?: unknown }).document;
	afterEach(() => {
		(globalThis as { document?: unknown }).document = realDoc;
	});

	/** Install a fake `document` whose active pane optionally holds an IME textarea. */
	function installDoc(pane: { focus: () => void; querySelector?: () => unknown } | null, ime: unknown) {
		if (pane) pane.querySelector = () => ime;
		(globalThis as { document?: unknown }).document = {
			querySelector: (sel: string) =>
				sel === '[data-rg-pane-active="true"]' || sel === '[data-rg-pane-id]' ? pane : null,
		};
	}

	it('returns false when there is no document (non-DOM env)', () => {
		(globalThis as { document?: unknown }).document = undefined;
		expect(focusActiveTerminal()).toBe(false);
	});

	it('focuses the IME helper textarea when present', () => {
		const ime = { focus: vi.fn() };
		const pane = { focus: vi.fn() };
		installDoc(pane, ime);

		expect(focusActiveTerminal()).toBe(true);
		expect(ime.focus).toHaveBeenCalledTimes(1);
		expect(pane.focus).not.toHaveBeenCalled();
	});

	it('falls back to the pane container in direct mode (no textarea)', () => {
		const pane = { focus: vi.fn() };
		installDoc(pane, null);

		expect(focusActiveTerminal()).toBe(true);
		expect(pane.focus).toHaveBeenCalledTimes(1);
	});

	it('returns false when no terminal pane exists', () => {
		installDoc(null, null);
		expect(focusActiveTerminal()).toBe(false);
	});
});
