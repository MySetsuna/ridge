# 自定义主题背景图：工作区级渲染 + 卡片预览

> 2026-06-16。续 [2026-06-15-custom-theme-design.md]。本设计把已落地的"自定义主题背景图"
> 从**单 pane 局部**升级为**整工作区连续**，并在主题列表卡片上展示背景设置。

## 背景 / 现状

`ThemeEntry.bgImage`/`bgImageOpacity` 数据模型、`activeBgImage` store、保存/解析命令均已存在
（见上一版设计）。当前渲染路径有两处不足：

1. **背景图是单 pane 的**：`RidgePane.svelte` 在每个 pane 容器内渲染 `.rg-pane-bgimg`
   （`position:absolute; inset:0; background-size:cover`）。每个 pane 各自 `cover` 缩放一份
   → 分屏时是多份独立缩放的图，而非一张连续铺满工作区的图。
2. **主题列表卡片不展示背景设置**：`SettingsPanel.svelte` 的主题卡片只画 bg/surface/accent/fg
   四个合成色块，看不出某主题带不带壁纸。

"背景图在文字后边"这一点**已经可用**：`themeBridge.ts` 在 `activeBgImage.url != null` 时，把
推给 wasm 内核的 `background` 改成**alpha=0** 的 term-bg；而共享宿主画布的 swap-chain 是
`CompositeAlphaMode::PreMultiplied`（`surface_host.rs:159`）——默认底色单元渲染为透明，透出其
下方 DOM。**因此理论上无需改 term/Rust 代码**（待 CDP 真机核验；若核验发现透不出再改 webgpu.rs）。

## 架构关键事实

- **唯一共享宿主画布**：`+page.svelte:1866` 的 `<canvas data-rg-host>`，
  `position:absolute; inset:0; z-index:0; pointer-events:none`，挂在 pane 区包裹层
  （`+page.svelte:1832`，`relative flex-1 … flex flex-col`）里，在 `{#each 工作区}` 之后、
  即在所有 `SplitContainer` DOM **之上**绘制。
- pane 区内各层（自底向上、均在画布之下）：`splitpanes__pane`(relative,无 bg) →
  SplitContainer 卡片 div(无 bg) → `RidgePane .rg-pane-container`（**`background: var(--rg-term-bg)` 不透明**）。
  pane 容器的不透明 term-bg 是唯一会遮挡更下层的实色层。
- 画布透明像素"fall through to the canvas's CSS parents"（surface_host 注释）——即透出画布之下、
  包裹层之内绘制的内容。

## ⚠️ 验证结论（2026-06-16 CDP 真机）：Approach B 已被证伪

下面的 Approach B（纯 DOM/CSS）**真机验证不可行**，原因记录在此，供后续渲染器方案参考。

**实测**（`pnpm tauri:dev:cdp` + CDP 驱动，建带壁纸自定义主题并激活）：
- DOM 层叠完全正确：工作区图层为包裹层首子节点、画布之下；pane 容器 `background: transparent`；
  画布空白单元像素读出 `rgba(0,0,0,0)`（themeBridge 推 alpha=0 + premultiplied swap-chain 生效，
  渲染器**确实**输出透明默认底色）。
- **但**把 `html` / `body` / 画布父级 / 祖先节点的 `background-color`（红/绿）或 `background-image`
  逐一改掉，终端区域**毫无变化**，始终是窗口底色（wheat 主题的奶油色）。
- **结论**：WebView2 的 WebGPU 画布把透明像素合成在**固定的窗口/webview 基底色**之上，**不**与画布
  之下的 DOM 栈合成。⇒ **任何放在终端画布后方的 DOM 壁纸都无法透出**。既有的「单 pane 背景图」
  代码同理从未真正显示在文字后方。这正是用户预判的「纯 js/html 不能实现 → 改 term 代码」。

**已落地（本设计内已验证可行的部分）**：
- Req 1 主题卡片展示壁纸：`resolveThemeBgUrl` + SettingsPanel 卡片叠图，CDP 验证通过，commit `035f241`。
- 顺带修复文件搜索弹窗 z-index（`2d5cf59`，与本设计无关但同会话）。
- Approach B 的 `+page.svelte` / `RidgePane.svelte` 改动已 revert（证伪）。

## 后续独立任务：渲染器自绘壁纸（真正的 Req 2/3 修复）

term 渲染器（`packages/ridge-term`）需自己画壁纸——这是核心渲染器改动，单独一轮做：
1. **图片上传**：JS 端 `convertFileSrc` 加载图 → 离屏 canvas `getImageData` 取 RGBA → 传给 wasm
   （避免在 wasm 引入 image 解码 crate）。随 `activeBgImage` 变化推送/清除。
