// §2 — latched modifier state shared between the on-screen quick-key bar
// (VirtualKeyboard) and the terminal input path (TerminalCanvas).
//
// Tapping Ctrl/Alt/Shift on the quick-key bar LATCHES that modifier (and raises
// the soft keyboard). It stays armed across keystrokes — every following key
// forms a chord with it (tap Ctrl → Ctrl+C, Ctrl+V, …) — until the user taps
// the same modifier again to release it (caps-lock style). The input path reads
// via `peekMods()` and never auto-clears. Plain quick-keys (Esc/Tab/arrows/…)
// do not raise/close the keyboard. `consumeMods`/`clearMods` remain available
// for explicit one-shot/reset use.

export interface Mods {
  ctrl: boolean;
  alt: boolean;
  shift: boolean;
}

// A single shared reactive object. We only ever MUTATE its properties (never
// reassign the binding), so exporting the proxy directly is safe and keeps
// `stickyMods.ctrl` reactive inside components.
export const stickyMods = $state<Mods>({ ctrl: false, alt: false, shift: false });

export function toggleMod(m: 'ctrl' | 'alt' | 'shift') {
  stickyMods[m] = !stickyMods[m];
}

/** Read the current modifiers without clearing them. */
export function peekMods(): Mods {
  return { ctrl: stickyMods.ctrl, alt: stickyMods.alt, shift: stickyMods.shift };
}

/** True if any sticky modifier is currently armed. */
export function anyMod(): boolean {
  return stickyMods.ctrl || stickyMods.alt || stickyMods.shift;
}

/** Read the current modifiers AND clear them (used when forming a chord). */
export function consumeMods(): Mods {
  const m = peekMods();
  clearMods();
  return m;
}

export function clearMods() {
  stickyMods.ctrl = false;
  stickyMods.alt = false;
  stickyMods.shift = false;
}
