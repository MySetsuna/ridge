/**
 * Terminal link hover / click arbitration (OP-TERM-LINK).
 *
 * Pure functions — no DOM. Manager wires these into pointer handlers so:
 * - Ctrl/Cmd-hover on a link → show underline + pointer (affordance)
 * - TUI mouse reporting on → click goes to program only; open link needs Ctrl+click
 * - TUI mouse off → Ctrl+click opens; bare click never opens (avoids accidental nav)
 */

import type { LinkSpan, LinkSpanKind } from './linkSpans';

export type LinkOpenTarget =
  | { kind: 'url'; href: string }
  | { kind: 'path'; path: string; line?: number; col?: number }
  | { kind: 'file-url'; href: string };

export interface HoverUnderlineDecision {
  /** Whether renderer/DOM should paint underline under the span. */
  showUnderline: boolean;
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
  if (/^[A-Za-z]:$/.test(pathPart.replace(/\\/g, ''))) return { path: text };
  // "http://..." must not parse as path:line
  if (/^https?:\/\//i.test(text) || /^file:\/\//i.test(text)) {
    return { path: text };
  }
  const line = m[2] ? parseInt(m[2], 10) : undefined;
  const col = m[3] ? parseInt(m[3], 10) : undefined;
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
 * Hover affordance: underline only with Ctrl/Cmd modifier (iTerm/VS Code style).
 * Without modifier, no underline (TUI UIs often use their own hover).
 */
export function decideHoverUnderline(opts: {
  hasLinkHit: boolean;
  modifierHeld: boolean;
  spanText?: string | null;
}): HoverUnderlineDecision {
  if (opts.hasLinkHit && opts.modifierHeld) {
    return {
      showUnderline: true,
      cursor: 'pointer',
      spanText: opts.spanText ?? null,
    };
  }
  return { showUnderline: false, cursor: '', spanText: null };
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
  if (opts.mouseReportingOn) {
    // TUI owns clicks; open only with modifier+link
    if (opts.modifierHeld && opts.hasLinkHit) {
      return {
        forwardToProgram: false,
        openLink: true,
        startHostSelection: false,
      };
    }
    return {
      forwardToProgram: true,
      openLink: false,
      startHostSelection: false,
    };
  }
  // Host path: Ctrl+click opens; bare click starts selection
  if (opts.modifierHeld && opts.hasLinkHit) {
    return {
      forwardToProgram: false,
      openLink: true,
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
