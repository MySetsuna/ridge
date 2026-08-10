/**
 * Terminal link hover / click arbitration (OP-TERM-LINK).
 *
 * Pure functions — no DOM. Manager wires these into pointer handlers so:
 * - Hover on a validated link → show a thin underline, pointer, and a direct-click hint
 * - Primary click on a validated link → open without requiring a keyboard modifier
 * - Non-link clicks retain TUI forwarding / host selection semantics
 */

import type { LinkSpan, LinkSpanKind } from './linkSpans';

export type LinkOpenTarget =
  | { kind: 'url'; href: string }
  | { kind: 'path'; path: string; line?: number; col?: number }
  | { kind: 'file-url'; href: string };

export interface HoverUnderlineDecision {
  /** Whether renderer/DOM should paint underline under the span. */
  showUnderline: boolean;
  /** Whether a non-blocking hint should tell the user that a direct click opens. */
  showHint: boolean;
  hintText: string | null;
  /** CSS cursor value (`pointer` | ``). */
  cursor: '' | 'pointer';
  /** Hit span text when underline should show. */
  spanText: string | null;
}

export interface LinkClickDecision {
  /** Forward mouse event to TUI program (SGR). */
  forwardToProgram: boolean;
  /** Open the link/path via host ports. */
  openLink: boolean;
  /** Start host text selection. */
  startHostSelection: boolean;
}

/** Parse optional `file:line` / `file:line:col` suffix from path text. */
export function parsePathWithLocation(text: string): {
  path: string;
  line?: number;
  col?: number;
} {
  // Windows drive: C:\foo\bar.rs:12:3 — only treat trailing :digits as loc
  const m = text.match(/^(.*?)(?::(\d+))(?::(\d+))?$/);
  if (!m) return { path: text };
  const pathPart = m[1];
  // Avoid treating "C:" as path with line — require more path after drive
  if (/^[A-Za-z]:$/.test(pathPart)) return { path: text };
  if (/^[A-Za-z]:$/.test(pathPart.replaceAll(/\\/g, ''))) return { path: text };
  // "http://..." must not parse as path:line
  if (/^https?:\/\//i.test(text) || /^file:\/\//i.test(text)) {
    return { path: text };
  }
  const line = m[2] ? Number.parseInt(m[2], 10) : undefined;
  const col = m[3] ? Number.parseInt(m[3], 10) : undefined;
  // Reject if path looks empty
  if (!pathPart || pathPart.length < 1) return { path: text };
  return { path: pathPart, line, col };
}

export function resolveOpenTarget(
  text: string,
  kind: LinkSpanKind | 'osc8',
): LinkOpenTarget {
  if (kind === 'url' || kind === 'osc8') {
    return { kind: 'url', href: text };
  }
  if (kind === 'file-url') {
    return { kind: 'file-url', href: text };
  }
  const loc = parsePathWithLocation(text);
  return {
    kind: 'path',
    path: loc.path,
    line: loc.line,
    col: loc.col,
  };
}

/**
 * Resolve relative path against pane cwd then workspace root.
 */
export function resolvePathAgainstCwd(
  path: string,
  paneCwd: string | null | undefined,
  workspaceRoot: string | null | undefined,
): string {
  if (!path) return path;
  // Absolute win / posix / home
  if (/^[A-Za-z]:[\\/]/.test(path) || path.startsWith('/') || path.startsWith('~/')) {
    return path;
  }
  const base = (paneCwd && paneCwd.length > 0 ? paneCwd : workspaceRoot) || '';
  if (!base) return path;
  const sep = base.includes('\\') || path.includes('\\') ? '\\' : '/';
  const left = base.replace(/[\\/]+$/, '');
  const right = path.replace(/^\.[\\/]/, '');
  return `${left}${sep}${right}`;
}

/**
 * Hover affordance: a validated link is actionable with a primary click on
 * both desktop and mobile, so keep the underline/cursor stable while hovering.
 */
export function decideHoverUnderline(opts: {
  hasLinkHit: boolean;
  modifierHeld: boolean;
  spanText?: string | null;
}): HoverUnderlineDecision {
  if (opts.hasLinkHit) {
    return {
      showUnderline: true,
      showHint: !opts.modifierHeld,
      hintText: opts.modifierHeld ? null : '点击可跳转',
      cursor: 'pointer',
      spanText: opts.spanText ?? null,
    };
  }
  return { showUnderline: false, showHint: false, hintText: null, cursor: '', spanText: null };
}

