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
