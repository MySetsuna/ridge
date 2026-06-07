// src/lib/utils/linkResolver.ts
//
// 跨 MarkdownPreview / 终端 / 未来 chat 面板复用的链接路由器。
//
// 三段式：classifyLink 纯字符串分类；resolveLink 结合上下文（cwd /
// basePath / knownCwds）决定具体动作；executeAction 真正执行 IO（打开
// 编辑器 / 系统资源管理器 / 默认浏览器）。
//
// 调用方只关心 resolveLink + executeAction 这一对，不直接拼路径。

import { isTauri, invoke } from '@tauri-apps/api/core';
import { fileEditorStore } from '$lib/stores/fileEditor';
import { choiceDialog } from '$lib/components/RidgeDialog.svelte';
import {
  hostKeyFromUrl,
  isTrustedUrl,
  trustHostFromUrl,
} from '$lib/utils/linkTrust';
import {
  isCurrentDirHref,
  isExternalUrl,
  isPosixAbsolute,
  isWindowsAbsolute,
  joinPath,
  pathStartsWith,
  stripQuery,
} from '$lib/utils/path';

export type LinkKind =
  | 'http'
  | 'mailto'
  | 'file-url'
  | 'absolute'
  | 'relative'
  | 'fragment'
  | 'unknown';

export type LinkAction =
  | { kind: 'open-url'; href: string; trustBase?: string }
  | { kind: 'open-file'; path: string; line?: number; col?: number }
  | { kind: 'reveal'; path: string }
  | { kind: 'fragment'; id: string }
  | { kind: 'noop'; reason?: string };

export interface ResolveCtx {
  /** 调用者所在 pane 的 OSC 7 cwd（终端使用）。 */
  cwd?: string;
  /** 调用者文档的目录（markdown 使用，例如 markdown 文件所在目录）。 */
  basePath?: string;
  /** 所有 pane 当前 cwd（用于"是否属于任意 cwd 树"判断）。 */
  knownCwds?: string[];
}

/** 纯字符串规则，不查 fs。顺序短路。 */
export function classifyLink(href: string): LinkKind {
  if (!href) return 'unknown';
  if (href.startsWith('#')) return 'fragment';
  if (/^(https?|ftp):/i.test(href)) return 'http';
  if (/^(mailto|tel):/i.test(href)) return 'mailto';
  if (/^file:/i.test(href)) return 'file-url';
  if (isWindowsAbsolute(href) || isPosixAbsolute(href) || href.startsWith('~/') || href.startsWith('~\\')) {
    return 'absolute';
  }
  if (/^\.{1,2}[\\/]/.test(href) || /^[^:\s]+\.[a-zA-Z]{1,8}$/.test(href)) {
    return 'relative';
  }
  return 'unknown';
}

/** 从 file:// URL 中抽出真实文件路径。失败返回 null。 */
function pathFromFileUrl(href: string): string | null {
  try {
    const u = new URL(href);
    return decodeURIComponent(u.pathname.replace(/^\/(\w:)/, '$1'));
  } catch {
    return null;
  }
}

/** 容错的 decodeURIComponent —— 失败时回退到原字符串（与 markdown 链接的原有逻辑一致）。 */
function safeDecode(s: string): string {
  try {
    return decodeURIComponent(s);
  } catch {
    return s;
  }
}

/**
 * 把 href 归一为 `LinkAction`。具体决策：
 *
 * - http/mailto → open-url（mailto 在 executeAction 里跳过信任弹窗）
 * - fragment → 交回调用方（resolver 不滚 DOM）
 * - file-url / absolute / relative → 解析为绝对路径后判断归属：
 *   * 命中 `knownCwds` 任一前缀 → open-file（在 ridge 内置编辑器打开）
 *   * 否则 → reveal（系统资源管理器定位）
 *
 * 文件 vs 目录无法纯字符串判断；策略：路径**末段含已知文本扩展名**走
 * open-file，否则走 reveal。后端 `read_file_for_editor` 失败兜底。
 */
