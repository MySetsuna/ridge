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

import { clearPathProbeCache } from '@ridge/remote/shared/terminal/linkOpenHost';
import { classifyLink, openTerminalLink, resolveLink } from './linkResolver';

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

describe('openTerminalLink', () => {
	const request = {
		type: 'path' as const,
		path: 'C:\\repo\\src\\main.ts',
		line: 12,
		col: 3,
		origin: { kind: 'local' as const, workspaceId: 'ws-a', paneId: 'pane-a' },
	};

	it('opens an origin-proven file in Ridge and preserves line/column', async () => {
		clearPathProbeCache();
		const openFile = vi.fn(async () => {});
		await expect(openTerminalLink(request, {
			inspectPath: vi.fn(async () => ({ exists: true, isDirectory: false })),
			openFile,
		})).resolves.toEqual({ handled: true });
		expect(openFile).toHaveBeenCalledWith(request.path, 12, 3);
	});

	it('routes a local directory to Explorer and never opens it as a file', async () => {
		clearPathProbeCache();
		const revealDirectory = vi.fn(async () => true);
		const openFile = vi.fn(async () => {});
		await openTerminalLink({ ...request, path: 'C:\\repo\\src', line: undefined, col: undefined }, {
			inspectPath: vi.fn(async () => ({ exists: true, isDirectory: true })),
			revealDirectory,
			openFile,
		});
		expect(revealDirectory).toHaveBeenCalledWith('C:\\repo\\src', 'ws-a');
		expect(openFile).not.toHaveBeenCalled();
	});

	it('keeps missing paths inert with an explicit error', async () => {
		clearPathProbeCache();
		const notify = vi.fn();
		const openFile = vi.fn(async () => {});
		await expect(openTerminalLink(request, {
			inspectPath: vi.fn(async () => ({ exists: false })),
			notify,
			openFile,
		})).resolves.toEqual({ handled: true, reason: 'missing_path' });
		expect(notify).toHaveBeenCalledWith(expect.stringContaining('路径不存在'), 'error');
		expect(openFile).not.toHaveBeenCalled();
	});

	it('never sends a foreign path to local editor or Explorer', async () => {
		clearPathProbeCache();
		const openFile = vi.fn(async () => {});
		const revealDirectory = vi.fn(async () => true);
		const notify = vi.fn();
		await expect(openTerminalLink({
			...request,
			origin: { kind: 'remote', hostId: 'lan:a', workspaceId: 'remote-ws', paneId: 'remote-pane' },
		}, {
			inspectPath: vi.fn(async () => ({ exists: true, isDirectory: false })),
			openFile,
			revealDirectory,
			notify,
		})).resolves.toEqual({ handled: true, reason: 'foreign_file_fallback' });
		expect(openFile).not.toHaveBeenCalled();
		expect(revealDirectory).not.toHaveBeenCalled();
		expect(notify).toHaveBeenCalled();
	});

	it('opens only http/https URLs through the browser path', async () => {
		const openUrl = vi.fn(async () => {});
		await openTerminalLink({
			type: 'url',
			href: 'https://example.test',
			origin: request.origin,
		}, { openUrl });
		expect(openUrl).toHaveBeenCalledWith('https://example.test', undefined);
		await expect(openTerminalLink({
			type: 'url',
			href: 'javascript:alert(1)',
			origin: request.origin,
		}, { openUrl })).resolves.toEqual({ handled: false, reason: 'unsafe_url' });
	});
});
