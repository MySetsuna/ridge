# 渲染器自绘工作区壁纸（WebGPU）Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在 WebGPU 默认路径上，由 term 渲染器在共享宿主画布上自绘一张连续铺满整个工作区的壁纸，位于文字/带色底色之下且文字清晰浮于其上，替换从未在 WebGPU 路径生效的 DOM `.rg-pane-bgimg` 方案。

**Architecture:** 壁纸纹理作为进程级 `GpuContext` 单例资源（一张图，所有 pane/workspace 共享）。`SurfaceHost::begin_frame` 在壁纸激活时每帧画一个不透明全屏 quad（`mix(主题底色, 图, opacity)`，alpha=1）顶替原 `LoadOp::Clear`，盖住陈旧像素并铺满 gutter；pane 照旧每帧记录（themeBridge 已把默认底色推成 alpha=0，透明默认单元保留壁纸、字形压其上）。JS 侧用离屏 canvas 解码图为 RGBA 推给 wasm，沿用 `setTheme` 的 `invalidateAllPanes()` + `_invalidateHost()` 立即重绘组合。

**Tech Stack:** Rust + wgpu（wasm32，`feature = "webgpu"`）、WGSL、wasm-bindgen、TypeScript、Svelte 5。

## Global Constraints

- **范围仅 WebGPU 默认主线程 global host 路径**：`preferWebgpu=true` 且 `manager.globalHost !== null`。Canvas2D 回退、worker-renderer（`usingWorkerRenderer()`）路径**不在本次范围**，壁纸在这两条路径为 no-op，无回归。
- **壁纸是全局单张**：跟随全局激活主题（`activeBgImage`），不是每工作区/每 pane 独立存储。数据模型不变（`ThemeEntry.bgImage` / `bgImageOpacity`）。
- **surface 格式** `wgpu::TextureFormat::Bgra8Unorm`（`SURFACE_FORMAT` / `CANVAS_FORMAT`）；壁纸管线 blend 用 `wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING`，quad 输出 alpha=1（等价不透明覆盖）。
- **256 字节行对齐**：`write_texture` 必须满足 `wgpu::COPY_BYTES_PER_ROW_ALIGNMENT`（256）。`img_w * 4` 非 256 倍数时按对齐行距重打包后再传。
- **opacity 语义**：壁纸在**主题底色**（`theme_bg.rgb`，忽略其 alpha）之上按 opacity 混合，不是窗口基底色。
- **cover 缩放**：等比缩放铺满、裁掉溢出（同现有 DOM `background-size:cover`）。
- **themeBridge 不改**：`activeBgImage.url != null` 时把内核 `background` 推成 alpha=0 的现有逻辑（`themeBridge.ts:82`）保留。
- **提交策略**：一个功能点一个 commit（用户偏好）。Commit message 用中文 type(scope) 前缀，沿用仓库既有风格（如 `feat(render):`）。

---

## File Structure

| 文件 | 责任 | 动作 |
|---|---|---|
| `packages/ridge-term/src/render/wallpaper.rs` | 壁纸**纯函数**（cover-UV 变换、256 行对齐重打包）+ 单测 | Create |
| `packages/ridge-term/src/render/shaders/wallpaper.wgsl` | 全屏 quad 顶点 + cover 采样 + opacity 混合片元 | Create |
| `packages/ridge-term/src/render/mod.rs` | 注册 `mod wallpaper;` | Modify |
| `packages/ridge-term/src/render/gpu_context.rs` | 壁纸纹理/管线/采样器/uniform 资源 + `set_wallpaper` / `clear_wallpaper` | Modify |
| `packages/ridge-term/src/render/surface_host.rs` | `begin_frame` 画壁纸 quad；`set_wallpaper` / `clear_wallpaper` 转发 + `needs_initial_clear` | Modify |
| `packages/ridge-term/src/lib.rs` | `SurfaceHostHandle` 暴露 `setWallpaper` / `clearWallpaper` wasm-bindgen 封装 | Modify |
| `src/lib/stores/themes.ts` | 解码激活主题壁纸为 RGBA，暴露 `activeWallpaper` 信号 | Modify |
| `src/lib/terminal/manager.ts` | `setWallpaper` / `clearWallpaper`（转发 `_globalHostHandle` + invalidate）+ 订阅信号 | Modify |
| `src/lib/components/RidgePane.svelte` | 删除失效的 `.rg-pane-bgimg` DOM + CSS + `activeBgImage` 引用 | Modify |

---

## Task 1: 壁纸纯函数（cover-UV 变换 + 256 行对齐重打包）

纯逻辑、零 wgpu 依赖，可在 host target 用 `cargo test --lib` 单测（TDD）。后续 Task 2/3 的 shader uniform 与纹理上传都消费这两个函数。

**Files:**
- Create: `packages/ridge-term/src/render/wallpaper.rs`
- Modify: `packages/ridge-term/src/render/mod.rs`（加 `pub mod wallpaper;`）

**Interfaces:**
- Produces:
  - `pub struct CoverUv { pub scale: [f32; 2], pub offset: [f32; 2] }`
  - `pub fn cover_uv_transform(canvas_w: u32, canvas_h: u32, img_w: u32, img_h: u32) -> CoverUv`
    片元用 `sample_uv = frag_uv * scale + offset` 把全屏 `[0,1]` 映射进图片，等比铺满、裁切溢出、居中。`img` 任一维为 0 时返回单位变换（`scale=[1,1] offset=[0,0]`）防除零。
  - `pub fn pack_rows_to_alignment(rgba: &[u8], width: u32, height: u32) -> (Vec<u8>, u32)`
    返回 `(bytes, bytes_per_row)`，`bytes_per_row` 向上取整到 256 的倍数。已对齐时原样返回（`bytes` 克隆、`bytes_per_row = width*4`）。

- [ ] **Step 1: 写失败测试**

在 `packages/ridge-term/src/render/wallpaper.rs` 写：

