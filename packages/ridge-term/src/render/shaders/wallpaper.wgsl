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
