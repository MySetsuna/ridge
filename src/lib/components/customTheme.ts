// 自定义主题编辑器的纯逻辑：颜色键清单、scoped 预览 CSS、表单→ThemeEntry 组装。
import type { ThemeEntry } from '$lib/stores/themes';

/** 常驻取色器的 18 个核心 UI 色（顺序即展示顺序）。 */
export const CORE_COLOR_KEYS = [
  'bg', 'bg-raised', 'surface', 'surface-2', 'glass',
  'border', 'border-bright', 'fg', 'fg-muted', 'accent',
  'accent-glow', 'term-bg', 'tui-bg', 'scrollbar', 'scrollbar-hover',
  'title-proc', 'title-sep', 'title-cwd',
] as const;

/** 进阶区的 16 个 ANSI 终端色。 */
export const ANSI_COLOR_KEYS = [
  'ansi-black', 'ansi-red', 'ansi-green', 'ansi-yellow',
  'ansi-blue', 'ansi-magenta', 'ansi-cyan', 'ansi-white',
  'ansi-brightBlack', 'ansi-brightRed', 'ansi-brightGreen', 'ansi-brightYellow',
  'ansi-brightBlue', 'ansi-brightMagenta', 'ansi-brightCyan', 'ansi-brightWhite',
] as const;

/** 这些键惯用 rgba（取色器旁显示 alpha 滑块）。 */
export const ALPHA_COLOR_KEYS = new Set<string>([
  'glass', 'border', 'border-bright', 'accent-glow', 'scrollbar', 'scrollbar-hover',
]);

/**
 * 核心 18 色按语义分组，供编辑器分区展示（替代一坨技术键名的密网格）。
 * `titleKey` 指向 i18n `customTheme.*` 的分组标题；`keys` 之和恰为 CORE_COLOR_KEYS。
 */
export const CORE_COLOR_GROUPS: ReadonlyArray<{ titleKey: string; keys: readonly string[] }> = [
  { titleKey: 'grpSurface', keys: ['bg', 'bg-raised', 'surface', 'surface-2', 'glass'] },
  { titleKey: 'grpLine', keys: ['border', 'border-bright', 'scrollbar', 'scrollbar-hover'] },
  { titleKey: 'grpText', keys: ['fg', 'fg-muted'] },
  { titleKey: 'grpAccent', keys: ['accent', 'accent-glow'] },
  { titleKey: 'grpTerminal', keys: ['term-bg', 'tui-bg'] },
  { titleKey: 'grpTitlebar', keys: ['title-proc', 'title-sep', 'title-cwd'] },
] as const;

/** 人类可读的色名（展示用；缺省回退到原始键）。 */
export const COLOR_LABEL: Record<string, string> = {
  'bg': '窗口底色', 'bg-raised': '面板', 'surface': '表面', 'surface-2': '表面（深）', 'glass': '玻璃层',
  'border': '描边', 'border-bright': '亮描边', 'scrollbar': '滚动条', 'scrollbar-hover': '滚动条悬停',
  'fg': '文字', 'fg-muted': '次要文字', 'accent': '强调色', 'accent-glow': '强调光晕',
  'term-bg': '终端底色', 'tui-bg': 'TUI 底色', 'title-proc': '进程名', 'title-sep': '分隔符', 'title-cwd': '路径',
};

/** 把 colors map 渲染成 scoped `--rg-*` 行内样式（仅作用于预览容器）。 */
export function previewStyle(colors: Record<string, string>): string {
  return Object.entries(colors)
    .map(([k, v]) => `--rg-${k}: ${v};`)
    .join(' ');
}

export interface ThemeFormState {
  id: string;
  label: string;
  type: 'dark' | 'light';
  colors: Record<string, string>;
  loaderPrimary: string;
  loaderSecondary: string;
  bgImage?: string;
  bgImageOpacity: number;
}

/** 表单 → 可保存的 ThemeEntry（id 留空交后端规整）。 */
export function buildThemeEntry(f: ThemeFormState): ThemeEntry {
  return {
    id: f.id,
    label: f.label.trim(),
    type: f.type,
    loader: { primary: f.loaderPrimary, secondary: f.loaderSecondary },
    colors: { ...f.colors },
    ...(f.bgImage ? { bgImage: f.bgImage } : {}),
    ...(f.bgImage ? { bgImageOpacity: f.bgImageOpacity } : {}),
  };
}