```rust
//! 壁纸渲染的纯逻辑（无 wgpu 依赖，host target 可单测）。
//! 资源/管线在 `gpu_context.rs`，每帧绘制在 `surface_host.rs`。

/// 全屏 quad 片元采样图片用的 UV 变换：`sample_uv = frag_uv * scale + offset`。
/// 等比铺满画布（cover）、裁切溢出、居中。
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct CoverUv {
    pub scale: [f32; 2],
    pub offset: [f32; 2],
}

/// 计算 cover 模式下的 UV 缩放/偏移。
///
/// 思路：以「画布纵横比 vs 图片纵横比」决定哪一维需要裁切。被裁的维度
/// `scale > 1`（采样范围 < [0,1]，等比放大），并用 `offset` 把可见窗口
/// 居中。另一维 `scale = 1, offset = 0`（铺满，不裁）。
pub fn cover_uv_transform(canvas_w: u32, canvas_h: u32, img_w: u32, img_h: u32) -> CoverUv {
    if img_w == 0 || img_h == 0 || canvas_w == 0 || canvas_h == 0 {
        return CoverUv { scale: [1.0, 1.0], offset: [0.0, 0.0] };
    }
    let canvas_aspect = canvas_w as f32 / canvas_h as f32;
    let img_aspect = img_w as f32 / img_h as f32;
    if canvas_aspect > img_aspect {
        // 画布更宽：横向铺满，纵向裁切。可见高度比例 = img_aspect / canvas_aspect。
        let visible = img_aspect / canvas_aspect; // < 1
        CoverUv { scale: [1.0, visible], offset: [0.0, (1.0 - visible) * 0.5] }
    } else {
        // 画布更高（或等比）：纵向铺满，横向裁切。
        let visible = canvas_aspect / img_aspect; // <= 1
        CoverUv { scale: [visible, 1.0], offset: [(1.0 - visible) * 0.5, 0.0] }
    }
}

/// 把紧凑 RGBA（`bytes_per_row = width*4`）重打包到 wgpu 要求的 256 字节
/// 行对齐。返回 `(bytes, bytes_per_row)`。已对齐则原样克隆返回。
pub fn pack_rows_to_alignment(rgba: &[u8], width: u32, height: u32) -> (Vec<u8>, u32) {
    let unpadded = width * 4;
    const ALIGN: u32 = 256; // wgpu::COPY_BYTES_PER_ROW_ALIGNMENT
    let padded = unpadded.div_ceil(ALIGN) * ALIGN;
    if padded == unpadded {
        return (rgba.to_vec(), unpadded);
    }
    let mut out = vec![0u8; (padded * height) as usize];
    for row in 0..height as usize {
        let src = row * unpadded as usize;
        let dst = row * padded as usize;
        out[dst..dst + unpadded as usize]
            .copy_from_slice(&rgba[src..src + unpadded as usize]);
    }
    (out, padded)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cover_square_image_in_wide_canvas_crops_vertically() {
        // 2:1 画布、1:1 图 → 横向铺满，纵向裁到 0.5、居中偏移 0.25。
        let uv = cover_uv_transform(200, 100, 100, 100);
        assert_eq!(uv.scale, [1.0, 0.5]);
        assert_eq!(uv.offset, [0.0, 0.25]);
    }

    #[test]
    fn cover_square_image_in_tall_canvas_crops_horizontally() {
        // 1:2 画布、1:1 图 → 纵向铺满，横向裁到 0.5、居中偏移 0.25。
        let uv = cover_uv_transform(100, 200, 100, 100);
        assert_eq!(uv.scale, [0.5, 1.0]);
        assert_eq!(uv.offset, [0.25, 0.0]);
    }

    #[test]
    fn cover_matching_aspect_is_identity() {
        let uv = cover_uv_transform(160, 90, 1600, 900);
        assert_eq!(uv.scale, [1.0, 1.0]);
        assert_eq!(uv.offset, [0.0, 0.0]);
    }

    #[test]
    fn cover_zero_image_is_identity() {
        let uv = cover_uv_transform(100, 100, 0, 0);
        assert_eq!(uv, CoverUv { scale: [1.0, 1.0], offset: [0.0, 0.0] });
    }

    #[test]
    fn pack_already_aligned_returns_unpadded() {
        // width=64 → 64*4=256，已对齐。
        let data = vec![7u8; 256 * 2];
        let (out, bpr) = pack_rows_to_alignment(&data, 64, 2);
        assert_eq!(bpr, 256);
        assert_eq!(out, data);
    }

    #[test]
    fn pack_unaligned_pads_each_row_to_256() {
        // width=10 → 40 字节/行，pad 到 256。2 行。
        let data = vec![9u8; 40 * 2];
        let (out, bpr) = pack_rows_to_alignment(&data, 10, 2);
        assert_eq!(bpr, 256);
        assert_eq!(out.len(), 256 * 2);
        // 每行前 40 字节是数据，其余是 0 填充。
        assert!(out[0..40].iter().all(|&b| b == 9));
        assert!(out[40..256].iter().all(|&b| b == 0));
        assert!(out[256..296].iter().all(|&b| b == 9));
    }
}
```

在 `packages/ridge-term/src/render/mod.rs` 顶部模块声明区加一行（与既有 `mod`/`pub mod` 并列）：

```rust
pub mod wallpaper;
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p ridge-term --lib wallpaper`
Expected: 编译失败 / 测试不存在（首次创建后应先因尚未加入 mod 树或断言写好而 FAIL → 加 `pub mod wallpaper;` 后转为 PASS）。若一次性写完则直接进入 Step 3 验证 PASS。

- [ ] **Step 3: 运行测试确认通过**

Run: `cargo test -p ridge-term --lib wallpaper`
Expected: PASS（6 个测试全绿）。

> 若 `-p ridge-term` 包名不符，改用 `cd packages/ridge-term && cargo test --lib wallpaper`。

- [ ] **Step 4: Commit**

```bash
git add packages/ridge-term/src/render/wallpaper.rs packages/ridge-term/src/render/mod.rs
git commit -m "feat(render): 壁纸纯函数 cover-UV 变换 + 256 行对齐重打包 + 单测"
```

