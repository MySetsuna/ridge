import { afterEach, describe, expect, it, vi } from 'vitest';

import { writeClipboard } from './clipboard';

afterEach(() => vi.unstubAllGlobals());

describe('remote clipboard fallback', () => {
	it('returns false for empty input and uses the secure clipboard API', async () => {
		const writeText = vi.fn().mockResolvedValue(undefined);
		vi.stubGlobal('navigator', { clipboard: { writeText } });
		expect(await writeClipboard('')).toBe(false);
		expect(await writeClipboard('hello')).toBe(true);
		expect(writeText).toHaveBeenCalledWith('hello');
	});

	it('uses an editable off-screen textarea when secure clipboard fails', async () => {
		const textarea = {
			value: '',
			style: {} as Record<string, string>,
			contentEditable: 'false',
			readOnly: true,
			focus: vi.fn(),
			select: vi.fn(),
			setSelectionRange: vi.fn(),
			remove: vi.fn(),
		};
		const body = { appendChild: vi.fn() };
		const range = { selectNodeContents: vi.fn() };
		const selection = { removeAllRanges: vi.fn(), addRange: vi.fn() };
		vi.stubGlobal('navigator', { clipboard: { writeText: vi.fn().mockRejectedValue(new Error('insecure')) } });
		vi.stubGlobal('document', {
			createElement: vi.fn(() => textarea),
			body,
			createRange: vi.fn(() => range),
			execCommand: vi.fn(() => true),
		});
		vi.stubGlobal('window', { getSelection: vi.fn(() => selection) });

		expect(await writeClipboard('fallback')).toBe(true);
		expect(textarea.value).toBe('fallback');
		expect(textarea.contentEditable).toBe('true');
		expect(textarea.readOnly).toBe(false);
		expect(body.appendChild).toHaveBeenCalledWith(textarea);
		expect(textarea.remove).toHaveBeenCalled();
	});

	it('fails closed when no clipboard or DOM fallback exists', async () => {
		vi.stubGlobal('navigator', {});
		vi.stubGlobal('document', undefined);
		expect(await writeClipboard('hello')).toBe(false);
	});
});
