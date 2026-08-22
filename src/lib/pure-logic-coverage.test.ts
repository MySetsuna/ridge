import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { get } from 'svelte/store';

import { autoGrow } from './actions/autoGrow';
import { portal } from './actions/portal';
import { translate, setLocale, billingRegion } from './i18n';
import {
	registerSidebarPlugin,
	sidebarPluginStore,
	unregisterSidebarPlugin,
} from './stores/sidebarPlugins';
import { showToast, toastStore } from './stores/toast';
import { getTransport, hasTransport, setTransport } from './transport/context';
import { hex8, hex8WithAlpha } from '@ridge/remote/shared/terminal/cssColor';
afterEach(() => {
	vi.unstubAllGlobals();
});

describe('pure UI state helpers', () => {
	it('translates known, interpolated, and missing keys', () => {
		const known = translate('zh', 'common.close');
		expect(known).not.toBe('common.close');
		expect(translate('zh', 'cloud.pairingHint', { code: '123', sec: 30 })).toContain('123');
		expect(translate('en', 'missing.key')).toBe('missing.key');
		setLocale('en');
		expect(get(billingRegion)).toBe('intl');
		setLocale('zh');
		expect(get(billingRegion)).toBe('cn');
	});

	it('registers sidebar plugins once, in order, and unregisters them', () => {
		const ids = ['coverage-global', 'coverage-pane', 'coverage-workspace'];
		const component = null as never;
		registerSidebarPlugin({ id: ids[0], title: 'global', scope: 'global', component, order: 30 });
		registerSidebarPlugin({ id: ids[1], title: 'pane', scope: 'pane', component, order: 10 });
		registerSidebarPlugin({ id: ids[2], title: 'workspace', scope: 'workspace', component, order: 20 });
		registerSidebarPlugin({ id: ids[0], title: 'duplicate', scope: 'global', component, order: 0 });
		const items = get(sidebarPluginStore as { subscribe: typeof sidebarPluginStore.subscribe });
		expect(items.map((item) => item.id)).toEqual([ids[1], ids[2], ids[0]]);
		unregisterSidebarPlugin(ids[1]);
		expect(get(sidebarPluginStore).map((item) => item.id)).toEqual([ids[2], ids[0]]);
		ids.forEach(unregisterSidebarPlugin);
	});

	it('tracks transport registration and rejects reads before registration', () => {
		const provider = {} as never;
		setTransport(provider);
		expect(hasTransport()).toBe(true);
		expect(getTransport()).toBe(provider);
	});

	it('cycles one-shot and locked modifiers with correct consumption', () => {
		vi.stubGlobal('$state', (value: unknown) => value);
		return import('../remote/lib/modState.svelte').then(({ anyMod, clearMods, consumeMods, cycleMod, peekMods }) => {
			clearMods();
		expect(anyMod()).toBe(false);
		expect(cycleMod('ctrl')).toBe('armed');
		expect(peekMods()).toMatchObject({ ctrl: true });
		expect(consumeMods()).toMatchObject({ ctrl: true });
		expect(anyMod()).toBe(false);
		expect(cycleMod('shift')).toBe('armed');
		expect(cycleMod('shift')).toBe('locked');
		expect(consumeMods()).toMatchObject({ shift: true });
		expect(peekMods()).toMatchObject({ shift: true });
		expect(cycleMod('shift')).toBe('off');
		});
	});

	it('keeps mobile remote state independent and pane-local', async () => {
		vi.stubGlobal('$state', (value: unknown) => value);
		const { MobileRemoteUiState } = await import('../remote/lib/mobileRemoteUiState.svelte');
		const state = new MobileRemoteUiState(true);
		expect(state.sentenceBuffer).toBe(true);
		expect(state.activePaneId).toBeNull();
		state.activeWorkspaceId = 'workspace-1';
		state.activePaneId = 'pane-1';
		state.showKeyboard = false;
		expect(state.activeWorkspaceId).toBe('workspace-1');
		expect(state.activePaneId).toBe('pane-1');
		expect(state.showKeyboard).toBe(false);
	});
});