---

## Task 2: WGSL shader + GpuContext 壁纸资源/管线/上传

新建壁纸 shader，并在进程级 `GpuContext` 单例上挂载壁纸纹理、采样器、uniform、管线，提供 `set_wallpaper` / `clear_wallpaper`。本任务交付物以**编译通过**为验证（GPU 绘制由 Task 8 真机验证）。

**Files:**
- Create: `packages/ridge-term/src/render/shaders/wallpaper.wgsl`
- Modify: `packages/ridge-term/src/render/gpu_context.rs`

**Interfaces:**
- Consumes: `super::wallpaper::{cover_uv_transform, pack_rows_to_alignment}`（Task 1）；`SURFACE_FORMAT`（gpu_context.rs:121）。
- Produces（`GpuContext` 新增）:
  - 字段 `pub wallpaper: Option<WallpaperTex>`、`pub wallpaper_opacity: f32`、`wallpaper_pipeline: wgpu::RenderPipeline`、`wallpaper_sampler: wgpu::Sampler`、`wallpaper_uniform: wgpu::Buffer`、`wallpaper_bgl: wgpu::BindGroupLayout`、`wallpaper_bind_group: Option<wgpu::BindGroup>`
  - `pub struct WallpaperTex { pub texture: wgpu::Texture, pub view: wgpu::TextureView, pub img_w: u32, pub img_h: u32 }`
  - `pub fn set_wallpaper(&mut self, rgba: &[u8], w: u32, h: u32, opacity: f32)`
  - `pub fn clear_wallpaper(&mut self)`
  - `pub fn has_wallpaper(&self) -> bool`
  - `pub const WALLPAPER_UNIFORM_SIZE: u64 = 32;`（uniform 字节数，供 buffer 创建与 `surface_host` 写入共用）

- [ ] **Step 1: 写 shader**

Create `packages/ridge-term/src/render/shaders/wallpaper.wgsl`:

```wgsl
// Ridge 工作区壁纸 shader。
// 全屏不透明 quad，铺满整张宿主画布、位于所有 pane pass 之前绘制（由
// surface_host.rs::begin_frame 顶替 clear pass 调用）。输出
// `mix(主题底色, 图, opacity)`，alpha 恒为 1，故完全盖住陈旧像素。
//
// cover 缩放（等比铺满、裁切、居中）由 Rust 侧 `cover_uv_transform` 纯函数
// 算出 scale/offset 经 uniform 传入；片元只做 `sample_uv = uv*scale+offset`。

struct WallpaperUniform {
    // UV 变换（cover）。
    uv_scale: vec2<f32>,
    uv_offset: vec2<f32>,
    // 主题底色 RGB（0..1）+ opacity。
    bg_rgb: vec3<f32>,
    opacity: f32,
};

@group(0) @binding(0) var<uniform> u: WallpaperUniform;
@group(0) @binding(1) var img_tex: texture_2d<f32>;
@group(0) @binding(2) var img_smp: sampler;

struct VertexOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

// (vertex_index in 0..4) → 全屏 TriangleStrip 角点，复用 cell.wgsl 的位移技巧。
// 角点顺序 (0,0)→(1,0)→(0,1)→(1,1)，uv 与之同向（顶左原点）。
fn corner_for(vid: u32) -> vec2<f32> {
    return vec2<f32>(f32(vid & 1u), f32((vid >> 1u) & 1u));
}

@vertex
fn vs_main(@builtin(vertex_index) vid: u32) -> VertexOut {
    let corner = corner_for(vid);
    var out: VertexOut;
    // [0,1] → NDC [-1,1]，y 翻转使 uv.y=0 落在画布顶部。
    out.clip = vec4<f32>(corner.x * 2.0 - 1.0, 1.0 - corner.y * 2.0, 0.0, 1.0);
    out.uv = corner;
    return out;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    let sample_uv = in.uv * u.uv_scale + u.uv_offset;
    let img = textureSample(img_tex, img_smp, sample_uv).rgb;
    let rgb = mix(u.bg_rgb, img, u.opacity);
    // alpha=1：不透明覆盖。PREMULTIPLIED blend 下 rgb 已是最终色（src.a=1）。
    return vec4<f32>(rgb, 1.0);
}
```

- [ ] **Step 2: 在 GpuContext 加字段**

在 `packages/ridge-term/src/render/gpu_context.rs` 的 `pub struct GpuContext { ... }`（约 124-... 行）内，紧接 `pub sampler: wgpu::Sampler,`（cell sampler，约 133 行）之后插入壁纸字段：

```rust
    // ── 工作区壁纸（全局一张，随进程单例）──────────────────────────
    /// 当前壁纸纹理；`None` = 未激活（begin_frame 退回普通 clear）。
    pub wallpaper: Option<WallpaperTex>,
    /// 壁纸在主题底色之上的混合不透明度，0..1。
    pub wallpaper_opacity: f32,
    /// 全屏 quad 管线（顶点无 buffer，4 顶点 TriangleStrip）。
    wallpaper_pipeline: wgpu::RenderPipeline,
    /// linear + ClampToEdge，cover 采样用。
    wallpaper_sampler: wgpu::Sampler,
    /// `WallpaperUniform`（32 字节）。每帧由 surface_host 写入。
    pub wallpaper_uniform: wgpu::Buffer,
    /// quad bind group layout（uniform + texture + sampler）。
    wallpaper_bgl: wgpu::BindGroupLayout,
    /// 随 `set_wallpaper` 重建；`None` 时不画。
    pub wallpaper_bind_group: Option<wgpu::BindGroup>,
```

并在文件中（`GpuContext` impl 之外，靠近 `SURFACE_FORMAT` 常量定义处）加结构体与常量：

