# 渲染器自绘工作区壁纸：term 内连续铺满 + 文字浮于其上

> 2026-06-18。续 [2026-06-16-workspace-wide-theme-background-design.md]。
> 那一版用 CDP 真机**证伪**了"画布后方任何 DOM 壁纸都能透出"，并把真正的修复
> 列为"后续独立任务：渲染器自绘壁纸"。本设计就是落地那个任务。

## 背景 / 现状

- 数据模型已存在：`ThemeEntry.bgImage` / `bgImageOpacity`、全局 `activeBgImage` store、
  `resolveThemeBgUrl` / `setActiveBgImage`（`themes.ts`），随 `applyTheme` 切换（`settings.ts`）。
  **壁纸跟随全局激活主题**（一张图），不是按工作区独立存储。
- themeBridge 已做对一半：`activeBgImage.url != null` 时把推给内核的 `background` 改成
  **alpha=0**（`themeBridge.ts:82`），让默认底色单元渲染为透明。
- 现有渲染是 `RidgePane.svelte:1761` 的 `.rg-pane-bgimg`（canvas 之下的 DOM `<div>`，
  `inset:0; background-size:cover`），**每个 pane 各画一份**。
- **关键缺陷**（2026-06-16 CDP 已证）：WebView2 的 WebGPU 画布把透明像素合成到**固定窗口基底色**
  上，**不**与画布下方 DOM 合成 → 这张 DOM 壁纸在默认 WebGPU 路径上**从未真正显示在文字后方**。

## 目标（本次确认）

1. **一张图连续铺满整个工作区**：分屏时多个 pane 拼成同一张连续大图（含 splitter gutter 缝隙也铺满），
   而非每个 pane 各自 `cover` 一份。仍跟随全局主题，数据模型不变。
2. **在 term 渲染器内实现**：壁纸由 GPU 画在**共享宿主画布**上、位于文字之下；文字/带色底色浮于其上、清晰可读。
3. **范围**：仅 WebGPU（默认路径）。Canvas2D 回退不支持壁纸（与现状一致，无回归）。

## 架构关键事实（已核实）

- **共享宿主画布**：`SurfaceHost`（`render/surface_host.rs`）每工作区一个 `wgpu::Surface`，绑 `<canvas data-rg-host>`。
  `begin_frame` 按需整屏 clear；`record_pane(scissor, pipeline, record)` 逐 pane 设 viewport+scissor 后画 cell。
  swap-chain `alpha_mode = PreMultiplied`、`desired_maximum_frame_latency = 1`（双缓冲）。
- **进程级 GPU 单例**：`GpuContext`（`render/gpu_context.rs`）持有 device/queue、cell 管线、字形 atlas、sampler。
  surface 格式 `Bgra8Unorm`（按 sRGB 字节语义、ROP 不做 gamma）。cell 管线用 `PREMULTIPLIED_ALPHA_BLENDING`。
- **§4b 增量模型**（`manager.ts` RAF 主循环）：每开一帧，**所有可见 pane 都记录一次 draw**——
  脏 pane 全量 `render()`（kernel 遍历 + cell 编码 + 上传 + draw），非脏 pane `recordCachedOnly()`
  （回放上一帧缓存的 CellInstance buffer，**不遍历 kernel**）。非脏 pane 的回放靠 per-pane scissor
  覆盖自己的区域，不依赖宿主 clear。
- **静止 backdrop 变更的既有模式**（`setTheme`，`manager.ts:3864`）：对每个 pane `invalidateAll()`
  （丢缓存 → 下一帧全量重读底色）+ `_invalidateHost()`（强制下一帧 `LoadOp::Clear` 让 gutter 重画）。

## ⚠️ 核心正确性规则（本设计的根基）

无壁纸时增量模型为何正确：默认底色单元是**不透明**的，回放时盖住 buffer 里的陈旧像素。
一旦启用壁纸，themeBridge 把默认底色推成 **alpha=0**（透明），回放**不再盖旧像素**。又因 swap-chain
是双缓冲（lat=1），壁纸若只在 `needs_initial_clear` 那一帧画，另一个 buffer 的非 clear 帧里、透明默认
单元下方会**透出陈旧像素 → 鬼影**；`opacity < 1` 时半透明壁纸下方同样透出陈旧内容。

