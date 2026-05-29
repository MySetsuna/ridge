// §2 — sticky modifier state shared between the on-screen quick-key bar
// (VirtualKeyboard) and the terminal input path (TerminalCanvas).
//
// Tapping Ctrl/Alt/Shift on the quick-key bar sets a *sticky* modifier and
// raises the soft keyboard; the NEXT keystroke — including soft-keyboard text
// that arrives via `beforeinput` (insertText) — consumes the sticky modifiers
// to form a chord (e.g. tap Ctrl, then type `c` → Ctrl+C), after which they
// clear. Plain quick-keys (Esc/Tab/arrows/…) do not raise/close the keyboard.

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