```rust
/// `WallpaperUniform` 的字节大小（std140：vec2+vec2+vec3+f32，末尾 f32 与
/// vec3 同 16 字节槽对齐 → 2*8 + 16 = 32）。surface_host 写入时按此布局。
pub const WALLPAPER_UNIFORM_SIZE: u64 = 32;

/// 壁纸纹理 + 其原始像素尺寸（cover UV 计算用）。
pub struct WallpaperTex {
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub img_w: u32,
    pub img_h: u32,
}
```

- [ ] **Step 3: 在 GpuContext::new 构建壁纸管线/采样器/uniform/bgl**

在 `gpu_context.rs::new`（264 起）里，`cell_pipeline` 构建完、`sampler` 创建之后（约 459-460 行之后），插入壁纸资源构建（仿照 cell 管线写法，注意 quad 无顶点 buffer）：

```rust
        // ── 壁纸管线（全屏 quad）──────────────────────────────────
        let wallpaper_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("ridge-wallpaper-shader"),
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(include_str!(
                "shaders/wallpaper.wgsl"
            ))),
        });
        let wallpaper_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("ridge-wallpaper-bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let wallpaper_pl_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("ridge-wallpaper-pipeline-layout"),
            bind_group_layouts: &[&wallpaper_bgl],
            push_constant_ranges: &[],
        });
        let wallpaper_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("ridge-wallpaper-pipeline"),
            layout: Some(&wallpaper_pl_layout),
            vertex: wgpu::VertexState {
                module: &wallpaper_shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[], // 全屏 quad 由 vertex_index 生成，无顶点 buffer
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                unclipped_depth: false,
                polygon_mode: wgpu::PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &wallpaper_shader,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: SURFACE_FORMAT,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview: None,
            cache: None,
        });
        let wallpaper_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("ridge-wallpaper-sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        let wallpaper_uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("ridge-wallpaper-uniform"),
            size: WALLPAPER_UNIFORM_SIZE,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
```

然后在 `new()` 末尾构造 `Self { ... }` 的字段列表里（与 `sampler,` 并列）补上：

```rust
            wallpaper: None,
            wallpaper_opacity: 1.0,
            wallpaper_pipeline,
            wallpaper_sampler,
            wallpaper_uniform,
            wallpaper_bgl,
            wallpaper_bind_group: None,
```

- [ ] **Step 4: 加 set_wallpaper / clear_wallpaper / has_wallpaper**

在 `gpu_context.rs` 的 `impl GpuContext { ... }` 内（任意方法旁，如 `reset_frame_written` 附近）加：

```rust
    /// 上传/替换全局壁纸纹理。`rgba` 是 straight-alpha 紧凑像素
    /// （`bytes_per_row = w*4`，浏览器 `getImageData` 的布局）。处理 256
    /// 字节行对齐后 `write_texture`，并重建 bind group。
    pub fn set_wallpaper(&mut self, rgba: &[u8], w: u32, h: u32, opacity: f32) {
        if w == 0 || h == 0 {
            self.clear_wallpaper();
            return;
        }
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("ridge-wallpaper-tex"),
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let (packed, bytes_per_row) = super::wallpaper::pack_rows_to_alignment(rgba, w, h);
        self.queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &packed,
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(bytes_per_row),
                rows_per_image: Some(h),
            },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("ridge-wallpaper-bg"),
            layout: &self.wallpaper_bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.wallpaper_uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&self.wallpaper_sampler),
                },
            ],
        });
        self.wallpaper = Some(WallpaperTex { texture, view, img_w: w, img_h: h });
        self.wallpaper_opacity = opacity.clamp(0.0, 1.0);
        self.wallpaper_bind_group = Some(bind_group);
    }

    /// 关闭壁纸。begin_frame 退回普通 clear。
    pub fn clear_wallpaper(&mut self) {
        self.wallpaper = None;
        self.wallpaper_bind_group = None;
    }

    /// 壁纸是否激活（surface_host::begin_frame 据此决定画 quad 还是 clear）。
    pub fn has_wallpaper(&self) -> bool {
        self.wallpaper.is_some() && self.wallpaper_bind_group.is_some()
    }
```

- [ ] **Step 5: 编译验证**

Run: `cd packages/ridge-term && cargo build --features webgpu --target wasm32-unknown-unknown`
Expected: 编译通过（无 error）。若本机未装 wasm target，退而用 `cargo check --features webgpu`（host target 也应通过，wgpu 类型在 host 可用）。

- [ ] **Step 6: Commit**

```bash
git add packages/ridge-term/src/render/shaders/wallpaper.wgsl packages/ridge-term/src/render/gpu_context.rs
git commit -m "feat(render): GpuContext 壁纸纹理/管线/采样器资源 + set_wallpaper/clear_wallpaper"
```

---

## Task 3: SurfaceHost 每帧画壁纸 quad + 转发 set/clear

让 `begin_frame` 在壁纸激活时用全屏 quad 顶替 clear pass，并提供 `set_wallpaper`/`clear_wallpaper` 转发到 ctx 且置 `needs_initial_clear`。

**Files:**
- Modify: `packages/ridge-term/src/render/surface_host.rs`

**Interfaces:**
- Consumes: `ctx.has_wallpaper()`、`ctx.wallpaper`、`ctx.wallpaper_opacity`、`ctx.wallpaper_uniform`、`ctx.wallpaper_bind_group`、`WALLPAPER_UNIFORM_SIZE`、`super::wallpaper::cover_uv_transform`、`self.config.width/height`、`self.frame_clear_color`（主题底色）。
- Produces:
  - `pub fn set_wallpaper(&mut self, rgba: &[u8], w: u32, h: u32, opacity: f32)`
  - `pub fn clear_wallpaper(&mut self)`

- [ ] **Step 1: begin_frame 改为壁纸激活时画 quad**

在 `surface_host.rs::begin_frame`（251 起），把现有的 `if self.needs_initial_clear { ... clear pass ... }` 块（约 300-317 行）替换为「壁纸优先」逻辑。`self.frame_clear_color` 已在本函数开头由 `rgba_to_wgpu_color(theme_bg)` 设好（260 行）——壁纸激活时 `theme_bg` 是 `[bg_r,bg_g,bg_b,0]`，rgb 即主题底色：

