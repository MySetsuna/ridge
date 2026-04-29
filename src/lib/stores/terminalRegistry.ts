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

export interface TerminalEntry {
  term: Terminal;
  fitAddon: FitAddon;
  webglAddon: WebglAddon | null;
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

/** Move the terminal's DOM element from parking lot back into `container`.
 *  Returns true if successfully restored. */
export function restoreTerminal(paneId: string, container: HTMLElement): boolean {
  const entry = registry.get(paneId);
  if (!entry || !entry.term.element) return false;
  try {
    container.appendChild(entry.term.element);
    entry.fitAddon.fit();
    requestAnimationFrame(() => {
      if (entry.term.element?.isConnected) entry.fitAddon.fit();
    });
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