2. **GPU 纹理 + 管线**：上传 RGBA 为 `wgpu::Texture` + sampler/bind-group；新增「贴图四边形」管线
   （或扩展 cell 管线），在**透明 clear 之后、cell 背景之前**画一张铺满**共享宿主画布**的贴图四边形。
3. **工作区连续**：贴图 UV 映射到**整个宿主画布**而非单 pane；每 pane 的 scissor 内按其在宿主画布
   的偏移采样同一张图 → 天然连续铺满整个工作区。
4. **文字浮于其上**：默认底色单元已是 alpha=0（themeBridge）→ 壁纸透出；字形/带色底色不透明压在上面。
5. **不透明度**：uniform；**Canvas2D 回退**：用 `drawImage` 画同一张图（或显式不支持，`log` 告知）。
6. 改后 `wasm-pack build` 重编 + CDP 真机复验（连续铺满 / 文字清晰 / 分屏仍一张 / resize 不错位）。

---

## ~~方案（Approach B：单一工作区图层 + 活动时 pane 背景透明）~~（已证伪，保留备查）

绘制顺序（CSS 2.1 Appendix E 第 6 步，定位元素按 tree order）：包裹层第一个子节点的图层
→ 各 relative pane → 共享画布。即 **图 → pane（透明）→ 画布**。

### Task 1 — `src/lib/stores/themes.ts`
抽出共享解析器，消除第 3 份"拼目录 + convertFileSrc"重复：
```ts
export async function resolveThemeBgUrl(t: ThemeEntry | undefined): Promise<string | null>
```
`setActiveBgImage` 改为复用它。（`SettingsPanel`/`CustomThemeModal` 亦可复用。）

### Task 2 — `src/routes/+page.svelte`
在 pane 区包裹层（1832）内、`{#each}` 之前，新增**唯一**工作区背景层：
```svelte
{#if $activeBgImage.url}
  <div class="rg-workspace-bgimg"
       style="background-image:url('{$activeBgImage.url}'); opacity:{$activeBgImage.opacity};"
       aria-hidden="true"></div>
{/if}
```
CSS：`position:absolute; inset:0; z-index:0; pointer-events:none; background:center/cover no-repeat`。
作为第一个子节点 → 绘制在所有 pane 之下、画布之下；`opacity` 与其后的 `--rg-bg-raised` 混合。

### Task 3 — `src/lib/components/RidgePane.svelte`
- 删除 `.rg-pane-bgimg` 块（DOM + `<style>` 规则）。
- pane 容器背景在背景图激活时置透明，否则保持 term-bg：
  `style="background: {$activeBgImage.url ? 'transparent' : 'var(--rg-term-bg)'}; …"`
  （`activeBgImage` 已 import）。这样透明画布单元 → 透明 pane → 透出工作区图层。
- pane 上的覆盖层（滚动条/跳转按钮/搜索条，z-index 10–21）位置不变，仍在图层之上、按既有
  方式透过画布透明区可见——无回归。

### Task 4 — `src/lib/components/SettingsPanel.svelte`
- `themePreview[id]` 扩展 `bgUrl: string|null`（对有 `bgImage` 的主题用 `resolveThemeBgUrl`
  异步解析，桌面端；存到一个 `$state` map，`$effect` 里填充）+ `bgOpacity`。
- 卡片预览条（`h-16`）底层叠加一张 `background-image` 图（带主题 opacity），色块浮于其上 →
  一眼可见"该主题带壁纸"。
- 带 `bgImage` 的卡片右上角加一个 `ImageIcon` 角标。

### Task 5 — 验证
- `pnpm check`（svelte 0/0）、`pnpm test`（vitest）、`customTheme.test.ts` 不回归。
- `pnpm tauri:dev:cdp` 真机：建带壁纸自定义主题 → 切换 → 截图确认：①整工作区一张连续图；
  ②文字清晰浮于图上（透明默认底色透出图）；③分屏后仍是同一张连续图（非每 pane 一份）；
  ④主题卡片显示壁纸 + 角标。若文字后透不出图，则改 `webgpu.rs` `clear()`/`draw_row_backgrounds`：
  背景图激活标志置位时，默认底色单元写 alpha=0（当前依赖 themeBridge 的 alpha-0 已应足够）。

## 取舍 / 边界

- **Canvas2D 回退**（无 WebGPU host）：每 pane 自有不透明画布会盖住工作区图层 → 该回退下背景图
  不显示。与旧单 pane 方案在 Canvas2D 下同样不可见，**无回归**；WebGPU 为默认路径。
- **远控/移动端**：`bgImage` 经 `convertFileSrc` + `theme-assets/`，仅桌面（Tauri）可解析；
  远控 host 无 fs 时 `resolveThemeBgUrl` 返回 null → 自然 no-op，不在本次范围。
- **不改 Rust**（除非 Task 5 核验为必需）：复用既有 alpha-0 + premultiplied 通路。