```rust
        // 壁纸激活：每帧画一个不透明全屏 quad 顶替 clear。quad 输出
        // mix(主题底色, 图, opacity)、alpha=1，盖住陈旧像素 → 不需要也不
        // 依赖 needs_initial_clear（双缓冲下若只在 clear 帧画会出鬼影）。
        let ctx = self.ctx.borrow();
        if ctx.has_wallpaper() {
            if let (Some(tex), Some(bind_group)) =
                (ctx.wallpaper.as_ref(), ctx.wallpaper_bind_group.as_ref())
            {
                let uv = super::wallpaper::cover_uv_transform(
                    self.config.width,
                    self.config.height,
                    tex.img_w,
                    tex.img_h,
                );
                // WallpaperUniform std140 布局（32 字节）：
                //   uv_scale  : vec2<f32> @0
                //   uv_offset : vec2<f32> @8
                //   bg_rgb    : vec3<f32> @16
                //   opacity   : f32       @28
                let c = self.frame_clear_color; // 主题底色（rgb 有效，a 忽略）
                let mut buf = [0u8; WALLPAPER_UNIFORM_SIZE as usize];
                buf[0..4].copy_from_slice(&uv.scale[0].to_le_bytes());
                buf[4..8].copy_from_slice(&uv.scale[1].to_le_bytes());
                buf[8..12].copy_from_slice(&uv.offset[0].to_le_bytes());
                buf[12..16].copy_from_slice(&uv.offset[1].to_le_bytes());
                buf[16..20].copy_from_slice(&(c.r as f32).to_le_bytes());
                buf[20..24].copy_from_slice(&(c.g as f32).to_le_bytes());
                buf[24..28].copy_from_slice(&(c.b as f32).to_le_bytes());
                buf[28..32].copy_from_slice(&ctx.wallpaper_opacity.to_le_bytes());
                ctx.queue.write_buffer(&ctx.wallpaper_uniform, 0, &buf);

                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("ridge-host-wallpaper-pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        resolve_target: None,
                        // Load：quad 不透明铺满整屏，等效 clear。
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                });
                pass.set_pipeline(&ctx.wallpaper_pipeline);
                pass.set_bind_group(0, bind_group, &[]);
                pass.draw(0..4, 0..1);
            }
        } else if self.needs_initial_clear {
            // 无壁纸：维持原行为，仅 needs_initial_clear 时 LoadOp::Clear。
            let mut _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("ridge-host-clear-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(self.frame_clear_color),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
        }
        drop(ctx);
        self.needs_initial_clear = false;
```

> 注意：`wallpaper_pipeline` 字段当前是私有的。`begin_frame` 在同模块外（`surface_host.rs` ≠ `gpu_context.rs`）访问它，需把 `gpu_context.rs` 中 `wallpaper_pipeline` 字段改为 `pub`（与 `wallpaper_uniform` 一致）。本步顺手把 `wallpaper_pipeline` 字段的可见性改为 `pub wallpaper_pipeline`。

- [ ] **Step 2: 加 set_wallpaper / clear_wallpaper 转发**

在 `surface_host.rs` 的 `impl SurfaceHost` 内（`invalidate` 方法附近，约 229 行后）加：

```rust
    /// 设置全局壁纸（转调进程级 GpuContext）。换图/调 opacity/首次激活后
    /// 置 `needs_initial_clear`，保证下一帧无论是否有 pane 脏都重画。
    pub fn set_wallpaper(&mut self, rgba: &[u8], w: u32, h: u32, opacity: f32) {
        self.ctx.borrow_mut().set_wallpaper(rgba, w, h, opacity);
        self.needs_initial_clear = true;
    }

    /// 关闭壁纸。置 `needs_initial_clear` 让下一帧用主题底色重新 clear。
    pub fn clear_wallpaper(&mut self) {
        self.ctx.borrow_mut().clear_wallpaper();
        self.needs_initial_clear = true;
    }
```

- [ ] **Step 3: 编译验证**

Run: `cd packages/ridge-term && cargo build --features webgpu --target wasm32-unknown-unknown`
Expected: 编译通过。

- [ ] **Step 4: Commit**

```bash
git add packages/ridge-term/src/render/surface_host.rs packages/ridge-term/src/render/gpu_context.rs
git commit -m "feat(render): SurfaceHost 每帧画不透明壁纸 quad 顶替 clear + set/clear 转发"
```

---

## Task 4: lib.rs 暴露 setWallpaper / clearWallpaper（wasm-bindgen）

让 JS 可调到壁纸。沿用 `resize` / `invalidate` 的 `SurfaceHostHandle` 封装范式（lib.rs:1595-1605）。

**Files:**
- Modify: `packages/ridge-term/src/lib.rs`

**Interfaces:**
- Consumes: `SurfaceHost::set_wallpaper` / `clear_wallpaper`（Task 3）。
- Produces（`SurfaceHostHandle` JS 方法）:
  - `setWallpaper(rgba: Uint8Array, w: number, h: number, opacity: number): void`
  - `clearWallpaper(): void`

- [ ] **Step 1: 加 wasm-bindgen 方法**

在 `lib.rs` 的第三个 `#[wasm_bindgen] impl SurfaceHostHandle`（1573-1640，含 `init`/`resize`/`invalidate`/`beginFrame`/`endFrame`）里，`invalidate`（1603-1605）之后插入：

```rust
        /// 上传/替换全局壁纸纹理。`rgba` 是 straight-alpha 紧凑 RGBA
        /// （`w*h*4` 字节，浏览器 getImageData 布局）。空 buffer / 任一
        /// 维为 0 视作关闭。
        #[wasm_bindgen(js_name = setWallpaper)]
        pub fn set_wallpaper(&self, rgba: &[u8], w: u32, h: u32, opacity: f32) {
            self.host.borrow_mut().set_wallpaper(rgba, w, h, opacity);
        }

        /// 关闭全局壁纸，下一帧退回主题底色 clear。
        #[wasm_bindgen(js_name = clearWallpaper)]
        pub fn clear_wallpaper(&self) {
            self.host.borrow_mut().clear_wallpaper();
        }
```

