import { writable } from 'svelte/store';

type ApplyUpdate = (reloadPage?: boolean) => Promise<void>;

export const pwaUpdateReady = writable(false);
let applyUpdate: ApplyUpdate | undefined;

export function configurePwaUpdate(updater: ApplyUpdate): void {
	applyUpdate = updater;
}

export function markPwaUpdateReady(): void {
	pwaUpdateReady.set(true);
}

/** Apply only after an explicit foreground action or a background transition. */
export async function applyPwaUpdate(): Promise<void> {
	if (!applyUpdate) return;
	pwaUpdateReady.set(false);
	await applyUpdate(true);
}
