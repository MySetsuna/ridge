/** Link adapter. ridge-term owns detection, soft wrapping and cell projection. */
export type LinkSpanKind = 'url' | 'path';

export interface LinkSpan {
  row: number; // 0-based viewport row
  c0: number;  // inclusive start col
  c1: number;  // exclusive end col
  text: string; // 命中文本（已剥两端常见标点）
  kind: LinkSpanKind;
  /** Stable identity for one logical link, including soft-wrapped segments. */
  linkId?: string;
}


interface KernelLike { linkSpans(): LinkSpan[]; }

export function trimTrailingSeparators(s: string): string {
  let end = s.length;
  while (end > 0 && (s[end - 1] === '/' || s[end - 1] === '\\')) end -= 1;
  return s.slice(0, end);
}

/** Host-opening validation remains a separate trust boundary. */
export function isStrictWebUrl(href: string): boolean {
  if (!href || href.length > 2_048 || /[\\\u0000-\u0020\u007f]/.test(href)) return false;
  try {
    const url = new URL(href);
    return (url.protocol === 'http:' || url.protocol === 'https:') &&
      url.hostname.length > 0 && !url.username && !url.password;
  } catch {
    return false;
  }
}

export class LinkSpanIndex {
  private spans: LinkSpan[] = [];
  private dirty = true;

  get needsRefresh(): boolean { return this.dirty; }

  markDirty(): void { this.dirty = true; }
  clear(): void { this.spans = []; this.dirty = true; }

  recompute(kernel: KernelLike): void {
    this.spans = kernel.linkSpans();
    this.dirty = false;
  }

  hitTest(kernel: KernelLike, row: number, col: number): LinkSpan | null {
    if (this.dirty) this.recompute(kernel);
    return this.spans.find(span => span.row === row && col >= span.c0 && col < span.c1) ?? null;
  }

  regionsForSpan(kernel: KernelLike, span: LinkSpan): LinkSpan[] {
    if (this.dirty) this.recompute(kernel);
    return span.linkId ? this.spans.filter(candidate => candidate.linkId === span.linkId) : [span];
  }
}