- [ ] **Step 2: 重新构建 wasm 包**

Run: `cd packages/ridge-term && wasm-pack build --target web --features webgpu`（沿用仓库既有 wasm 构建命令；若仓库有封装脚本如 `pnpm build:wasm` 则用之）
Expected: 构建成功，生成的 `.d.ts` 中 `SurfaceHostHandle` 含 `setWallpaper` / `clearWallpaper`。

- [ ] **Step 3: Commit**

```bash
git add packages/ridge-term/src/lib.rs
git commit -m "feat(render): SurfaceHostHandle 暴露 setWallpaper/clearWallpaper wasm-bindgen 封装"
```

---

## Task 5: themes.ts 解码壁纸为 RGBA + 暴露 activeWallpaper 信号

在现有 `activeBgImage`（URL 信号）旁，新增解码出像素的 `activeWallpaper` 信号，供 manager 推给 wasm。解码交给浏览器（离屏 canvas `getImageData`），wasm 不引入图片解码 crate。

**Files:**
- Modify: `src/lib/stores/themes.ts`
- Test: `src/lib/stores/themes.wallpaper.test.ts`（Create）

**Interfaces:**
- Consumes: 现有 `resolveThemeBgUrl(t)`、`getTheme(id)`、`bgImageStore`。
- Produces:
  - `export interface WallpaperData { rgba: Uint8Array; w: number; h: number; opacity: number }`
  - `export const activeWallpaper: { subscribe }`（store，值为 `WallpaperData | null`）
  - `export async function decodeImageToRgba(url: string): Promise<{ rgba: Uint8Array; w: number; h: number } | null>`（导出供测试与复用）
  - `setActiveBgImage` 内部在解析出 url 后追加解码并 `set` 到 `activeWallpaper`（url=null → 置 null）。

- [ ] **Step 1: 写失败测试**

Create `src/lib/stores/themes.wallpaper.test.ts`:

```typescript
import { describe, it, expect, vi, beforeEach } from 'vitest';

// 被测函数纯靠 DOM Image + canvas，jsdom 不真正解码，故 mock 这两者。
describe('decodeImageToRgba', () => {
  beforeEach(() => {
    vi.resetModules();
  });

  it('returns RGBA bytes + dimensions on successful decode', async () => {
    // mock Image：onload 在 src 赋值后微任务触发
    class FakeImage {
      onload: (() => void) | null = null;
      onerror: (() => void) | null = null;
      width = 2;
      height = 1;
      set src(_v: string) {
        queueMicrotask(() => this.onload && this.onload());
      }
    }
    vi.stubGlobal('Image', FakeImage as unknown as typeof Image);

    const fakeCtx = {
      drawImage: vi.fn(),
      getImageData: vi.fn(() => ({
        data: new Uint8ClampedArray([1, 2, 3, 4, 5, 6, 7, 8]),
        width: 2,
        height: 1,
      })),
    };
    const fakeCanvas = {
      width: 0,
      height: 0,
      getContext: vi.fn(() => fakeCtx),
    };
    vi.stubGlobal('document', {
      createElement: vi.fn(() => fakeCanvas),
    });

    const { decodeImageToRgba } = await import('./themes');
    const out = await decodeImageToRgba('blob:fake');
    expect(out).not.toBeNull();
    expect(out!.w).toBe(2);
    expect(out!.h).toBe(1);
    expect(Array.from(out!.rgba)).toEqual([1, 2, 3, 4, 5, 6, 7, 8]);
  });

  it('returns null when the image fails to load', async () => {
    class FailImage {
      onload: (() => void) | null = null;
      onerror: (() => void) | null = null;
      set src(_v: string) {
        queueMicrotask(() => this.onerror && this.onerror());
      }
    }
    vi.stubGlobal('Image', FailImage as unknown as typeof Image);
    const { decodeImageToRgba } = await import('./themes');
    const out = await decodeImageToRgba('blob:broken');
    expect(out).toBeNull();
  });
});
```

- [ ] **Step 2: 运行测试确认失败**

Run: `pnpm test -- themes.wallpaper`
Expected: FAIL（`decodeImageToRgba` 未导出）。

- [ ] **Step 3: 实现 decodeImageToRgba + activeWallpaper 信号**

在 `src/lib/stores/themes.ts` 的活动壁纸信号区（143-144 行）旁补：

```typescript
// ── 活动壁纸像素信号（喂给 WebGPU 渲染器）────────────────────────
export interface WallpaperData {
  rgba: Uint8Array;
  w: number;
  h: number;
  opacity: number;
}
const wallpaperStore = writable<WallpaperData | null>(null);
export const activeWallpaper = { subscribe: wallpaperStore.subscribe };

/**
 * 用离屏 canvas 把图片 URL 解码成紧凑 RGBA（straight alpha，bytes_per_row =
 * w*4）。解码失败（404 / 非图片 / 跨域 taint）返回 null。导出供测试。
 */
export async function decodeImageToRgba(
  url: string,
): Promise<{ rgba: Uint8Array; w: number; h: number } | null> {
  try {
    const img: HTMLImageElement = await new Promise((resolve, reject) => {
      const im = new Image();
      im.onload = () => resolve(im);
      im.onerror = () => reject(new Error('img load failed'));
      im.src = url;
    });
    const w = img.width;
    const h = img.height;
    if (!w || !h) return null;
    const canvas = document.createElement('canvas');
    canvas.width = w;
    canvas.height = h;
    const ctx = canvas.getContext('2d', { willReadFrequently: true });
    if (!ctx) return null;
    ctx.drawImage(img, 0, 0);
    const { data } = ctx.getImageData(0, 0, w, h);
    return { rgba: new Uint8Array(data.buffer.slice(0)), w, h };
  } catch {
    return null;
  }
}
```

