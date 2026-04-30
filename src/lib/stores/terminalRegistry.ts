// src/lib/stores/terminalRegistry.ts
//
// Terminal parking lot: keeps xterm Terminal instances (and their WebGL
// contexts) alive across Svelte component destroy/recreate cycles that
// happen during pane splits. Without this, splitting a pane destroys the
// existing Pane component, tears down the WebGL context, and recreates
// everything — which is flaky on WebView2 and visually resets the terminal.
//
// Lifecycle:
//   1. renderView() creates a terminal → registerTerminal(paneId, entry)
//   2. Pane component destroyed by split → parkTerminal(paneId)
//        .xterm element moved to off-screen div, WebGL context stays alive
//   3. New Pane component mounts with same paneId → restoreTerminal(paneId, viewInner)
//        .xterm element moved back into the new container + fitAddon.fit()
//   4. User explicitly closes a pane → disposeTerminal(paneId)
//        Full teardown + registry removal

import type { Terminal } from 'xterm';
import type { FitAddon } from 'xterm-addon-fit';
import type { WebglAddon } from 'xterm-addon-webgl';
import type { SerializeAddon } from '@xterm/addon-serialize';

export interface TerminalEntry {
  term: Terminal;
  fitAddon: FitAddon;
  webglAddon: WebglAddon | null;
  /** Optional SerializeAddon attached at register-time. Used by
   *  `serializeTerminalState` to capture the visible buffer + scrollback
   *  on park/save so a future restore (e.g. workspace reopen) can replay
   *  the terminal's last state. */
  serializeAddon: SerializeAddon | null;
}

const registry = new Map<string, TerminalEntry>();

function getParkingLot(): HTMLDivElement {
  const id = '__rg-terminal-parking-lot__';
  let el = document.getElementById(id) as HTMLDivElement | null;
  if (!el) {
    el = document.createElement('div');
    el.id = id;
    // Off-screen but still in the document so WebGL contexts remain valid.
    el.style.cssText =
      'position:fixed;left:-9999px;top:-9999px;width:1px;height:1px;overflow:hidden;pointer-events:none;opacity:0;z-index:-1';
    document.body.appendChild(el);
  }
  return el;
}

export function registerTerminal(paneId: string, entry: TerminalEntry): void {
  registry.set(paneId, entry);
}

export function getRegisteredTerminal(paneId: string): TerminalEntry | undefined {
  return registry.get(paneId);
}

/** Move the terminal's DOM element to the off-screen parking lot.
 *  The xterm instance and WebGL context remain alive in the document. */
export function parkTerminal(paneId: string): void {
  const entry = registry.get(paneId);
  if (!entry || !entry.term.element) return;
  try {
    getParkingLot().appendChild(entry.term.element);
  } catch {
    // Element already detached or moved — leave it.
  }
}

/** Capture the current visible buffer + up to 5000 lines of scrollback as
 *  an ANSI-encoded string. Returns null if the pane isn't registered or
 *  has no SerializeAddon attached.
 *
 *  Use this for workspace save (write to .ridge) — the returned string can
 *  be `term.write(...)`-ed back into a fresh terminal to reproduce the
 *  scrollback. The caller is responsible for storage durability and any
 *  filesystem permissioning required for sensitive output. */
export function serializeTerminalState(paneId: string): string | null {
  const entry = registry.get(paneId);
  if (!entry || !entry.serializeAddon) return null;
  try {
    return entry.serializeAddon.serialize({ scrollback: 5000 });
  } catch {
    return null;
  }
}

/** Snapshot every registered pane's terminal state into a `paneId →
 *  serialized` map. Skips panes whose serialize call returns null. Used by
 *  `saveWorkspaceToFile` to populate `RidgeFile.serialized_panes`. */
export function serializeAllTerminalStates(): Record<string, string> {
  const out: Record<string, string> = {};
  for (const [paneId] of registry) {
    const s = serializeTerminalState(paneId);
    if (s) out[paneId] = s;
  }
  return out;
}

/** Move the terminal's DOM element from parking lot back into `container`.
 *  Returns true if successfully restored.
 *
 *  Multi-frame fit: split-restore drops the canvas back into a container
 *  whose dimensions are still settling. A single fit() at append time often
 *  reads stale 0×0 dimensions and produces a black row across the top.
 *
 *  Strategy:
 *  1. fit + clearTextureAtlas + refresh immediately (may run against stale
 *     dimensions; cheap to retry).
 *  2. Poll up to 3 rAF ticks for `rect.width>0 && rect.height>0`. As soon
 *     as the container has measured itself, re-run the trio and stop.
 *
 *  3 frames is enough to cover splitpanes' two-phase layout settling (one
 *  frame for the new pane to mount, one for the parent to redistribute,
 *  one for slack); going beyond rarely helps and can mask real bugs. */
export function restoreTerminal(paneId: string, container: HTMLElement): boolean {
  const entry = registry.get(paneId);
  if (!entry || !entry.term.element) return false;
  try {
    container.appendChild(entry.term.element);
    // Immediate three-set: best-effort, may run against stale dimensions.
    entry.fitAddon.fit();
    entry.webglAddon?.clearTextureAtlas();
    entry.term.refresh(0, entry.term.rows - 1);
    let frame = 0;
    const tick = () => {
      if (!entry.term.element?.isConnected) return;
      const rect = container.getBoundingClientRect();
      if (rect.width > 0 && rect.height > 0) {
        entry.fitAddon.fit();
        entry.webglAddon?.clearTextureAtlas();
        entry.term.refresh(0, entry.term.rows - 1);
        return;
      }
      if (++frame < 3) requestAnimationFrame(tick);
    };
    requestAnimationFrame(tick);
    return true;
  } catch {
    return false;
  }
}

/** Fully dispose the terminal and remove it from the registry.
 *  Call this when a pane is explicitly closed (not just restructured by a split). */
export function disposeTerminal(paneId: string): void {
  const entry = registry.get(paneId);
  if (!entry) return;
  registry.delete(paneId);
  try {
    entry.term.dispose();
  } catch {
    /* ignore errors during disposal */
  }
}
