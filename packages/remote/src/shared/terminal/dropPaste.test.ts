import { describe, it, expect } from 'vitest';
import { formatDroppedPathsForPaste } from './dropPaste';

describe('formatDroppedPathsForPaste', () => {
	it('单个路径原样返回（不加引号、不补空格）', () => {
		expect(formatDroppedPathsForPaste(['C:\\a\\img.png'])).toBe('C:\\a\\img.png');
	});
	it('含空格路径也不加引号', () => {
		expect(formatDroppedPathsForPaste(['C:\\my pics\\a.png'])).toBe('C:\\my pics\\a.png');
	});
	it('多个路径用空格连接', () => {
		expect(formatDroppedPathsForPaste(['a.png', 'b.jpg'])).toBe('a.png b.jpg');
	});
	it('trim 并丢弃空串', () => {
		expect(formatDroppedPathsForPaste([' a.png ', '', '  ', 'b.png'])).toBe('a.png b.png');
	});
	it('全空返回空串', () => {
		expect(formatDroppedPathsForPaste([])).toBe('');
	});
});
