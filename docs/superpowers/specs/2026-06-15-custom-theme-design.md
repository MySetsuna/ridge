# 自定义主题（内置主题编辑器）设计

- 日期：2026-06-15
- 状态：待评审
- 作者：Claude（与 Jack 协作）

## 1. 背景与目标

设置面板「外观」页目前只能从内置的 8 套主题里**选择**，无法新建。本设计在该页新增一个内置的「自定义主题」入口卡片，点击后弹出一个大编辑弹窗，让用户：

- 给主题**起名字**；
- 设置**背景图片**与**背景图透明度**（仅作用于终端区域）；
- 调整**全部主题配置项**（核心 UI 色常驻、ANSI 终端色与 loader 启动动画色折叠进阶）；
- **实时预览**编辑效果；
- **保存**为以用户命名的主题，出现在主题列表里与内置主题并列；
- 对已建的自定义主题**二次编辑 / 删除**。

### 非目标（YAGNI）

- 主题导入 / 导出（文件互传）。
- 每个 pane 用不同背景图。
- splash 启动画面背景图（splash 仍只用 `bg` 颜色）。
- 整窗背景图（本期只做**终端区域**背景）。
- 远控端（移动端 / web-remote / cloud）的背景图同步——远控只继续同步颜色，背景图是桌面本地特性。

## 2. 现状梳理（关键事实）

- **主题目录** `ridge.theme`：随包分发，位于 exe 同级（dev 时是仓库根）。经 `get_theme_data` 读取，**对发布版只读**（发布版在 `C:\Program Files\ridge`，普通权限写不了）。
- **当前主题** id 持久化在 `<LOCALAPPDATA>/ridge/active-theme.txt`（`set_active_theme`），与 `ridge.theme` 分离。
- **主题应用** `applyTheme(id)`（`src/lib/stores/settings.ts`）：把 `theme.colors` 的每个键写成 `--rg-{key}` CSS 变量。
- **配置项**：~18 个核心色（bg / bg-raised / surface / surface-2 / glass / border / border-bright / fg / fg-muted / accent / accent-glow / term-bg / tui-bg / scrollbar / scrollbar-hover / title-proc / title-sep / title-cwd）+ 可选 16 个 `ansi-*` 终端色 + `loader` 启动动画色（primary / secondary + 若干可选数值旋钮）。部分核心色是 rgba（glass / border / border-bright / accent-glow / scrollbar / scrollbar-hover）。
- **终端渲染链路（背景图可行性的核心）**：
  - 终端 surface 已配置 `CompositeAlphaMode::PreMultiplied`（`packages/ridge-term/src/render/surface_host.rs:159`）——透明像素合成时真的透明。
  - 默认 cell 背景"resolves to transparent"（`render/backend.rs:496`）——空白格本就透明。
  - 唯一不透明的满屏底块是 `clear()`（`render/webgpu.rs:473`），用 `theme.bg`，且 `rgba_u8_to_f32(bg_color)` **保留 alpha**。
  - `themeBridge.ts` 已把 `--rg-term-bg` 规整为带 alpha 的 hex8 喂给内核。
  - **结论：把 `term-bg` 喂成 alpha=0，满屏底块即透明，canvas 背后的 CSS 层（纯色底 + 背景图）透出——无需改动任何 Rust 渲染代码。**
- `src/lib/utils/cssColor.ts` 已有 `hex8WithAlpha(input, alpha)`（line 85），可直接用于改写 alpha。

## 3. 总体方案

分四块：数据/持久化（Rust + TS store）、终端背景图渲染（纯 CSS + themeBridge）、编辑弹窗组件、设置面板主题页改动。

### 3.1 数据模型与持久化

**TS 侧**（`src/lib/stores/themes.ts`）扩展 `ThemeEntry`：

```ts
export interface ThemeEntry {
  id: string;
  label: string;
  type: 'dark' | 'light';
  loader: LoaderConfig;
  colors: Record<string, string>;
  bgImage?: string;        // 资源文件名/ id（相对 theme-assets/），无图则缺省
  bgImageOpacity?: number; // 0..1，背景图透明度，缺省视为 1
}
```

> 是否「可编辑/删除」由前端按 **id 是否以 `custom-` 前缀**判定（自定义主题 id 强制带前缀），无需在数据模型里额外加 `builtin` 字段。

**Rust 侧**（`packages/ridge-core/src/commands/theme.rs` + `src-tauri` 薄封装）：

