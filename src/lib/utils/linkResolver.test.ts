import { describe, expect, it, vi } from 'vitest';

vi.mock('@tauri-apps/api/core', () => ({
	isTauri: vi.fn(() => false),
	invoke: vi.fn(),
}));
vi.mock('$lib/stores/fileEditor', () => ({
	fileEditorStore: { openFile: vi.fn() },
}));
vi.mock('$lib/components/RidgeDialog.svelte', () => ({
	choiceDialog: vi.fn(),
}));

import { classifyLink, resolveLink } from './linkResolver';

describe('link resolver classification', () => {
	it('classifies external, document, file, and unsafe forms', () => {
		expect(classifyLink('https://example.com')).toBe('http');
		expect(classifyLink('ftp://example.com/a')).toBe('http');
		expect(classifyLink('mailto:a@example.com')).toBe('mailto');
		expect(classifyLink('file:///C:/repo/readme.md')).toBe('file-url');
		expect(classifyLink('#section')).toBe('fragment');
		expect(classifyLink('C:\\repo\\readme.md')).toBe('absolute');
		expect(classifyLink('./readme.md')).toBe('relative');
		expect(classifyLink('readme.md')).toBe('relative');
		expect(classifyLink('')).toBe('unknown');
	});
});

describe('resolveLink', () => {
	it('fails closed for empty and javascript hrefs', () => {
		expect(resolveLink('', {})).toEqual({ kind: 'noop', reason: 'empty href' });
		expect(resolveLink('javascript:alert(1)', {})).toEqual({
			kind: 'noop',
			reason: 'javascript: ignored',
		});
	});

	it('keeps fragments and trusted-context metadata intact', () => {
		expect(resolveLink('#logs', { basePath: '/repo/docs' })).toEqual({ kind: 'fragment', id: 'logs' });
		expect(resolveLink(' https://example.com/a ', { basePath: '/repo/docs' })).toEqual({
			kind: 'open-url',
			href: 'https://example.com/a',
			trustBase: '/repo/docs',
		});
	});

	it('resolves owned files, external files, and current directories deterministically', () => {
		expect(resolveLink('./src/main.ts?raw#L4', {
			basePath: 'C:\\repo',
			knownCwds: ['C:\\repo'],
		})).toEqual({ kind: 'open-file', path: 'C:\\repo\\src\\main.ts' });
		expect(resolveLink('../shared', { basePath: 'C:\\repo\\docs' })).toEqual({
			kind: 'reveal',
			path: 'C:\\repo\\docs\\..\\shared',
		});
		expect(resolveLink('file:///C:/outside/log.txt', { cwd: 'C:\\repo' })).toEqual({
			kind: 'reveal',
			path: 'C:/outside/log.txt',
		});
		expect(resolveLink('.', {})).toEqual({ kind: 'noop', reason: 'no basePath for "."' });
		expect(resolveLink('.', { basePath: '/repo' })).toEqual({ kind: 'reveal', path: '/repo' });
	});

	it('rejects relative and unknown links when no context exists', () => {
		expect(resolveLink('./missing.md', {})).toEqual({ kind: 'noop', reason: 'relative without base' });
		expect(resolveLink('opaque-token', {})).toEqual({ kind: 'noop', reason: 'unknown without base' });
	});
});
