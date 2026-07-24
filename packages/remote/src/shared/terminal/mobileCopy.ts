/**
 * Mobile / touch copy helper (V-MOB-CP).
 * Copy must write clipboard + clear selection only — never focus a hidden
 * textarea and never invoke paste.
 */

export type CopySideEffects = {
  writeText: (text: string) => void | Promise<void>;
  clearSelection: () => void;
  focusInput?: () => void;
  paste?: (text: string) => void;
};

/**
 * Perform a selection copy. Returns true if text was non-empty and write ran.
 * Guarantees: does not call focusInput or paste.
 */
export function copySelectionOnly(
  text: string,
  fx: CopySideEffects,
): boolean {
  const t = text ?? '';
  if (!t) return false;
  void fx.writeText(t);
  fx.clearSelection();
  // Explicit non-calls documented for reviewers / tests:
  // fx.focusInput?.() — NEVER
  // fx.paste?.(t) — NEVER
  return true;
}
