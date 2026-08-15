/** DEC mouse bits from `mouseReportingModes`: ?1000=0x1, ?1002=0x2, ?1003=0x4. */
export function shouldForwardPointerMotion(modes: number, buttons: number): boolean {
	if ((modes & 0x7) === 0) return false;
	return (modes & 0x4) !== 0 || buttons !== 0;
}

/** SGR release uses the pressed button (`<0;x;ym`), not X10 button 3. */
export function sgrReleaseButton(button: number, lastButtons = 0): number {
	if (button === 0 || button === 1 || button === 2) return button;
	if (lastButtons & 1) return 0;
	if (lastButtons & 2) return 2;
	if (lastButtons & 4) return 1;
	return 0;
}
