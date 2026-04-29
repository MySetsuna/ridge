// Shared svelte context between <RgSplit> and its child <RgPane> / <RgSplitter>.
// Children read `direction` from here so the consumer doesn't have to repeat it
// on every Pane.

export const RG_SPLIT_CTX = Symbol('rg-split-ctx');

export interface RgSplitContext {
  /** Reactive read-only — consumers should treat this as a getter. */
  readonly direction: 'horizontal' | 'vertical';
}
