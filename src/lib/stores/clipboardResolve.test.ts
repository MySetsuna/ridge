import { describe, it, expect } from 'vitest';
import { resolveActiveClipboard } from './fileExplorer';

const internalCopy = { paths: ['C:\\ridge\\old.txt'], mode: 'copy' as const, seq: 100 };
const internalCut = { paths: ['C:\\ridge\\moved.txt'], mode: 'cut' as const, seq: 100 };

describe('resolveActiveClipboard', () => {
	it('序列号一致 → 用内部复制', () => {
		expect(resolveActiveClipboard(internalCopy, 100, ['C:\\ext\\new.txt'])).toBe(internalCopy);
	});
	it('序列号一致 → 用内部剪切', () => {
		expect(resolveActiveClipboard(internalCut, 100, [])).toBe(internalCut);
	});
	it('序列号变了 + 系统有文件 → 用系统(复制)', () => {
		expect(resolveActiveClipboard(internalCopy, 101, ['C:\\ext\\new.txt'])).toEqual({
			paths: ['C:\\ext\\new.txt'],
			mode: 'copy',
			seq: 101,
		});
	});
	it('序列号变了 + 系统为空 → 退回内部兜底', () => {
		expect(resolveActiveClipboard(internalCut, 101, [])).toBe(internalCut);
	});
	it('无内部 + 系统有文件 → 用系统', () => {
		expect(resolveActiveClipboard(null, 5, [' C:\\a.txt ', ''])).toEqual({
			paths: ['C:\\a.txt'],
			mode: 'copy',
			seq: 5,
		});
	});
	it('无内部 + 系统为空 → null', () => {
		expect(resolveActiveClipboard(null, 5, [])).toBeNull();
	});
});