- `ThemeEntry` 增加 `bg_image: Option<String>`、`bg_image_opacity: Option<f32>`（`#[serde(rename_all)]` 对齐 camelCase：`bgImage` / `bgImageOpacity`），`#[serde(skip_serializing_if = "Option::is_none")]`。`builtin` 不入 Rust 模型，由合并逻辑在 TS/返回时标注（见下）。
- 新文件位置常量：`USER_THEMES_FILE = "user-themes.json"`，与 `active-theme.txt` 同在 `app_data_dir()`（`<LOCALAPPDATA>/ridge`）。`theme-assets/` 子目录存图片。
- **合并读取**：新增 `read_user_themes(app_data_dir) -> Vec<ThemeEntry>`（文件不存在/解析失败 → 空 Vec，绝不让启动失败）。`get_theme_data` 在返回前把用户主题**追加**到内置主题之后；id 撞车时用户主题让位（理论上不会，因 id 强制 `custom-` 前缀）。`active_theme_entry()`（远控推送用）同样走合并集，这样选中的自定义主题也能被远控解析其颜色——但**推送前剥离 `bg_image`**（远控不需要、避免传无意义字段）。
- 新命令（src-tauri `#[tauri::command]`，逻辑下沉 ridge-core 便于单测）：
  - `save_user_theme(entry: ThemeEntry) -> Result<ThemeEntry, String>`：校验 → 规整 id（`custom-<slug(label)>`，与现有 id 去重，必要时追加数字后缀）→ upsert 进 `user-themes.json`（按 id 覆盖即「编辑」，不存在即「新增」）→ 返回最终落盘的 entry。
  - `delete_user_theme(id: String) -> Result<(), String>`：从 `user-themes.json` 移除该 id；连带删除其 `theme-assets/` 图片（best-effort）。
  - `save_theme_bg_image(bytes: Vec<u8>, ext: String) -> Result<String, String>`：把图片写入 `theme-assets/<uuid>.<ext>`，返回文件名。校验扩展名白名单（png/jpg/jpeg/webp/gif）与体积上限（如 ≤ 20MB）。
- **图片加载**：前端用 `@tauri-apps/api/core` 的 `convertFileSrc(<theme-assets 绝对路径>)` 得到 webview 可加载的 URL。需要把 `theme-assets` 目录加入 asset protocol 作用域（`tauri.conf.json` 的 `app.security.assetProtocol.scope`，或等效的 `$APPLOCALDATA/ridge/theme-assets/**`）。新增小命令 `get_theme_assets_dir() -> String` 返回绝对路径供前端拼路径 + `convertFileSrc`。

### 3.2 终端背景图渲染（无 Rust 渲染改动）

当**当前活动主题**含 `bgImage` 时：

1. **CSS 分层**（`src/lib/components/RidgePane.svelte` 容器，现为 `style="background: var(--rg-term-bg)"`）：
   - 容器 `background-color: var(--rg-term-bg)`（纯色底，保持不透明基准色，保证可读性）。
   - 在容器内、canvas 之下插一层背景图元素 `.rg-pane-bgimg`：`position:absolute; inset:0; background-image:url(<convertFileSrc>); background-size:cover; background-position:center; opacity: <bgImageOpacity>; pointer-events:none;`。
   - canvas 仍在最上层，靠 PreMultiplied 透明合成把背后两层透出。
2. **themeBridge 改写**（`src/lib/terminal/themeBridge.ts` `readRidgeTheme`）：活动主题含 `bgImage` 时，把 `out.background`（来自 `--rg-term-bg`）的 alpha 改为 0（`hex8WithAlpha(bg, 0)`），使 `clear()` 满屏底块透明。`out.cursorAccent` 改用**去 alpha 的实底色**（`#rrggbbff`），避免光标块上的字形变透明。其余键不变。
   - 活动主题需让 bridge 拿得到 `bgImage` 标志：在 `applyTheme` 时把当前主题的 `bgImage`/`bgImageOpacity` 暴露为一个轻量信号（如挂到 `<html>` 的 `data-rg-bgimg` 属性或一个独立可订阅 store），bridge 读它决定是否清 alpha。RidgePane 同源读取该信号渲染图片层。
3. **旋钮语义**：`背景图透明度 = bgImageOpacity`（图片层 opacity）。低 → 纯色底为主、更可读；高 → 图片为主。无 `bgImage` 时 `clear()` 仍用不透明 term-bg，行为与现状**完全一致**。

### 3.3 编辑弹窗 `CustomThemeModal.svelte`（新组件）

- **布局**：大弹窗 ~920×640，`max-w/max-h` 自适应，z-index 9996（高于 SettingsPanel 9994、低于 ContextMenu 9999）。左列滚动表单，右列实时预览。ESC / 点遮罩关闭（有未保存改动时二次确认）。
- **表单字段**：
  - 主题名（label，必填，去空白；空或纯空白禁用保存）。
  - 类型 `type`（深 / 浅，分段切换）。
  - 「基于」选择：下拉选一个现有主题，克隆其 `colors`/`loader` 作为编辑起点（新建时默认 `endless-dark`）。
  - 背景图：选择按钮（`@tauri-apps/plugin-dialog` `open` 取图片）→ 读字节 → `save_theme_bg_image` → 得文件名；展示缩略图 + 「移除」；透明度滑块（0–100%）。
  - 核心 18 色：常驻取色器网格。每色一个混合控件 = 原生 `<input type=color>`（hex 部分）+ 仅对 rgba 类色显示 alpha 滑块；并配一个文本输入兜底（可直接粘 `rgba()`/`#rrggbbaa`）。
  - 进阶（可折叠 `<details>`，默认收起）：16 个 ANSI 终端色 + loader（primary/secondary + 可选数值旋钮）。未填的 ANSI/loader 项 → 继承「基于」主题或留空（与现状一致：dark 主题不写 ANSI 即用内核默认）。
