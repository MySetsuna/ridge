// src/lib/lsp/lspClient.ts
//
// 前端 LSP 薄客户端：把 Monaco 的 go-to-definition 桥到 Rust LSP host（见
// src-tauri/src/lsp）。负责 文件路径↔file:// URI 转换、语言 id 映射、文档同步
// (didOpen/didChange) 与 definition 结果解析。FileEditor 在 Ctrl+Click 非路径
// token 时调 lspDefinition → 用 fileEditorStore.openFile 落到定义处。
//
// P1 仅 TypeScript/JavaScript + definition。诊断/hover/references(P2) 后续。

import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { showToast } from '../stores/toast';

// ── 供给检测（P4）：语言服务器缺失 → 分类 + 去重 + 用户提示 ──────────────────
// Rust host 起进程失败时返回 `启动语言服务器失败（<install_hint>）：<spawn error>`
// （src-tauri/src/lsp/mod.rs），此前前端仅 console.warn 静默吞掉——用户不知道
// 「跳转/悬浮没反应」是因为 server 没装。P4 把它分类出来、按语言去重地提示一次。
const LSP_SPAWN_FAIL_MARKER = '启动语言服务器失败';

/** 分类一个 LSP 命令错误：是否「语言服务器未安装/起不来」，并提取安装提示。
 *  纯函数、可离线测。提示自身可能含全角括号（rust-analyzer 的提示就有），故非贪婪
 *  锚定到收尾的 `）：` 提取，避免被内层括号截断；提取不到时回退整条消息。 */
export function classifyLspError(err: unknown): { missingServer: boolean; hint: string } {
  const msg = err instanceof Error ? err.message : String(err ?? '');
  if (!msg.includes(LSP_SPAWN_FAIL_MARKER)) return { missingServer: false, hint: '' };
  const m = msg.match(/启动语言服务器失败（([\s\S]*?)）：/);
  return { missingServer: true, hint: m?.[1] ?? msg };
}

/** 提示 sink：默认接真实 toast，生产开箱即用；测试经 [`setLspErrorSink`] 注入 spy。 */
let errorSink: (hint: string) => void = (hint) => showToast(hint, 'error');
/** 已提示过的安装提示集合，去重（避免每次 didChange/definition 刷屏）。 */
const shownHints = new Set<string>();

/** 覆盖缺失提示 sink（测试注入 spy；也可用于自定义 UI）。 */
export function setLspErrorSink(sink: (hint: string) => void): void {
  errorSink = sink;
}

/** 清空「已提示」去重集（测试用；server 重装后想再次提示也可调）。 */
export function resetLspErrorNotices(): void {
  shownHints.clear();
}

/** 若 err 表示语言服务器缺失，按提示去重地提示一次。返回是否本次触发了提示。 */
export function notifyLspError(err: unknown): boolean {
  const { missingServer, hint } = classifyLspError(err);
  if (!missingServer || shownHints.has(hint)) return false;
  shownHints.add(hint);
  errorSink(hint);
  return true;
}

/** LSP 跳转目标（已转成前端口径：path + 1-based line/column）。 */
export interface LspTarget {
  path: string;
  line: number; // 1-based（Monaco 口径）
  column: number; // 1-based
}

/** 文件路径 → `file://` URI。Windows `C:\a\b` → `file:///C:/a/b`。 */
export function pathToUri(path: string): string {
  const p = path.replace(/\\/g, '/');
  const encoded = p
    .split('/')
    // 盘符段 `C:` 的冒号不编码（保持 file:///C:/… 的常见形态）；其余段编码空格等。
    .map((seg) => (/^[A-Za-z]:$/.test(seg) ? seg : encodeURIComponent(seg)))
    .join('/');
  return p.startsWith('/') ? `file://${encoded}` : `file:///${encoded}`;
}