export function resolveLink(href: string, ctx: ResolveCtx): LinkAction {
  if (!href) return { kind: 'noop', reason: 'empty href' };
  const trimmed = href.trim();
  if (trimmed.toLowerCase().startsWith('javascript:')) {
    return { kind: 'noop', reason: 'javascript: ignored' };
  }
  const kind = classifyLink(trimmed);

  if (kind === 'fragment') {
    return { kind: 'fragment', id: trimmed.slice(1) };
  }
  if (kind === 'http' || kind === 'mailto') {
    return { kind: 'open-url', href: trimmed, trustBase: ctx.basePath };
  }

  // 文件类：拆 fragment、剥 query、解码、解析为绝对路径
  const hashIdx = trimmed.indexOf('#');
  const noHash = hashIdx >= 0 ? trimmed.slice(0, hashIdx) : trimmed;
  if (isCurrentDirHref(noHash)) {
    return ctx.basePath
      ? { kind: 'reveal', path: ctx.basePath }
      : { kind: 'noop', reason: 'no basePath for "."' };
  }
  const noQuery = stripQuery(noHash);
  let abs: string | null = null;

  if (kind === 'file-url') {
    abs = pathFromFileUrl(noQuery);
  } else if (kind === 'absolute') {
    abs = safeDecode(noQuery);
  } else if (kind === 'relative') {
    const base = ctx.basePath ?? ctx.cwd;
    if (!base) return { kind: 'noop', reason: 'relative without base' };
    abs = joinPath(base, safeDecode(noQuery));
  } else {
    // unknown：可能是裸文件名 `foo.md`，若有 base 也按相对处理
    const base = ctx.basePath ?? ctx.cwd;
    if (!base) return { kind: 'noop', reason: 'unknown without base' };
    abs = joinPath(base, safeDecode(noQuery));
  }

  if (!abs) return { kind: 'noop', reason: 'cannot resolve absolute path' };

  const lastSeg = abs.split(/[\\/]/).pop() ?? '';
  const looksLikeFile = /\.[a-zA-Z0-9]{1,8}$/.test(lastSeg);

  // 决定 open-file vs reveal
  const owned =
    (ctx.knownCwds?.some((c) => pathStartsWith(abs!, c)) ?? false) ||
    (ctx.cwd ? pathStartsWith(abs, ctx.cwd) : false);

  if (looksLikeFile && owned) return { kind: 'open-file', path: abs };
  // 非 cwd 内的文件 / 目录 → 系统资源管理器；保留原地查看体验。
  return { kind: 'reveal', path: abs };
}

/** 真正执行：打开编辑器 / 系统资源管理器 / 默认浏览器。
 *  fragment 由调用方处理（resolver 不滚 DOM），这里 noop。 */
export async function executeAction(action: LinkAction): Promise<void> {
  switch (action.kind) {
    case 'open-url': {
      await openExternalWithTrust(action.href, action.trustBase);
      return;
    }
    case 'open-file': {
      await fileEditorStore.openFile(action.path, {
        line: action.line,
        column: action.col,
      });
      return;
    }
    case 'reveal': {
      if (!isTauri()) return;
      try {
        await invoke('reveal_in_file_manager', { path: action.path });
      } catch (err) {
        console.warn('[linkResolver] reveal_in_file_manager failed', action.path, err);
      }
      return;
    }
    case 'fragment':
    case 'noop':
      return;
  }
}

/** 复用 markdown preview 的同会话信任流程。mailto/tel 跳过弹窗。 */
async function openExternalWithTrust(href: string, trustBase?: string): Promise<void> {
  if (/^(mailto|tel):/i.test(href)) {
    await openShell(href);
    return;
  }
  if (!isTrustedUrl(href, trustBase)) {
    const host = hostKeyFromUrl(href) ?? href;
    const choice = await choiceDialog({
      title: '打开外部链接',
      message: `${host}\n${href}`,
      okLabel: '始终允许（本次会话）',
      secondaryLabel: '仅本次',
      cancelLabel: '取消',
    });
    if (choice === 'cancel') return;
    if (choice === 'primary') trustHostFromUrl(href, trustBase);
  }
  await openShell(href);
}

async function openShell(href: string): Promise<void> {
  // 区分"原生 Tauri vs 浏览器"用构建标志 RIDGE_WEB_REMOTE，而非 isTauri()：
  // 远控 shim 的 isTauri() 恒为 true（见 tauriShim/core.ts），若用它判据则
  // window.open 兜底永不执行，外链在远控浏览器里点了无反应。普通 Tauri 构建
  // 下该标志为 false，此分支被 tree-shake，原生行为不变（仍走 opener 插件）。
  if (import.meta.env.RIDGE_WEB_REMOTE === true) {
    window.open(href, '_blank', 'noopener,noreferrer');
    return;
  }
  try {
    const { openUrl } = await import('@tauri-apps/plugin-opener');
    await openUrl(href);
  } catch (err) {
    console.warn('[linkResolver] openUrl failed', href, err);
    // import/openUrl 失败时仍尝试浏览器兜底，避免静默失效。
    window.open(href, '_blank', 'noopener,noreferrer');
  }
}
