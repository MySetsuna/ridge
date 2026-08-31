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
import { isStrictWebUrl, trimTrailingSeparators, type LinkSpanKind } from './linkSpans';

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
  const scope = ctx.paneCwd || ctx.workspaceRoot;
  if (!scope || looksOutsideWorkspace(path, scope)) {
    return { type: 'noop', reason: 'outside_workspace' };
  }
  if (isProbablyDirectory(path)) return { type: 'noop', reason: 'directory_path' };
  return { type: 'open_file', path, line, col };
}

export interface PathProbeResult {
  exists: boolean;
  isDirectory?: boolean;
}

interface PathProbeCacheEntry {
  expiresAt: number;
  value?: PathProbeResult;
  pending?: Promise<PathProbeResult>;
  abort?: () => void;
}

const PATH_PROBE_TIMEOUT_MS = 2_000;
const PATH_PROBE_TTL_MS = 15_000;
const PATH_PROBE_CACHE_MAX = 128;
const pathProbeCache = new Map<string, PathProbeCacheEntry>();

/** Click-only path proof. Timeout aborts origin RPC; successful positive and
 * negative proofs are briefly cached so repeated clicks never fan out IO. */
export async function probePathWithCache(
  key: string,
  inspect: (signal: AbortSignal) => Promise<PathProbeResult>,
  options: { timeoutMs?: number; ttlMs?: number; now?: () => number } = {},
): Promise<PathProbeResult> {
  const now = options.now ?? Date.now;
  const cached = pathProbeCache.get(key);
  if (cached?.pending) return cached.pending;
  if (cached?.value && cached.expiresAt > now()) return cached.value;
  if (cached) pathProbeCache.delete(key);

  const controller = new AbortController();
  const timeoutMs = options.timeoutMs ?? PATH_PROBE_TIMEOUT_MS;
  let timer: ReturnType<typeof setTimeout> | undefined;
  const timeout = new Promise<never>((_, reject) => {
    timer = setTimeout(() => {
      controller.abort(new Error('path probe timeout'));
      reject(new Error('path probe timeout'));
    }, timeoutMs);
  });
  const pending = Promise.race([inspect(controller.signal), timeout])
    .then((value) => {
      const entry: PathProbeCacheEntry = {
        value,
        expiresAt: now() + (options.ttlMs ?? PATH_PROBE_TTL_MS),
      };
      pathProbeCache.delete(key);
      pathProbeCache.set(key, entry);
      while (pathProbeCache.size > PATH_PROBE_CACHE_MAX) {
        const oldest = pathProbeCache.keys().next().value as string | undefined;
        if (oldest === undefined) break;
        pathProbeCache.delete(oldest);
      }
      return value;
    })
    .catch((error) => {
      pathProbeCache.delete(key);
      throw error;
    })
    .finally(() => {
      if (timer !== undefined) clearTimeout(timer);
    });
  pathProbeCache.set(key, {
    pending,
    expiresAt: 0,
    abort: () => controller.abort(new Error('path probe cancelled')),
  });
  return pending;
}

export function clearPathProbeCache(): void {
  for (const entry of pathProbeCache.values()) {
    entry.abort?.();
    void entry.pending?.catch(() => {});
  }
  pathProbeCache.clear();
}

export function isSafeHttpUrl(href: string): boolean {
  return isStrictWebUrl(href);
}

export function looksOutsideWorkspace(path: string, root: string): boolean {
  const norm = (s: string) => s.replaceAll('\\', '/').toLowerCase();
  const p = norm(path);
  const normalizedRoot = norm(root);
  const r = trimTrailingSeparators(normalizedRoot);
  if (/^[a-z]:\//.test(p) || p.startsWith('/')) {
    return p !== r && !p.startsWith(`${r}/`);
  }
  return p.includes('..');
}

/** The only non-web link class. */
export function isPathSpanKind(
  kind: LinkSpanKind | 'osc8' | null | undefined,
): boolean {
  return kind === 'path';
}

/** Underline CSS class tokens for renderer. */
export function underlineCssTokens(opts: {
  show: boolean;
  kind: LinkSpanKind | 'osc8' | null;
}): string[] {
  if (!opts.show) return [];
  const tokens = ['ridge-link-underline'];
  if (opts.kind === 'url' || opts.kind === 'osc8') tokens.push('ridge-link-url');
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
