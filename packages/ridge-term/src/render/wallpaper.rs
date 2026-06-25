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
