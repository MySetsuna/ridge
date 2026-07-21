// §2 — latched modifier state shared between the on-screen quick-key bar
// (VirtualKeyboard) and the terminal input path (TerminalCanvas).
//
// §one-shot-vs-lock: tapping Ctrl/Alt/Shift cycles through three states so a
// modifier is NEVER left silently stuck (the reported "ctrl 一直保存选中状态"):
//   off → armed (ONE-SHOT) → locked (caps-lock) → off
//   • armed  = forms a chord with the NEXT key, then auto-releases.
//   • locked = stays armed across keys until tapped off (deliberate manual lock).
// The input paths call `consumeMods()` after forming a chord, which clears
// one-shot (armed) modifiers but PRESERVES locked ones. `peekMods()` reads
// without changing state; `anyMod()` reports if any modifier is currently active.

export interface Mods {
  ctrl: boolean;
  alt: boolean;
  shift: boolean;
}

export type ModKey = 'ctrl' | 'alt' | 'shift';

// Active (armed OR locked) modifiers — what the input path applies to a key.
// Only ever MUTATE its properties (never reassign) so the exported proxy stays
// reactive inside components (`stickyMods.ctrl`).
export const stickyMods = $state<Mods>({ ctrl: false, alt: false, shift: false });
// Locked (caps-lock) modifiers — a subset that survives `consumeMods()`.
export const lockedMods = $state<Mods>({ ctrl: false, alt: false, shift: false });

/** Cycle a modifier off → armed → locked → off. Returns the new state so the
 *  caller can raise the soft keyboard exactly when it first becomes armed. */
export function cycleMod(m: ModKey): 'off' | 'armed' | 'locked' {
  if (lockedMods[m]) {
    // locked → off
    lockedMods[m] = false;
    stickyMods[m] = false;
    return 'off';
  }
  if (stickyMods[m]) {
    // armed (one-shot) → locked
    lockedMods[m] = true;
    return 'locked';
  }
  // off → armed (one-shot)
  stickyMods[m] = true;
  return 'armed';
}

/** Read the current modifiers without changing them. */
export function peekMods(): Mods {
  return { ctrl: stickyMods.ctrl, alt: stickyMods.alt, shift: stickyMods.shift };
}

/** True if any modifier is currently active (armed or locked). */
export function anyMod(): boolean {
  return stickyMods.ctrl || stickyMods.alt || stickyMods.shift;
}

/** Read the current modifiers AND release the one-shot (armed, non-locked) ones,
 *  keeping any locked (caps-lock) modifiers armed. Called after a key forms a
 *  chord. */
export function consumeMods(): Mods {
  const m = peekMods();
  stickyMods.ctrl = lockedMods.ctrl;
  stickyMods.alt = lockedMods.alt;
  stickyMods.shift = lockedMods.shift;
  return m;
}

/** Hard reset — clears both armed and locked state. */
export function clearMods() {
  stickyMods.ctrl = false;
  stickyMods.alt = false;
  stickyMods.shift = false;
  lockedMods.ctrl = false;
  lockedMods.alt = false;
  lockedMods.shift = false;
}