- **实时预览（右列，scoped）**：一个容器内联写入 `--rg-*` 覆盖（**只作用于预览容器，不碰全局 `documentElement`**，故不会污染真实应用）。内容仿真：假标题栏 + 标签、侧栏色块、一个 accent 按钮、一块「终端」区域——终端区铺背景图（按当前 opacity）+ 示例多行文本（含若干 ANSI 前景色样例），文字不透明压在图上，直观体现可读性。
- **保存**：组装 `ThemeEntry`（id 由后端规整）→ `save_user_theme` → 刷新 themes store（重拉 `get_theme_data`）→ 关闭弹窗 → `setTheme(savedId)` 选中新主题。

### 3.4 设置面板主题页改动（`SettingsPanel.svelte` appearance 段）

- 主题网格末尾追加一张虚线 **「+ 自定义主题」** 创建卡 → 打开空白 `CustomThemeModal`。
- 自定义主题卡（id 以 `custom-` 开头）hover 显示右上角**编辑 / 删除**两个小按钮：
  - 编辑 → 以该主题预填打开弹窗，保存即按同 id 覆盖。
  - 删除 → 二次确认 → `delete_user_theme` → 刷新 store；若被删的是当前活动主题，回退 `setTheme('endless-dark')`。
- 弹窗以子组件形式挂在 SettingsPanel 内（或 +page 顶层），用本地 `open`/`editingId` 状态驱动。

## 4. 组件与边界（隔离设计）

| 单元 | 职责 | 依赖 | 接口 |
|---|---|---|---|
| ridge-core `theme.rs` | 读写 user-themes.json、合并目录、存图、规整 id | 文件系统 | `save_user_theme` / `delete_user_theme` / `save_theme_bg_image` / `get_theme_data`(合并) / `get_theme_assets_dir` |
| `stores/themes.ts` | 前端主题集与 CRUD 封装 | invoke | `saveCustomTheme` / `deleteCustomTheme` / `refresh` + 扩展 `ThemeEntry` |
| `stores/settings.ts` `applyTheme` | 应用色板 + 暴露 bgImage 信号 | themes store | 现有 + `data-rg-bgimg` 信号 |
| `themeBridge.ts` | CSS→内核，bgImage 时清 term-bg alpha | settings/bgimg 信号 | `readRidgeTheme`(改) |
| `RidgePane.svelte` | 渲染背景图层 | bgimg 信号 + convertFileSrc | 容器内 `.rg-pane-bgimg` |
| `CustomThemeModal.svelte` | 编辑/预览/保存 | themes store + dialog + invoke | `open`/`editingId`/`onClose` |
| `SettingsPanel.svelte` | 创建卡 + 编辑/删除入口 | 上面所有 | — |

## 5. 错误处理

- 后端所有读路径（合并、读 user-themes）失败 → 降级空集，**绝不阻塞启动**（沿用 `get_theme_data` 现有容错风格）。
- `save_user_theme`/`save_theme_bg_image` 失败 → 返回 `Err(String)`，弹窗内 toast/内联提示，不关闭弹窗、不丢用户输入。
- 图片体积/类型超限 → 后端拒绝并回明确错误文案。
- `convertFileSrc` 加载失败（文件被外部删）→ 背景图层静默不显示，回退纯色底。
- 删除当前活动主题 → 自动回退默认主题，避免「悬空主题」。

## 6. 测试

- **ridge-core 单测**（可独立跑，符合 `cargo test -p ridge-core` 约束）：
  - id 规整与去重（`custom-<slug>`，撞名追加后缀）。
  - user-themes.json upsert / delete round-trip（用 `SetEnvGuard` 重定向 `LOCALAPPDATA`，沿用现有测试范式）。
  - `get_theme_data` 合并顺序（内置在前、用户在后）与解析失败降级。
  - `save_theme_bg_image` 扩展名白名单 / 体积上限。
- **前端**：`hex8WithAlpha(bg, 0)` 在 bgImage 分支被调用、无 bgImage 时不调用（themeBridge 单测或快照）；slug 化与表单校验的纯函数单测。
- **e2e（可选，遵循"plan 不写手测清单"，此处仅列自动化）**：复用 `tests/e2e-shell/theme-*.spec.ts` 范式补一条——保存自定义主题后出现在列表并可选中。

## 7. 提交拆分（每个功能点单独 commit）

1. `feat(theme): ridge-core 用户主题持久化 + 目录合并 + 存图命令`（含单测）。
2. `feat(theme): themes/settings store 扩展 + 背景图信号`。
3. `feat(theme): 终端背景图渲染（RidgePane 图层 + themeBridge alpha）`。
4. `feat(theme): CustomThemeModal 编辑弹窗 + 实时预览`。
5. `feat(theme): 设置面板自定义主题创建卡 + 编辑/删除入口`。
6. `chore(theme): asset protocol scope + i18n 文案`（若未并入上面）。
