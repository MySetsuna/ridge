import { writable, get } from 'svelte/store';
import { invoke, convertFileSrc } from '@tauri-apps/api/core';

// Splash loader contract. `primary` / `secondary` are required and feed
// the SVG stroke and accent fill. Everything else is optional — fields
// missing from a theme fall back to the hardcoded defaults baked into
// `src/app.html`'s CSS variables (no per-theme value = current visual).
//
// Numbers are interpreted as: pixel lengths for *Width / *Radius,
// milliseconds for *DurationMs / *DelayMs, raw scalars for opacities and
// the breathe-scale knob.
export interface LoaderConfig {
  primary: string;
  secondary: string;
  bg?: string;
  accentGlow?: string;
  strokeWidth?: number;
  cornerRadius?: number;
  drawDurationMs?: number;
  breatheDurationMs?: number;
  crossDelayMs?: number;
  fadeOutDurationMs?: number;
  fillOpacityPrimary?: number;
  fillOpacitySecondary?: number;
}

export interface ThemeEntry {
  id: string;
  label: string;
  type: 'dark' | 'light';
  loader: LoaderConfig;
  colors: Record<string, string>;
  bgImage?: string;        // theme-assets/ 下的文件名
  bgImageOpacity?: number; // 0..1，缺省视为 1
}

export interface ThemeFile {
  version: number;
  themes: ThemeEntry[];
}

const store = writable<ThemeFile>({ version: 1, themes: [] });

export const themeData = { subscribe: store.subscribe };

export function getThemeIds(): string[] {
  return get(store).themes.map(t => t.id);
}

export function getThemeLabels(): Record<string, string> {
  const out: Record<string, string> = {};
  for (const t of get(store).themes) {
    out[t.id] = t.label;
  }
  return out;
}

export function getTheme(id: string): ThemeEntry | undefined {
  return get(store).themes.find(t => t.id === id);
}

let _resolved = false;

export async function initThemeSystem(): Promise<void> {
  if (_resolved) return;
  try {
    const tf = await invoke<ThemeFile>('get_theme_data');
    store.set(tf);
    _resolved = true;
  } catch (e) {
    // reduced-capability host（无头 cli host / 精简 cloud host）不实现 get_theme_data。
    // 历史上这里 re-throw，会令 +page.svelte 启动 IIFE 在第一行整体中断 —— 后续
    // refreshWorkspaces / ensureActiveWorkspace 全被跳过，控制端永远停在「请先选择一个
    // 工作区」、终端不渲染。改为降级：保留默认（空）主题集继续启动，applyTheme 对未知
    // 主题 no-op、终端回退 CSS 默认色。不置 _resolved —— 留待将来连到支持主题的 host 重试。
    console.warn('initThemeSystem: get_theme_data 不可用（reduced-capability host），降级默认主题继续启动', e);
  }
}

/** 自定义主题 id 前缀（与 ridge-core CUSTOM_ID_PREFIX 对齐）。 */
export const CUSTOM_ID_PREFIX = 'custom-';

/** 是否自定义主题（可编辑/删除）。 */
export function isCustomTheme(id: string): boolean {
  return id.startsWith(CUSTOM_ID_PREFIX);
}

/** 由 label 生成 `custom-` 前缀 id（与后端规则一致，前端仅用于预测；最终以后端返回为准）。 */
export function slugifyThemeId(label: string): string {
  let slug = '';
  let prevDash = false;
  for (const ch of label.trim()) {
    if (/\p{L}|\p{N}/u.test(ch)) {
      slug += ch.toLowerCase();
      prevDash = false;
    } else if (!prevDash) {
      slug += '-';
      prevDash = true;
    }
  }
  slug = slug.replace(/^-+|-+$/g, '');
  return CUSTOM_ID_PREFIX + (slug || 'theme');
}

/** 重新从后端拉取合并后的主题目录（含用户主题）。 */
export async function refreshThemes(): Promise<void> {
  try {
    const tf = await invoke<ThemeFile>('get_theme_data');
    store.set(tf);
  } catch (e) {
    console.warn('refreshThemes failed', e);
  }
}

/** 保存（新增/编辑）自定义主题，返回后端规整后的 entry，并刷新 store。 */
export async function saveCustomTheme(entry: ThemeEntry): Promise<ThemeEntry> {
  const saved = await invoke<ThemeEntry>('save_user_theme', { entry });
  await refreshThemes();
  return saved;
}

/** 删除自定义主题并刷新 store。 */
export async function deleteCustomTheme(id: string): Promise<void> {
  await invoke('delete_user_theme', { id });
  await refreshThemes();
}

/** 把图片字节存到 theme-assets/，返回文件名。 */
export async function saveThemeBgImage(bytes: Uint8Array, ext: string): Promise<string> {
  return invoke<string>('save_theme_bg_image', { bytes: Array.from(bytes), ext });
}

// ── 活动主题背景图信号 ──────────────────────────────────────────────
export interface ActiveBgImage {
  url: string | null;     // convertFileSrc 后的可加载 URL
  opacity: number;        // 0..1
}
const bgImageStore = writable<ActiveBgImage>({ url: null, opacity: 1 });
export const activeBgImage = { subscribe: bgImageStore.subscribe };

// 三态：undefined = 未尝试，null = 已失败（永久跳过），string = 已缓存
let _assetsDir: string | null | undefined = undefined;
async function assetsDir(): Promise<string | null> {
  if (_assetsDir !== undefined) return _assetsDir;   // null 也命中，直接返回
  try {
    _assetsDir = await invoke<string>('get_theme_assets_dir');
  } catch {
    _assetsDir = null;   // 失败后永远跳过 invoke
  }
  return _assetsDir;
}

/** 解析某主题的背景图为可加载 URL，更新 activeBgImage 信号。fire-and-forget。 */
export async function setActiveBgImage(themeId: string): Promise<void> {
  const t = getTheme(themeId);
  if (!t || !t.bgImage) {
    bgImageStore.set({ url: null, opacity: 1 });
    return;
  }
  const dir = await assetsDir();
  if (!dir) { bgImageStore.set({ url: null, opacity: t.bgImageOpacity ?? 1 }); return; }
  const cleanDir = dir.replace(/[\\/]+$/, '');
  const sep = cleanDir.includes('\\') ? '\\' : '/';
  const url = convertFileSrc(`${cleanDir}${sep}${t.bgImage}`);
  bgImageStore.set({ url, opacity: t.bgImageOpacity ?? 1 });
}
