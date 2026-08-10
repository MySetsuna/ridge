/**
 * CONTRACT-51 / OP-TERM-LINK: open target → host action plan.
 * Manager + desktop invoke ports use this; pure, no DOM.
 */

import {
  parsePathWithLocation,
  resolveOpenTarget,
  resolvePathAgainstCwd,
  type LinkOpenTarget,
} from './linkAffordance';
import { trimTrailingSeparators, type LinkSpanKind } from './linkSpans';

export type HostOpenAction =
  | { type: 'open_url'; href: string }
  | { type: 'open_file'; path: string; line?: number; col?: number }
  | { type: 'reveal_in_tree'; path: string }
  | { type: 'noop'; reason: string };

export interface OpenContext {
  paneCwd?: string | null;
  workspaceRoot?: string | null;
  /** Prefer editor for files; directory → tree. */
  preferEditor?: boolean;
}

const DIR_HINT = /[/\\]$|\\$|\/$/;

export function isProbablyDirectory(path: string): boolean {
  if (!path) return false;
  if (DIR_HINT.test(path)) return true;
  // no extension and no trailing slash still may be dir — leave to host
  const base = path.split(/[/\\]/).pop() || '';
  if (!base.includes('.') && base.length > 0) return false;
  return false;
}

export function planHostOpen(
  text: string,
  kind: LinkSpanKind | 'osc8',
  ctx: OpenContext = {},
): HostOpenAction {
  const target = resolveOpenTarget(text, kind);
  return planFromTarget(target, ctx);
}

export function planFromTarget(target: LinkOpenTarget, ctx: OpenContext = {}): HostOpenAction {
  if (target.kind === 'url') {
    if (!isSafeHttpUrl(target.href)) {
      return { type: 'noop', reason: 'unsafe_url' };
    }
    return { type: 'open_url', href: target.href };
  }
  if (target.kind === 'file-url') {
    const path = fileUrlToPath(target.href);
    if (!path) return { type: 'noop', reason: 'bad_file_url' };
    return planPathOpen(path, undefined, undefined, ctx);
  }
  const abs = resolvePathAgainstCwd(target.path, ctx.paneCwd, ctx.workspaceRoot);
  return planPathOpen(abs, target.line, target.col, ctx);
}

function planPathOpen(
  path: string,
  line: number | undefined,
  col: number | undefined,
  ctx: OpenContext,
): HostOpenAction {
  if (!path || path.includes('\0')) {
    return { type: 'noop', reason: 'empty_path' };
  }
  // Block path traversal tricks relative to workspace when both set
  if (ctx.workspaceRoot && looksOutsideWorkspace(path, ctx.workspaceRoot)) {
    // still allow absolute opens outside workspace (editor may refuse)
  }
  if (isProbablyDirectory(path) || (ctx.preferEditor === false && !line)) {
    if (isProbablyDirectory(path)) {
      return { type: 'reveal_in_tree', path };
    }
  }
  return { type: 'open_file', path, line, col };
}

export function isSafeHttpUrl(href: string): boolean {
  try {
    const u = new URL(href);
    return u.protocol === 'http:' || u.protocol === 'https:';
  } catch {
    return false;
  }
}

export function fileUrlToPath(href: string): string | null {
  try {
    const u = new URL(href);
    if (u.protocol !== 'file:') return null;
    let p = decodeURIComponent(u.pathname);
    // Windows file:///C:/...
    if (/^\/[A-Za-z]:\//.test(p)) p = p.slice(1);
    return p;
  } catch {
    return null;
  }
}

export function looksOutsideWorkspace(path: string, root: string): boolean {
  const norm = (s: string) => s.replaceAll('\\', '/').toLowerCase();
  const p = norm(path);
  const normalizedRoot = norm(root);
  const r = trimTrailingSeparators(normalizedRoot);
  if (/^[a-z]:\//.test(p) || p.startsWith('/')) {
    return !p.startsWith(r);
  }
  return p.includes('..');
}

/** Path-like span kinds (post-refactor granular LinkSpanKind). */
export function isPathSpanKind(
  kind: LinkSpanKind | 'osc8' | null | undefined,
): boolean {
  return (
    kind === 'win-abs' ||
    kind === 'posix-abs' ||
    kind === 'home' ||
    kind === 'rel'
  );
}

/** Underline CSS class tokens for renderer. */
export function underlineCssTokens(opts: {
  show: boolean;
  kind: LinkSpanKind | 'osc8' | null;
}): string[] {
  if (!opts.show) return [];
  const tokens = ['ridge-link-underline'];
  if (opts.kind === 'url' || opts.kind === 'osc8') tokens.push('ridge-link-url');
  else if (opts.kind === 'file-url') tokens.push('ridge-link-file');
  else if (isPathSpanKind(opts.kind)) tokens.push('ridge-link-path');
  return tokens;
}

/** Dataset value for container: "row:c0:c1" or "row:osc8". */
export function encodeUnderlineDataset(
  row: number,
  c0: number | 'osc8',
  c1?: number,
): string {
  if (c0 === 'osc8') return `${row}:osc8`;
  return `${row}:${c0}:${c1 ?? c0}`;
}

export function decodeUnderlineDataset(
  value: string | undefined,
): { row: number; c0: number; c1: number } | { row: number; osc8: true } | null {
  if (!value) return null;
  if (value.endsWith(':osc8')) {
    const row = Number.parseInt(value.split(':')[0]!, 10);
    if (Number.isNaN(row)) return null;
    return { row, osc8: true };
  }
  const parts = value.split(':');
  if (parts.length < 3) return null;
  const row = Number.parseInt(parts[0]!, 10);
  const c0 = Number.parseInt(parts[1]!, 10);
  const c1 = Number.parseInt(parts[2]!, 10);
  if ([row, c0, c1].some((n) => Number.isNaN(n))) return null;
  return { row, c0, c1 };
}

/** Full click → open pipeline combining arbitration already done. */
export function buildOpenPlanFromHit(opts: {
  text: string;
  kind: LinkSpanKind | 'osc8';
  paneCwd?: string | null;
  workspaceRoot?: string | null;
}): HostOpenAction {
  return planHostOpen(opts.text, opts.kind, {
    paneCwd: opts.paneCwd,
    workspaceRoot: opts.workspaceRoot,
    preferEditor: true,
  });
}

export function parsePathLineCol(text: string): ReturnType<typeof parsePathWithLocation> {
  return parsePathWithLocation(text);
}