并把 `setActiveBgImage`（173-178 行）改为在更新 url 信号后同步解码壁纸：

```typescript
/** 解析某主题的背景图为可加载 URL + 解码像素，更新两个信号。fire-and-forget。 */
export async function setActiveBgImage(themeId: string): Promise<void> {
  const t = getTheme(themeId);
  const opacity = t?.bgImageOpacity ?? 1;
  const url = await resolveThemeBgUrl(t);
  bgImageStore.set({ url, opacity });
  if (!url) {
    wallpaperStore.set(null);
    return;
  }
  const decoded = await decodeImageToRgba(url);
  wallpaperStore.set(decoded ? { ...decoded, opacity } : null);
}
```

- [ ] **Step 4: 运行测试确认通过**

Run: `pnpm test -- themes.wallpaper`
Expected: PASS（2 个测试）。

- [ ] **Step 5: Commit**

```bash
git add src/lib/stores/themes.ts src/lib/stores/themes.wallpaper.test.ts
git commit -m "feat(themes): 离屏 canvas 解码壁纸为 RGBA + activeWallpaper 信号"
```

---

## Task 6: manager.ts setWallpaper / clearWallpaper + 订阅信号

把壁纸像素推给主线程 global host，并沿用 `setTheme` 的 `invalidateAllPanes()` + `_invalidateHost()` 立即重绘组合。订阅 `activeWallpaper`，变化时调用。

**Files:**
- Modify: `src/lib/terminal/manager.ts`

**Interfaces:**
- Consumes: `this._globalHostHandle()`（→ `SurfaceHostHandle.setWallpaper/clearWallpaper`，Task 4）、`this.invalidateAllPanes()`、`this._invalidateHost()`、`this.wake()`、`activeWallpaper`（Task 5）。
- Produces:
  - `public setWallpaper(rgba: Uint8Array, w: number, h: number, opacity: number): void`
  - `public clearWallpaper(): void`

- [ ] **Step 1: 加 setWallpaper / clearWallpaper 方法**

在 `manager.ts` 的 `setTheme`（3983 起）之后插入。`SurfaceHostHandle` 的 TS 类型（wasm-pack 生成的 `.d.ts`）在 Task 4 已含新方法，直接调用：

```typescript
	/**
	 * 推送全局壁纸像素到主线程 host（WebGPU 默认路径）。worker-renderer 与
	 * Canvas2D 路径无 global host，自然 no-op（不在本特性范围）。沿用
	 * setTheme 的「丢 pane 缓存 + 强制 host 重画」组合，保证无鬼影、立即生效。
	 */
	setWallpaper(rgba: Uint8Array, w: number, h: number, opacity: number): void {
		const host = this._globalHostHandle();
		if (!host) return;
		host.setWallpaper(rgba, w, h, opacity);
		this.invalidateAllPanes();
		this._invalidateHost();
		this.wake();
	}

	/** 关闭全局壁纸，下一帧退回主题底色 clear。 */
	clearWallpaper(): void {
		const host = this._globalHostHandle();
		if (!host) return;
		host.clearWallpaper();
		this.invalidateAllPanes();
		this._invalidateHost();
		this.wake();
	}
```

- [ ] **Step 2: 订阅 activeWallpaper 信号**

找到 manager 订阅 store 的 boot/setup 处。`setupTerminalThemeBridge`（themeBridge.ts）已订阅 `activeBgImage`；但壁纸像素订阅应建在 manager 生命周期内，以便调 `this.setWallpaper`。在 manager 构造函数或 `ready()` 完成后的初始化段（搜索现有 `.subscribe(` 调用作为放置参考；若 manager 无现成订阅点，则在构造函数末尾）加：

```typescript
		// §wallpaper：订阅解码后的壁纸像素，推给 WebGPU global host。
		// 首帧 host 可能尚未 attach——setWallpaper 内 _globalHostHandle()
		// 为 null 时 no-op，attachHost 完成后的下一次信号（或主题切换）补推。
		import('$lib/stores/themes').then(({ activeWallpaper }) => {
			this._unsubWallpaper = activeWallpaper.subscribe((wp) => {
				if (wp) this.setWallpaper(wp.rgba, wp.w, wp.h, wp.opacity);
				else this.clearWallpaper();
			});
		});
```

> 注意两点：
> 1. 在 manager 类加私有字段 `private _unsubWallpaper: (() => void) | null = null;`，并在 manager 的销毁/teardown 方法（搜索 `detachHost` 或 `destroy`）中调用 `this._unsubWallpaper?.();`。
> 2. 若 manager 顶部已静态 `import { ... } from '$lib/stores/themes'`，直接静态导入 `activeWallpaper` 并订阅，免用动态 `import()`。优先静态导入以保持风格一致。

- [ ] **Step 3: attachHost 后补推一次当前壁纸**

`attachHost`（762-800）成功设置 `this.globalHost` 后，host 刚就绪。若壁纸信号在 host attach 之前已 fire，需补推一次，否则首屏无壁纸直到下次主题切换。在 `attachHost` 的 `this.globalHost = { canvas, host };`（798）之后、`this.resizeHost();`（799）之前插入：

```typescript
			// host 刚就绪：若已有解码好的壁纸，立即补推（信号可能早于 attach fire）。
			try {
				const { get } = await import('svelte/store');
				const { activeWallpaper } = await import('$lib/stores/themes');
				const wp = get(activeWallpaper);
				if (wp) host.setWallpaper(wp.rgba, wp.w, wp.h, wp.opacity);
			} catch { /* SSR / store 未就绪 → 跳过，下次信号补推 */ }
```

> 若 manager 顶部已静态 import `get`（搜索 `from 'svelte/store'`，第 1-40 行已 import 多个 store 工具，大概率已有 `get`），改用静态引用，仅动态 import `activeWallpaper` 或同样静态化。

- [ ] **Step 4: 类型检查**

Run: `pnpm check`
Expected: svelte-check 0 error 0 warning（壁纸新增类型与方法均对齐）。

- [ ] **Step 5: Commit**

