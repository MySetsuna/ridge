/** DEC mouse bits: ?1000=0x1, ?1002=0x2, ?1003=0x4, ?9=0x8. */
export function shouldForwardPointerMotion(modes: number, buttons: number): boolean {
	if ((modes & 0x4) !== 0) return true;
	return (modes & 0x2) !== 0 && buttons !== 0;
}

/** Preserve the physical button; the negotiated encoder maps legacy release to button 3. */
export function sgrReleaseButton(button: number, lastButtons = 0): number {
	if (button === 0 || button === 1 || button === 2) return button;
	if (lastButtons & 1) return 0;
	if (lastButtons & 2) return 2;
	if (lastButtons & 4) return 1;
	return 0;
}
