import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

const managerSource = readFileSync(
	new URL('../../../packages/remote/src/shared/terminal/manager.ts', import.meta.url),
	'utf8',
);
const desktopSource = readFileSync(new URL('../../routes/+page.svelte', import.meta.url), 'utf8');
const mobileSource = readFileSync(new URL('../../remote/MainApp.svelte', import.meta.url), 'utf8');
const paneSource = readFileSync(new URL('../components/RidgePane.svelte', import.meta.url), 'utf8');

describe('terminal renderer bootstrap contracts', () => {
	it('claims the shared host when pane mount wins the desktop action race', () => {
		expect(desktopSource).toContain('data-rg-host');
		expect(managerSource).toContain('private async _ensureDomHostStarted()');
		expect(managerSource).toContain('await this._ensureDomHostStarted();');
		expect(managerSource).toContain('this.attachHost(hostCanvas)');
	});

	it('keeps mobile host canvas on the same WebGPU-first path', () => {
		expect(mobileSource).toContain('<canvas class="host-canvas" data-rg-host');
	});

	it('does not paint a second native textarea caret over the wasm cursor', () => {
		expect(paneSource).toContain('caret-color: transparent;');
		expect(paneSource).not.toContain('caret-color: var(--rg-accent, currentColor);');
	});

	it('defers shared-host invalidation until parked panes finish restoring', () => {
		expect(managerSource).toContain('private _hostInvalidateSuspendDepth = 0;');
		expect(managerSource).toContain('private _deferredHostInvalidate = false;');
		expect(managerSource).toContain('const restore = shouldRestore');
		expect(managerSource).toContain('void restore.then(paint, paint);');
		// Memory restores are serialized now; one tab switch must not fan out
		// concurrent WebGPU unpark/device work across parked panes.
		expect(managerSource).toContain(
			'this._memoryRestoreQueue = queued.then(() => undefined, () => undefined);',
		);
	});

	it('keeps link hover affordance continuous and thin', () => {
		expect(managerSource).toContain("'height:1px'");
		expect(managerSource).toContain('regionsForSpan(ent.kernel, span)');
		expect(managerSource).toContain('createLinkHintOverlay');
		expect(managerSource).toContain('按 Ctrl 可跳转');
	});
});
