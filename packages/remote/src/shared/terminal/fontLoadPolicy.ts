/** Desktop renderers need host-resolved font bytes; browser Remote renders with
 * its own browser font stack and must never issue desktop font RPCs. */
export function shouldLoadHostFontData(isRemoteBuild: boolean): boolean {
	return !isRemoteBuild;
}
