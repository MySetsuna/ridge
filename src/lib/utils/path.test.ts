import { describe, expect, it } from 'vitest';
import {
	isCurrentDirHref,
	commonPathAncestor,
	isExternalUrl,
	isHomeRelative,
	isPosixAbsolute,
	isWindowsAbsolute,
	joinPath,
	normalizePath,
	pathStartsWith,
	stripQuery,
} from './path';

describe('path helpers', () => {
	it('classifies external, absolute, home, and current-directory forms', () => {
		expect(isExternalUrl('https://example.com')).toBe(true);
		expect(isExternalUrl('mailto:a@example.com')).toBe(true);
		expect(isExternalUrl('ssh://example.com')).toBe(false);
		expect(isWindowsAbsolute('C:\\repo')).toBe(true);
		expect(isWindowsAbsolute('/repo')).toBe(false);
		expect(isPosixAbsolute('/repo')).toBe(true);
		expect(isPosixAbsolute('repo')).toBe(false);
		expect(isHomeRelative('~/repo')).toBe(true);
		expect(isHomeRelative('~\\repo')).toBe(true);
		expect(isHomeRelative('~')).toBe(true);
		expect(isCurrentDirHref('.\\')).toBe(true);
		expect(isCurrentDirHref('./file')).toBe(false);
	});

	it('joins and normalizes both separator styles', () => {
		expect(joinPath('C:\\repo\\', './src\\main.ts')).toBe('C:\\repo\\src\\main.ts');
		expect(joinPath('/repo/', './src\\main.ts')).toBe('/repo/src/main.ts');
		expect(normalizePath('C:\\repo\\src\\..\\main.ts')).toBe('C:\\repo\\main.ts');
		expect(normalizePath('/repo/./src/../main.ts')).toBe('/repo/main.ts');
		expect(normalizePath('../repo/../file')).toBe('../file');
	});

	it('strips cache queries and enforces path boundaries', () => {
		expect(stripQuery('file.ts?v=2')).toBe('file.ts');
		expect(stripQuery('file.ts')).toBe('file.ts');
	expect(pathStartsWith('C:\\Repo\\src\\main.ts', 'c:\\repo')).toBe(true);
		expect(pathStartsWith('/repo/src/main.ts', '/repo')).toBe(true);
		expect(pathStartsWith('/repository/file', '/repo')).toBe(false);
		expect(pathStartsWith('C:\\repo-old\\file', 'C:\\repo')).toBe(false);
	});

	it('finds a stable common workspace root independent of pane order', () => {
		expect(commonPathAncestor(['C:\\repo\\src', 'C:\\repo\\docs'])).toBe('C:\\repo');
		expect(commonPathAncestor(['/repo/src', '/repo/docs'])).toBe('/repo');
		expect(commonPathAncestor(['\\\\SERVER\\Share\\src', '\\\\server\\share\\docs'])).toBe('\\\\SERVER\\Share');
		expect(commonPathAncestor(['foo/src', 'foo/docs'])).toBe('foo');
		expect(commonPathAncestor(['foo', 'bar'])).toBeNull();
		expect(commonPathAncestor(['/Repo/src', '/repo/docs'])).toBe('/');
		expect(commonPathAncestor(['C:\\repo\\src', 'D:\\other'])).toBeNull();
		expect(commonPathAncestor(['C:\\repo\\src'])).toBe('C:\\repo\\src');
		expect(commonPathAncestor([])).toBeNull();
	});
});