```bash
git add src/lib/terminal/manager.ts
git commit -m "feat(terminal): manager.setWallpaper/clearWallpaper 推送 + 订阅 activeWallpaper 信号"
```

---

## Task 7: 删除失效的 DOM `.rg-pane-bgimg`

新渲染器壁纸生效后，DOM `.rg-pane-bgimg` 在 WebGPU 路径从未真正显示在文字后方（2026-06-16 CDP 已证），且每 pane 各画一份。删除它（DOM 块 + CSS + `activeBgImage` 在本组件的引用）。

**Files:**
- Modify: `src/lib/components/RidgePane.svelte`

**Interfaces:**
- 移除对 `activeBgImage` 的组件级使用（store 本身保留，设置面板卡片预览/编辑器预览仍用 `resolveThemeBgUrl`）。

- [ ] **Step 1: 删除 DOM 块**

删除 `RidgePane.svelte` 1802-1811 行整段（注释 + `{#if $activeBgImage.url}` ... `{/if}`）：

```svelte
	<!-- 终端背景图层：absolute z-index:0，必须是容器的首个子节点，
	     才能稳定排在 wasm canvas（由 manager 后续 append）的 DOM 顺序之前、
	     渲染在其下方。勿在它前面插入其它元素，否则层叠会错乱。 -->
	{#if $activeBgImage.url}
		<div
			class="rg-pane-bgimg"
			style="background-image: url('{$activeBgImage.url}'); opacity: {$activeBgImage.opacity};"
			aria-hidden="true"
		></div>
	{/if}
```

- [ ] **Step 2: 删除 CSS 规则**

删除 `RidgePane.svelte` 2199-2207 行的 `.rg-pane-bgimg { ... }` 整条规则。

- [ ] **Step 3: 删除 import**

删除 39 行 `import { activeBgImage } from '$lib/stores/themes';`（确认本组件再无其它 `activeBgImage` 用法——grep 组件内仅这两处）。

- [ ] **Step 4: 类型检查 + 无悬挂引用**

Run: `pnpm check`
Expected: 0/0；无 "unused import" / "undefined `activeBgImage`" 报错。

Run: `git grep -n "rg-pane-bgimg" src/`
Expected: 无输出（DOM + CSS 均已删）。

- [ ] **Step 5: Commit**

```bash
git add src/lib/components/RidgePane.svelte
git commit -m "refactor(render): 删除 WebGPU 路径从未生效的 DOM .rg-pane-bgimg 壁纸层"
```

---

## Task 8: 真机 CDP 集成验证（非自动化）

GPU 绘制无法用单测覆盖（见 `docs/superpowers/specs/2026-06-16-workspace-wide-theme-background-design.md` 的 CDP 证伪结论）。本任务为人工/CDP 复验，不产出 commit（除非发现缺陷需修）。

**Files:** 无（验证任务）。

- [ ] **Step 1: 全量构建**

Run: `cd packages/ridge-term && wasm-pack build --target web --features webgpu`（或仓库封装脚本），随后 `pnpm tauri:dev:cdp`。

- [ ] **Step 2: 逐条验证（对照 06-18 设计验证节）**

1. 建带壁纸的自定义主题并激活 → 整工作区一张连续图，文字/选中高亮清晰浮于其上（**修复用户三症状**：整张图、选中不被遮、文字在图前）。
2. 分屏 → 仍是同一张连续图（含 splitter gutter 铺满），非每 pane 一份。
3. 切主题 / 换图 / 调 opacity → 即时生效、无鬼影残留。
4. resize 窗口 / 切 sidebar → 壁纸 cover 重算、不错位、不拉伸变形。
5. 切到无壁纸主题 → 回到不透明底色，渲染与改动前一致（无回归、无性能退化）。

- [ ] **Step 3: 回归确认**

Run: `cargo test -p ridge-term --lib && pnpm test && pnpm check`
Expected: 全绿、0/0。

---

## Self-Review

**Spec 覆盖（对照 06-18 设计「组件与改动点」）：**
- themes.ts 解码 RGBA + 暴露信号 → Task 5 ✓
- manager.ts setWallpaper/clearWallpaper + 订阅 + invalidate 组合 → Task 6 ✓
- themeBridge.ts 不改 → Global Constraints 明确 ✓
- RidgePane.svelte 删除 .rg-pane-bgimg → Task 7 ✓
- gpu_context.rs 壁纸资源 + set/clear_wallpaper（含 256 对齐）→ Task 2 ✓
- wallpaper.wgsl（全屏 quad + cover + mix）→ Task 2 ✓
- surface_host.rs begin_frame 每帧画 quad + set/clear 转发 → Task 3 ✓
- lib.rs + SurfaceHostHandle wasm-bindgen → Task 4 ✓
- cover-UV / 256 对齐下沉纯函数 + 单测 → Task 1 ✓
- 验证（cargo test --lib / pnpm check / pnpm test / 真机 5 条）→ Task 8 ✓

**与设计的有意偏差（已确认）：**
- **不做 worker-renderer 镜像**：host frame 实测在主线程 RAF（manager.ts:4665-4681），壁纸是主线程 GpuContext 单例资源，worker 只画 pane → 无需像 setFont 那样经 workerRendererBridge 镜像。设计文档 §JS 侧「worker 模式经 workerRendererBridge 镜像」基于壁纸资源可能在 worker 的假设，实测不成立，故省去。用户已圈定「只修 webgpu 默认情况」，worker 路径明确排除。

**类型一致性：** `WallpaperData`/`WallpaperTex`/`CoverUv` 与 `setWallpaper(rgba,w,h,opacity)` 签名在 Task 1/2/4/5/6 间一致；`activeWallpaper` 信号名贯穿 Task 5/6。

**Placeholder 扫描：** 无 TBD/TODO；每个 code step 含完整代码；命令含预期输出。

---

## Execution Handoff

Plan 已保存到 `docs/superpowers/plans/2026-06-25-renderer-webgpu-wallpaper.md`。
