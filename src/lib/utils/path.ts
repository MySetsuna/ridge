// src/lib/utils/path.ts
//
// 路径处理共用工具。原来散落在 MarkdownPreview.svelte 内（isExternalUrl /
// isWindowsAbsolute / joinPath / stripQuery / isCurrentDirHref），现在被
// linkResolver 与终端 detector 共用，提到这里。

/** True for schemes the OS shell should handle (browser / mail client). */
export function isExternalUrl(href: string): boolean {
  return /^(https?:|mailto:|ftp:|tel:)/i.test(href);
}

/** True for a Windows-style absolute path like `C:\...` or `C:/...`. */
export function isWindowsAbsolute(href: string): boolean {
  return /^[a-zA-Z]:[\\/]/.test(href);
}

/** True for a POSIX-style absolute path like `/foo/bar`. */
export function isPosixAbsolute(href: string): boolean {
  return href.startsWith('/');
}

/** True for `~` or `~/`. The actual `$HOME` substitution must happen
 *  upstream (前端无法可靠取系统 home，依赖后端命令展开)。 */
export function isHomeRelative(href: string): boolean {
  return href === '~' || href.startsWith('~/') || href.startsWith('~\\');
}

/**
 * Join `base` (a directory) with `rel` (a relative posix-style path). Keeps
 * the separator style of `base` when possible. Strips leading `./`.
 */
export function joinPath(base: string, rel: string): string {
  const sep = base.includes('\\') && !base.includes('/') ? '\\' : '/';
  const cleanBase = base.replace(/[\\/]+$/, '');
  const cleanRel = rel.replace(/^\.\//, '');
  // Normalise rel's own slashes to match base's sep
  const normalisedRel = cleanRel.split(/[\\/]+/).join(sep);
  return `${cleanBase}${sep}${normalisedRel}`;
}

/**
 * 把路径中的 `..` / `.` 段折叠掉。不会查 fs，仅做字符串归一。
 * 用于"是否属于某 cwd 树"的前缀比较。
 */
export function normalizePath(p: string): string {
  if (!p) return p;
  const winDrive = /^([a-zA-Z]):[\\/]/.test(p) ? p.slice(0, 3) : '';
  const rest = winDrive ? p.slice(3) : p;
  const sep = p.includes('\\') && !p.includes('/') ? '\\' : '/';
  const isAbs = winDrive || rest.startsWith('/') || rest.startsWith('\\');
  const segs = rest.split(/[\\/]+/).filter(Boolean);
  const stack: string[] = [];
  for (const s of segs) {
    if (s === '.') continue;
    if (s === '..') {
      if (stack.length > 0 && stack[stack.length - 1] !== '..') stack.pop();
      else if (!isAbs) stack.push('..');
      continue;
    }
    stack.push(s);
  }
  const head = winDrive || (isAbs ? sep : '');
  return head + stack.join(sep);
}

/**
 * Strip a trailing `?query` (and any embedded query before the hash) from a
 * path-like href. CommonMark treats everything before `#` as the path part,
 * but real local files don't have `?query` — markdown sometimes uses it as
 * a cache-buster (`./img.png?v=2`). 静默丢掉。
 */
export function stripQuery(pathPart: string): string {
  const q = pathPart.indexOf('?');
  return q >= 0 ? pathPart.slice(0, q) : pathPart;
}

/**
 * Detect href that targets the containing directory itself (`.` / `./` / `.\`).
 * 由 markdown 的 `[here](.)` 触发，应交给 reveal_in_file_manager。
 */
export function isCurrentDirHref(href: string): boolean {
  return href === '.' || href === './' || href === '.\\';
}

/** 大小写敏感感知的 startsWith：Windows 绝对路径不区分大小写，POSIX 区分。 */
export function pathStartsWith(child: string, parent: string): boolean {
  if (!child || !parent) return false;
  const c = normalizePath(child);
  const p = normalizePath(parent.replace(/[\\/]+$/, ''));
  if (c.length < p.length) return false;
  const isWin = /^[a-zA-Z]:[\\/]/.test(p);
  const head = c.slice(0, p.length);
  if (isWin) {
    if (head.toLowerCase() !== p.toLowerCase()) return false;
  } else {
    if (head !== p) return false;
  }
  if (c.length === p.length) return true;
  const next = c[p.length];
  return next === '/' || next === '\\';
}
