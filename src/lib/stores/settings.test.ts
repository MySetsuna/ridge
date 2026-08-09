import { beforeEach, describe, expect, it, vi } from 'vitest';
import { get } from 'svelte/store';

const invokeMock = vi.hoisted(() => vi.fn().mockResolvedValue(undefined));
const getThemeMock = vi.hoisted(() => vi.fn());
const setActiveBgImageMock = vi.hoisted(() => vi.fn());

vi.mock('@tauri-apps/api/core', () => ({ invoke: invokeMock }));
vi.mock('./themes', () => ({ getTheme: getThemeMock, setActiveBgImage: setActiveBgImageMock }));

function installLocalStorage(raw: string | null = null) {
	const data = new Map<string, string>();
	if (raw !== null) data.set('ridge-settings', raw);
	vi.stubGlobal('localStorage', {
		getItem: vi.fn((key: string) => data.get(key) ?? null),
		setItem: vi.fn((key: string, value: string) => data.set(key, value)),
	});
	return data;
}

async function loadSettings(raw: string | null = null) {
	vi.resetModules();
	installLocalStorage(raw);
	return import('./settings');
}

beforeEach(() => {
	vi.clearAllMocks();
	invokeMock.mockResolvedValue(undefined);
	getThemeMock.mockReturnValue({ colors: { background: '#111', foreground: '#eee' } });
	setActiveBgImageMock.mockResolvedValue(undefined);
	vi.unstubAllGlobals();
});

describe('settings persistence and theme boundary', () => {
	it('loads safe defaults without local storage', async () => {
		const settings = await loadSettings();
		expect(get(settings.settingsStore)).toMatchObject({
			theme: 'endless-dark', terminalImeMode: 'ime', remoteEnabled: false,
			teammateEnabled: true, teammateHitlEnabled: false,
		});
	});

	it('rejects malformed and out-of-range persisted values', async () => {
		const settings = await loadSettings(JSON.stringify({
			theme: '', editorFontSize: 99, terminalPaddingPx: -1,
			terminalScrollbackLines: 1, terminalImeMode: 'bad', remoteEnabled: 'yes',
		}));
		expect(get(settings.settingsStore)).toMatchObject({
			theme: 'endless-dark', editorFontSize: 14, terminalPaddingPx: 0,
			terminalScrollbackLines: 2000, terminalImeMode: 'ime', remoteEnabled: false,
		});
	});

	it('persists setting updates and applies CSS theme variables', async () => {
		const data = await loadSettings();
		const style = { setProperty: vi.fn() };
		vi.stubGlobal('document', { documentElement: { style }, cookie: '' });
		data.setSetting('terminalPaddingPx', 12);
		data.applyTheme('light');
		expect(get(data.settingsStore).terminalPaddingPx).toBe(12);
		expect(style.setProperty).toHaveBeenCalledWith('--rg-background', '#111');
		expect(setActiveBgImageMock).toHaveBeenCalledWith('light');
	});

	it('keeps unknown themes harmless and forwards native theme persistence', async () => {
		const settings = await loadSettings();
		getThemeMock.mockReturnValueOnce(null);
		settings.applyTheme('missing');
		settings.setTheme('light');
		expect(invokeMock).toHaveBeenCalledWith('set_active_theme', { themeId: 'light' });
	});

	it('restores the remote server only when the persisted flag is enabled', async () => {
		const settings = await loadSettings(JSON.stringify({ remoteEnabled: true }));
		vi.stubGlobal('document', { documentElement: { style: { setProperty: vi.fn() } }, cookie: '' });
		settings.initSettingsBoot();
		expect(invokeMock).toHaveBeenCalledWith('set_remote_enabled', { enabled: true });
	});
});