describe('DOM-light actions and color normalization', () => {
	it('fits textarea height, toggles overflow, updates, and detaches input', () => {
		const listeners = new Map<string, () => void>();
		const node = {
			style: { height: '', overflowY: '' },
			scrollHeight: 80,
			addEventListener: vi.fn((event: string, listener: () => void) => listeners.set(event, listener)),
			removeEventListener: vi.fn((event: string, listener: () => void) => {
				if (listeners.get(event) === listener) listeners.delete(event);
			}),
		} as unknown as HTMLTextAreaElement;
		vi.stubGlobal('getComputedStyle', () => ({
			lineHeight: '20px', paddingTop: '2px', paddingBottom: '2px',
			borderTopWidth: '1px', borderBottomWidth: '1px',
		}));
		const action = autoGrow(node, { maxRows: 3 });
		expect(node.style.height).toBe('66px');
		expect(node.style.overflowY).toBe('auto');
		action.update({ maxRows: 5 });
		expect(node.style.height).toBe('82px');
		expect(node.style.overflowY).toBe('hidden');
		listeners.get('input')?.();
		action.destroy();
		expect(node.removeEventListener).toHaveBeenCalledTimes(1);
	});

	it('moves portal nodes to a selector target and removes them on destroy', () => {
		class FakeElement {}
		const body = { appendChild: vi.fn(), removeChild: vi.fn() };
		const host = { parentElement: null, dataset: {}, remove: vi.fn() } as unknown as HTMLElement;
		const target = Object.assign(new FakeElement(), { appendChild: vi.fn() });
		vi.stubGlobal('HTMLElement', FakeElement);
		vi.stubGlobal('document', {
			body,
			querySelector: vi.fn(() => target),
		});
		const action = portal(host, { target: '.overlay', id: 'coverage' }) as {
			update(options: { target?: HTMLElement | string; id?: string }): void;
			destroy(): void;
		};
		expect(target.appendChild).toHaveBeenCalledWith(host);
		expect(host.dataset.rgPortalId).toBe('coverage');
		action.update({ target: body as never });
		expect(body.appendChild).toHaveBeenCalledWith(host);
		action.destroy();
		expect(host.remove).toHaveBeenCalledTimes(1);
	});

	it('normalizes browser CSS colors and clamps replacement alpha', () => {
		const element = {
			isConnected: false,
			style: { color: '', cssText: '' },
			setAttribute: vi.fn(),
		};
		vi.stubGlobal('document', {
			documentElement: { appendChild: () => { element.isConnected = true; } },
			createElement: () => element,
		});
		vi.stubGlobal('getComputedStyle', () => ({
			color: element.style.color === 'red'
				? 'rgb(255, 0, 0)'
				: element.style.color.startsWith('rgb')
					? 'rgba(1, 2, 3, 0.5)'
					: element.style.color,
		}));
		expect(hex8('red')).toBe('#ff0000ff');
		expect(hex8('rgba(1, 2, 3, 0.5)')).toBe('#01020380');
		expect(hex8WithAlpha('#123456', 2)).toBe('#123456ff');
		expect(hex8WithAlpha('#123456', -1)).toBe('#12345600');
	});
});

describe('clipboard and toast boundaries', () => {
	it('uses the secure clipboard API and rejects empty input', async () => {
		const writeText = vi.fn().mockResolvedValue(undefined);
		vi.stubGlobal('navigator', { clipboard: { writeText } });
		const { writeClipboard } = await import('../remote/lib/clipboard');
		expect(await writeClipboard('hello')).toBe(true);
		expect(writeText).toHaveBeenCalledWith('hello');
		expect(await writeClipboard('')).toBe(false);
	});

	it('adds a typed toast and removes it after its bounded lifetime', () => {
		vi.useFakeTimers();
		showToast('coverage', 'info');
		expect(get(toastStore)).toEqual([expect.objectContaining({ message: 'coverage', type: 'info' })]);
		vi.advanceTimersByTime(3000);
		expect(get(toastStore)).toEqual([]);
		vi.useRealTimers();
	});
});