**规则**：**壁纸激活时，`SurfaceHost::begin_frame` 每开一帧都画一个不透明全屏壁纸四边形，输出
`mix(主题底色, 图, opacity)`（alpha=1），顶替原有的 clear pass。** 该 quad 完全盖住陈旧像素，
故不需要独立 clear。pane 照旧每帧记录（透明默认单元保留壁纸、字形/带色底色压在上面）。

成本与取舍（已与用户确认可忽略）：
- 最贵的 kernel 遍历 / cell 编码由 **per-pane 脏状态**决定，与宿主 clear 解耦 → 规则①**不**让 pane 退回全量重编码。
- 空闲帧（无 pane 脏）宿主帧不开 → 额外开销为 0。
- 净额外成本 = **每渲染帧一个全屏贴图 quad**（4 顶点、一次 draw、每像素一次采样+混合），亚毫秒级。
- 唯一"损失"= 放弃 gutter 跨帧持久化（省一次 clear 的钱），可忽略。该渲染器 P1.1 优化前本就每帧 clear。
- 不开壁纸的主题：`begin_frame` 行为完全不变（仍只在 `needs_initial_clear` 时 clear），零影响。

## 数据流

```
主题切换 / 编辑壁纸 / 调 opacity
  → activeBgImage 信号变化 (themes.ts)
  → JS: convertFileSrc 加载图 → 离屏 canvas drawImage → getImageData 取 RGBA(straight)
  → manager.setWallpaper(rgba, w, h, opacity)  (URL=null 时 clearWallpaper())
  → _globalHostHandle().setWallpaper(...)  (worker 路径镜像 workerRendererBridge)
  → wasm: GpuContext::set_wallpaper(...) 上传 wgpu::Texture（处理 256 字节行对齐）
  → manager.invalidateAllPanes() + _invalidateHost()  （复用 setTheme 同款组合，立即重绘）
  → 下一帧起 SurfaceHost::begin_frame 每帧画壁纸 quad
```

## 组件与改动点

### JS 侧

**`src/lib/stores/themes.ts`**
- `activeBgImage` 解析路径扩展：URL 解析成功后，用离屏 `OffscreenCanvas`/`HTMLCanvasElement` 加载图并
  `getImageData` 取 `{rgba: Uint8ClampedArray, w, h}`（解码交给浏览器，wasm 不引入图片解码 crate）。
- 暴露一个让 manager 订阅的信号/回调，携带 `{rgba, w, h, opacity}` 或 `null`（清除）。

**`src/lib/terminal/manager.ts`**
- 新增 `setWallpaper(rgba, w, h, opacity)` / `clearWallpaper()`：转发到 `_globalHostHandle()`；
  worker 模式经 `workerRendererBridge` 镜像（与 `setFont`/`setTheme` 同样的双路径）。
- 调用后执行 `invalidateAllPanes()` + `_invalidateHost()`（立即重绘，无鬼影）。
- 订阅 `activeBgImage`（或 themes.ts 暴露的壁纸信号），在变化时调上面两个方法。

**`src/lib/terminal/themeBridge.ts`**：不改（现有 alpha=0 逻辑保留）。

**`src/lib/components/RidgePane.svelte`**：删除失效的 `.rg-pane-bgimg`（DOM 块 + `<style>` 规则 +
对 `activeBgImage` 的相关引用）。它在 WebGPU 路径从未生效。

### wasm 侧（`packages/ridge-term`）

**`render/gpu_context.rs`** — 新增壁纸资源（全局一张，随单例）：
- 字段：`wallpaper: Option<WallpaperTex>`（`{texture, view, img_w, img_h}`）、`wallpaper_opacity: f32`、
  `wallpaper_pipeline: wgpu::RenderPipeline`、`wallpaper_sampler`（linear + ClampToEdge）、
  `wallpaper_uniform: wgpu::Buffer`、`wallpaper_bgl` / 每次重建的 `wallpaper_bind_group`。
- `set_wallpaper(rgba, w, h, opacity)`：建 `Rgba8Unorm` 纹理并 `write_texture` 上传——**处理 256 字节
  `COPY_BYTES_PER_ROW_ALIGNMENT`**（`w*4` 非 256 倍数时按对齐行距重打包后再传），重建 bind group。