export interface Osc8LinkGrid {
  rows(): number;
  cols(): number;
  hyperlinkAt(row: number, col: number): { uri?: unknown } | null;
  /** True when this row's last cell soft-wraps into the next row. */
  rowWrapped?: (row: number) => boolean;
}

/**
 * Return every visual segment of the OSC-8 link under the hovered cell.
 *
 * OSC-8 carries a URI per cell, but not a logical span id. Only join adjacent
 * rows when the terminal's authoritative soft-wrap flag proves they are one
 * visual line; repeated copies of the same URI on separate hard lines stay
 * independent. This keeps the underline continuous without over-highlighting.
 */
export function osc8UnderlineRegions(
  grid: Osc8LinkGrid,
  row: number,
  col: number,
  uri: string | null,
): { row: number; c0: number; c1: number }[] {
  if (!uri) return [{ row, c0: col, c1: col + 1 }];
  let rows = 0;
  let cols = 0;
  try { rows = Math.max(0, grid.rows()); } catch { rows = row + 1; }
  try { cols = Math.max(1, grid.cols()); } catch { cols = col + 1; }
  if (row < 0 || row >= rows || col < 0 || col >= cols) {
    return [{ row, c0: col, c1: col + 1 }];
  }
  const same = (r: number, c: number): boolean => {
    try { return grid.hyperlinkAt(r, c)?.uri === uri; } catch { return false; }
  };
  const segmentAt = (r: number, seed: number) => {
    if (r < 0 || r >= rows || seed < 0 || seed >= cols || !same(r, seed)) return null;
    let c0 = seed;
    let c1 = seed + 1;
    while (c0 > 0 && same(r, c0 - 1)) c0 -= 1;
    while (c1 < cols && same(r, c1)) c1 += 1;
    return { row: r, c0, c1 };
  };
  const current = segmentAt(row, col);
  if (!current) return [{ row, c0: col, c1: col + 1 }];
  const regions = [current];
  const wrapped = (r: number): boolean => {
    try { return grid.rowWrapped?.(r) === true; } catch { return false; }
  };
  let head = current;
  while (head.c0 === 0 && head.row > 0 && wrapped(head.row - 1)) {
    const previous = segmentAt(head.row - 1, cols - 1);
    if (!previous || previous.c1 !== cols) break;
    regions.unshift(previous);
    head = previous;
  }
  let tail = current;
  while (tail.c1 === cols && tail.row < rows - 1 && wrapped(tail.row)) {
    const next = segmentAt(tail.row + 1, 0);
    if (!next || next.c0 !== 0) break;
    regions.push(next);
    tail = next;
  }
  return regions;
}

/**
 * Click arbitration matrix.
 * mouseReportingOn: DEC mouse modes active on kernel.
 * modifierHeld: Ctrl/Cmd
 * hasLinkHit: OSC8 or linkSpans hit under cursor
 * primaryButton: left button
 */
export function decideLinkClick(opts: {
  mouseReportingOn: boolean;
  modifierHeld: boolean;
  hasLinkHit: boolean;
  primaryButton: boolean;
}): LinkClickDecision {
  if (!opts.primaryButton) {
    return {
      forwardToProgram: opts.mouseReportingOn,
      openLink: false,
      startHostSelection: false,
    };
  }
  // A validated link owns the primary click even when a TUI has enabled mouse
  // reporting. This is the only exception to the TUI forwarding contract.
  if (opts.hasLinkHit) {
    return {
      forwardToProgram: false,
      openLink: true,
      startHostSelection: false,
    };
  }
  if (opts.mouseReportingOn) {
    return {
      forwardToProgram: true,
      openLink: false,
      startHostSelection: false,
    };
  }
  return {
    forwardToProgram: false,
    openLink: false,
    startHostSelection: true,
  };
}

/** Serialize span underline region for renderer overlay. */
export function underlineRegionsFromSpan(span: Pick<LinkSpan, 'row' | 'c0' | 'c1'>): {
  row: number;
  c0: number;
  c1: number;
} {
  return { row: span.row, c0: span.c0, c1: span.c1 };
}
