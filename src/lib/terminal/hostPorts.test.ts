import { afterEach, describe, expect, it, vi } from 'vitest';

afterEach(() => vi.unstubAllGlobals());

describe('terminal host ports', () => {
	it('projects settings, font, theme, workspace, cwd, and link ports', async () => {
		vi.stubGlobal('localStorage', {
			getItem: () => null,
			setItem: vi.fn(),
			removeItem: vi.fn(),
		});
		const { makeHostPorts } = await import('./hostPorts');
		const ports = makeHostPorts();
		const settings = ports.settings?.get();
		expect(settings).toMatchObject({
			themeId: expect.any(String),
			terminalFontFamily: expect.any(String),
			defaultShell: expect.any(String),
		});
		expect(typeof ports.termSettings?.fontSize()).toBe('number');
		expect(typeof ports.workspace?.activeId()).toBe('string');
		expect(ports.cwd?.all()).toEqual(expect.any(Array));
	});
});