- `clear_wallpaper()`：置 `wallpaper = None`。
- 壁纸管线在 `GpuContext::new` 构建（含 shader 编译），与 cell 管线同期；目标格式 `SURFACE_FORMAT`，
  混合 `PREMULTIPLIED_ALPHA_BLENDING`（quad 输出预乘、alpha=1，等价不透明覆盖）。

**`render/shaders/wallpaper.wgsl`** — 新文件：
- 顶点：4 顶点三角带，用 `@builtin(vertex_index)` 生成 [-1,1] 全屏；输出 `uv`（按 cover 在片元算）。
- uniform：`canvas_w, canvas_h, img_w, img_h, opacity, bg_rgb`。
- 片元：按 `(canvas_aspect, img_aspect)` 做 **cover** UV 映射（等比铺满、裁切溢出），采样图色 `img`，
  输出 `vec4(mix(bg_rgb, img.rgb, opacity), 1.0)`（不透明，盖住陈旧像素）。

**`render/surface_host.rs`** — `begin_frame`：
- 壁纸激活（`ctx.wallpaper.is_some()`）时，**每帧**写 uniform（canvas 宽高来自 `self.config`、
  图片宽高/opacity/底色来自 ctx + `theme_bg`），用 ctx 的壁纸管线在整张宿主画布画一个全屏 quad，
  顶替原 clear pass（不再读 `needs_initial_clear`）。
- 未激活时维持现状（仅 `needs_initial_clear` 时 `LoadOp::Clear`）。
- 注意：壁纸 quad 用 `LoadOp::Load` 打开 pass 即可（quad 不透明、铺满整屏，等效 clear）。

**`render/gpu_context.rs` 或 `surface_host.rs`** — `set_wallpaper`/`clear_wallpaper` 调用后置
`needs_initial_clear`（无壁纸→有壁纸 / 换图 / 调 opacity 的首帧统一走重画）。

**`src/lib.rs` + `SurfaceHostHandle`（JS 侧类型）**：暴露 `set_wallpaper(rgba, w, h, opacity)` /
`clear_wallpaper()` 的 wasm-bindgen 封装（与 `resize` / `invalidate` 同款，内部 `host_rc().borrow_mut()`
转调 ctx）。

## 边界与取舍

- **opacity 语义**：壁纸在**主题底色**之上按 opacity 混合（不是窗口基底色）→ opacity 调低退回终端主题色，观感一致。
- **cover 缩放**：等比缩放铺满、裁掉溢出（同现有 DOM `background-size:cover`）。
- **resize / DPR**：`resize()` 已置 `needs_initial_clear`；壁纸每帧按 `config` 现尺寸重算 cover UV，天然不错位。
- **切工作区**：CSS 显隐、宿主 surface 不重配，壁纸纹理在 ctx 共享 → 无黑闪、无需重传。
- **gutter**：随规则①壁纸铺满整张宿主画布（含 splitter 缝隙），视觉更连续（用户已确认接受）。
- **Canvas2D 回退**：不支持，`set_wallpaper`/`clear_wallpaper` 在 Canvas2D 路径为 no-op；无回归。
- **远控 / 无 fs host**：`resolveThemeBgUrl` 返回 null → `clearWallpaper`，自然 no-op，不在范围内。

## 验证

- `cargo test --lib`：cover-UV 计算下沉为纯函数并单测（输入 canvas/img 宽高 → 期望 UV scale/offset）；
  256 行对齐重打包逻辑可单测。
- `pnpm check`（svelte 0/0）、`pnpm test`（vitest）不回归；`.rg-pane-bgimg` 删除后无悬挂引用。
- `wasm-pack build` 重编 + `pnpm tauri:dev:cdp` 真机复验：
  1. 建带壁纸自定义主题并激活 → 整工作区一张连续图、文字清晰浮于其上。
  2. 分屏 → 仍是同一张连续图（含 gutter 铺满），非每 pane 一份。
  3. 切主题 / 换图 / 调 opacity → 即时生效、无鬼影残留。
  4. resize 窗口 / 切 sidebar → 壁纸 cover 重算、不错位、不拉伸变形。
  5. 切到无壁纸主题 → 回到不透明底色，渲染与改动前一致（无回归、无性能退化）。
