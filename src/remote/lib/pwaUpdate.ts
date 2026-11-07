import { get, writable } from 'svelte/store';

type ApplyUpdate = (reloadPage?: boolean) => Promise<void>;

export const pwaUpdateReady = writable(false);
export const pwaUpdateBusy = writable(false);
export const pwaUpdateBlocked = writable(false);
export const pwaUpdateSaving = writable(false);
export const pwaUpdateDeferred = writable(false);
export const pwaUpdateError = writable('');
let applyUpdate: ApplyUpdate | undefined;
let generation = 0;
let updating = false;

interface EditGuard {
	dirty(): boolean;
	saving(): boolean;
	save(): Promise<void>;
	discard(): void;
}
const editors = new Map<symbol, EditGuard>();

export function pwaEditsBlocked(): boolean {
	return [...editors.values()].some(editor => editor.dirty() || editor.saving());
}

export function notifyPwaEditState(): void {
	pwaUpdateBlocked.set(pwaEditsBlocked());
	pwaUpdateSaving.set([...editors.values()].some(editor => editor.saving()));
}

export function registerPwaEditGuard(editor: EditGuard): () => void {
	const key = Symbol('editor');
	editors.set(key, editor);
	notifyPwaEditState();
	return () => { editors.delete(key); notifyPwaEditState(); };
}

export function configurePwaUpdate(updater: ApplyUpdate): void {
	applyUpdate = updater;
}

export function markPwaUpdateReady(): void {
	generation++;
	pwaUpdateReady.set(true);
	pwaUpdateDeferred.set(false);
	pwaUpdateError.set('');
}

export function deferPwaUpdate(): void {
	if (!updating) pwaUpdateDeferred.set(true);
}

export function resumePwaUpdatePrompt(): void {
	pwaUpdateDeferred.set(false);
}

/** Every foreground/background update shares the same live edit gate. */
export async function applyPwaUpdate(action: 'safe' | 'save' | 'discard' = 'safe'): Promise<boolean> {
	if (!applyUpdate || !get(pwaUpdateReady) || updating) return false;
	if (action === 'safe' && (get(pwaUpdateDeferred) || pwaEditsBlocked())) return false;
	const pending = [...editors.entries()];
	if (pending.some(([, editor]) => editor.saving())) return false;
	const version = generation;
	updating = true;
	pwaUpdateBusy.set(true);
	pwaUpdateError.set('');
	try {
		for (const [key, editor] of pending) {
			if (!editors.has(key) || !editor.dirty()) continue;
			if (action === 'save') await editor.save();
			if (action === 'discard') editor.discard();
		}
		notifyPwaEditState();
		if (pwaEditsBlocked()) return false;
		await applyUpdate(true);
		if (version === generation) pwaUpdateReady.set(false);
		return true;
	} catch (error) {
		pwaUpdateError.set(error instanceof Error ? error.message : String(error));
		return false;
	} finally {
		updating = false;
		pwaUpdateBusy.set(false);
		notifyPwaEditState();
	}
}