/** `file://` URI → 文件路径。Windows（含盘符）转回反斜杠以匹配本机路径口径。 */
export function uriToPath(uri: string): string {
  let p = uri.replace(/^file:\/\//, '');
  try {
    p = decodeURIComponent(p);
  } catch {
    /* 保留原串 */
  }
  // file:///C:/a → /C:/a：盘符前的前导斜杠去掉，/ 还原成 \，并把盘符统一大写
  //（Monaco/LSP 常返回小写盘符 file:///c:/…，本机 tab 路径多为大写盘符，归一避免重复 tab）。
  const winDrive = /^\/([A-Za-z]):/.exec(p);
  if (winDrive) {
    return p
      .slice(1)
      .replace(/\//g, '\\')
      .replace(/^[a-z]:/, (m) => m.toUpperCase());
  }
  return p;
}

/** 文件扩展名 → LSP languageId。不支持的语言返回 null（P1 仅 TS/JS）。 */
export function lspLanguageId(path: string): string | null {
  const lower = path.toLowerCase();
  if (lower.endsWith('.tsx')) return 'typescriptreact';
  if (lower.endsWith('.jsx')) return 'javascriptreact';
  if (lower.endsWith('.ts') || lower.endsWith('.mts') || lower.endsWith('.cts'))
    return 'typescript';
  if (lower.endsWith('.js') || lower.endsWith('.mjs') || lower.endsWith('.cjs'))
    return 'javascript';
  if (lower.endsWith('.rs')) return 'rust';
  return null;
}

/** 该文件是否由 P1 LSP 覆盖（TS/JS 家族）。 */
export function lspSupports(path: string): boolean {
  return lspLanguageId(path) !== null;
}

/**
 * 解析 LSP definition 原始结果（Location | Location[] | LocationLink[] | null）为
 * LspTarget[]。LSP 是 0-based 行列 → 转 1-based（Monaco/openFile 口径）。
 */
export function parseDefinition(raw: unknown): LspTarget[] {
  if (!raw || typeof raw !== 'object') {
    if (Array.isArray(raw)) return raw.flatMap((r) => parseDefinition(r));
    return [];
  }
  // LSP error 信封（Rust host 在出错时回 {__lsp_error}）。
  if ('__lsp_error' in (raw as Record<string, unknown>)) return [];
  if (Array.isArray(raw)) {
    return (raw as unknown[]).flatMap((r) => parseDefinition(r));
  }
  const obj = raw as Record<string, unknown>;
  // LocationLink：{ targetUri, targetSelectionRange | targetRange }
  if (typeof obj.targetUri === 'string') {
    const range = (obj.targetSelectionRange ?? obj.targetRange) as
      | { start?: { line?: number; character?: number } }
      | undefined;
    const start = range?.start;
    return [
      {
        path: uriToPath(obj.targetUri),
        line: (start?.line ?? 0) + 1,
        column: (start?.character ?? 0) + 1,
      },
    ];
  }
  // Location：{ uri, range: { start: {line, character} } }
  if (typeof obj.uri === 'string') {
    const start = (obj.range as { start?: { line?: number; character?: number } } | undefined)
      ?.start;
    return [
      {
        path: uriToPath(obj.uri),
        line: (start?.line ?? 0) + 1,
        column: (start?.character ?? 0) + 1,
      },
    ];
  }
  return [];
}

// ── invoke 包装（仅在 Tauri 桌面有效；web-remote 经 invoke-request 桥同样可达）──

export async function lspDidOpen(
  workspaceRoot: string,
  path: string,
  text: string
): Promise<void> {
  const languageId = lspLanguageId(path);
  if (!languageId) return;
  try {
    await invoke('lsp_did_open', {
      workspaceRoot,
      uri: pathToUri(path),
      languageId,
      text,
    });
  } catch (err) {
    console.warn('[lsp] did_open failed', path, err);
    notifyLspError(err);
  }
}

export async function lspDidChange(
  workspaceRoot: string,
  path: string,
  version: number,
  text: string
): Promise<void> {
  if (!lspSupports(path)) return;
  try {
    await invoke('lsp_did_change', {
      workspaceRoot,
      uri: pathToUri(path),
      version,
      text,
    });
  } catch (err) {
    console.warn('[lsp] did_change failed', path, err);
    notifyLspError(err);
  }
}

/** go-to-definition：line/character 为 0-based（LSP 口径，调用方从 Monaco 1-based 转）。 */
export async function lspDefinition(
  workspaceRoot: string,
  path: string,
  line: number,
  character: number
): Promise<LspTarget[]> {
  if (!lspSupports(path)) return [];
  try {
    const raw = await invoke<unknown>('lsp_definition', {
      workspaceRoot,
      uri: pathToUri(path),
      line,
      character,
    });
    return parseDefinition(raw);
  } catch (err) {
    console.warn('[lsp] definition failed', path, err);
    notifyLspError(err);
    return [];
  }
}

/** Find All References（P3）：返回引用位置（Location[] → LspTarget[]，复用 parseDefinition）。 */
export async function lspReferences(
  workspaceRoot: string,
  path: string,
  line: number,
  character: number
): Promise<LspTarget[]> {
  if (!lspSupports(path)) return [];
  try {
    const raw = await invoke<unknown>('lsp_references', {
      workspaceRoot,
      uri: pathToUri(path),
      line,
      character,
    });
    return parseDefinition(raw);
  } catch (err) {
    console.warn('[lsp] references failed', path, err);
    notifyLspError(err);
    return [];
  }
}

// ── Hover（P2）────────────────────────────────────────────────────────────────

export interface LspHover {
  /** 渲染好的 Markdown（签名 / 文档）。 */
  markdown: string;
}

/** 把 LSP Hover.contents（MarkupContent | MarkedString | MarkedString[]）拍平为 Markdown。 */
function hoverContentsToMarkdown(contents: unknown): string {
  if (contents == null) return '';
  if (typeof contents === 'string') return contents;
  if (Array.isArray(contents)) {
    return contents.map(hoverContentsToMarkdown).filter(Boolean).join('\n\n---\n\n');
  }
  if (typeof contents === 'object') {
    const o = contents as Record<string, unknown>;
    if (typeof o.value === 'string') {
      // MarkedString { language, value } → 代码围栏；MarkupContent { kind, value } → 原值。
      return typeof o.language === 'string'
        ? `\`\`\`${o.language}\n${o.value}\n\`\`\``
        : o.value;
    }
  }
  return '';
}

/** 解析 LSP Hover 原始结果（{ contents, range? } | null）。 */
export function parseHover(raw: unknown): LspHover | null {
  if (!raw || typeof raw !== 'object') return null;
  const obj = raw as Record<string, unknown>;
  if ('__lsp_error' in obj) return null;
  const md = hoverContentsToMarkdown(obj.contents).trim();
  return md ? { markdown: md } : null;
}

export async function lspHover(
  workspaceRoot: string,
  path: string,
  line: number,
  character: number
): Promise<LspHover | null> {
  if (!lspSupports(path)) return null;
  try {
    const raw = await invoke<unknown>('lsp_hover', {
      workspaceRoot,
      uri: pathToUri(path),
      line,
      character,
    });
    return parseHover(raw);
  } catch (err) {
    console.warn('[lsp] hover failed', path, err);
    notifyLspError(err);
    return null;
  }
}

// ── 诊断（P2）：LSP host 经 Tauri event `lsp://diagnostics` 推送 ──────────────────

export interface LspDiagnostic {
  range: {
    start: { line: number; character: number };
    end: { line: number; character: number };
  };
  /** 1=Error 2=Warning 3=Information 4=Hint（LSP DiagnosticSeverity）。 */
  severity?: number;
  message: string;
  source?: string;
  code?: string | number;
}

export interface LspDiagnosticsPayload {
  uri: string;
  diagnostics: LspDiagnostic[];
}

/** 订阅 LSP 诊断推送。回调收到 {uri, diagnostics}；调用方据 uri 设置 Monaco markers。 */
export function onLspDiagnostics(
  cb: (payload: LspDiagnosticsPayload) => void
): Promise<UnlistenFn> {
  return listen<LspDiagnosticsPayload>('lsp://diagnostics', (e) => cb(e.payload));
}
