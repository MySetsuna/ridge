//! Glyph atlas — content-addressed cache of rasterized glyphs.
//!
//! ## Purpose (TASKS §4.2 + OVERVIEW §D1)
//!
//! The WebGPU renderer rasterizes each selected-font glyph once, uploads its
//! bitmap to a texture array, then references it by `(layer, uv)` at draw
//! time. The atlas maps `GlyphKey → GlyphEntry`; texture ownership remains
//! in the shared WebGPU context.
//!
//! ## Decoupling from the renderer
//!
//! `GlyphAtlas` knows nothing about wgpu or web-sys. It is a pure data
//! structure: insert + lookup + LRU eviction. Host `cargo test --lib` can
//! therefore verify eviction without a GPU.
//!
//! ## Key design
//!
//! Color is intentionally NOT in the key. SDF / coverage rendering tints
//! at draw time via a shader uniform; bitmap rendering does the same with
//! a multiply blend. Including color would explode the cache by 16M× —
//! same glyph at every possible RGB.
//!
//! Font size is quantized to `u16` (1/100 of a pixel) so floating-point
//! jitter from devicePixelRatio rounding can't fragment the cache. Size
//! 14.0 and 14.000001 hash to the same bucket.

use std::collections::{hash_map::Entry, HashMap};

/// Cache key. Identifies a glyph variant by (font, size, raster density,
/// codepoint or font-internal id, weight/slant flags).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GlyphKey {
    /// Hash of the resolved font family (after fallback chain). Two
    /// distinct family strings that resolve to the same physical face
    /// should produce the same hash for cache hits.
    pub font_family_hash: u64,
    /// Font size in 1/100 px units. 14.5 px → 1450.
    pub font_size_q: u16,
    /// Codepoint OR font-internal glyph id (for shaping outputs).
    /// Renderer chooses based on whether shaping was applied.
    pub glyph_id: u32,
    /// Weight + slant flags packed into a u8.
    /// Bit 0 = bold, bit 1 = italic, bits 2-7 reserved.
    pub style_flags: u8,
    /// Rasterizer device density in 1/1000 units. The atlas is shared by
    /// panes, so DPR must participate in identity or a high-DPR pane can
    /// reuse a low-DPR bitmap admitted by a sibling pane.
    pub raster_dpr_q: u16,
}

impl GlyphKey {
    pub const STYLE_BOLD: u8 = 0b01;
    pub const STYLE_ITALIC: u8 = 0b10;

    pub fn new(
        font_family_hash: u64,
        font_size_q: u16,
        glyph_id: u32,
        style_flags: u8,
        raster_dpr: f32,
    ) -> Self {
        let dpr = if raster_dpr.is_finite() && raster_dpr > 0.0 {
            raster_dpr
        } else {
            1.0
        };
        Self {
            font_family_hash,
            font_size_q,
            glyph_id,
            style_flags,
            raster_dpr_q: (dpr * 1000.0).round().clamp(1.0, u16::MAX as f32) as u16,
        }
    }
}

/// Cached entry — where the bitmap lives in the texture array and how
/// to position it relative to the cell box.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GlyphEntry {
    /// Texture array layer (or 2D atlas index — backend-defined).
    pub layer: u16,
    /// UV rect inside that layer: `(u0, v0, u1, v1)` normalized 0..1.
    pub uv: [f32; 4],
    /// Horizontal advance in CSS pixels (used for width-2 wide cells
    /// to confirm the glyph actually occupies two cell widths).
    pub advance: f32,
    /// Vertical offset from cell top to glyph baseline. Backend uses
    /// this to position the bitmap inside the cell box.
    pub ascent_offset: f32,
    /// Horizontal offset from the logical cell origin to the bitmap left.
    /// Negative values retain native left overhang.
    pub left_offset: f32,
    /// Native device-pixel bitmap dimensions after removing any atlas-only
    /// supersample factor. Draw quads always preserve this extent.
    pub px_w: u16,
    pub px_h: u16,
    /// True when the glyph carries a color-emoji palette in its atlas
    /// pixels (per `RasterizedGlyph::is_color`). The renderer keeps the
    /// complete native bitmap extent; it may overhang its reserved cells.
    pub is_color: bool,
    /// Box Drawing or Block Elements glyph rasterized by Swash, clipped to the
    /// exact snapped terminal cell without scaling.
    pub is_cell_graphic: bool,
}

/// Place a glyph at its native raster size, including font overhang.
#[allow(dead_code)]
pub(crate) fn glyph_quad_geometry(
    pixel_x: f32,
    pixel_y: f32,
    entry: &GlyphEntry,
) -> ([f32; 2], [f32; 2]) {
    let (cell_xy, cell_size, _) = glyph_quad_geometry_with_uv(pixel_x, pixel_y, entry);
    (cell_xy, cell_size)
}

/// Return native-size glyph geometry and the complete atlas UV rectangle.
/// Terminal width controls layout only; later cells naturally overpaint any
/// visual overflow, matching native terminal overlap behavior.
#[cfg_attr(
    not(any(test, all(target_arch = "wasm32", feature = "webgpu"))),
    allow(dead_code)
)]
pub(crate) fn glyph_quad_geometry_with_uv(
    pixel_x: f32,
    pixel_y: f32,
    entry: &GlyphEntry,
) -> ([f32; 2], [f32; 2], [f32; 4]) {
    (
        [pixel_x + entry.left_offset, pixel_y + entry.ascent_offset],
        [(entry.px_w as f32).max(1.0), (entry.px_h as f32).max(1.0)],
        entry.uv,
    )
}

/// Fit cell-sensitive glyphs without changing ordinary font geometry. Box
/// drawing spans the exact snapped cell width and is clipped at row boundaries;
/// color emoji and ordinary glyphs retain their native overhang.
#[cfg_attr(
    not(any(test, all(target_arch = "wasm32", feature = "webgpu"))),
    allow(dead_code)
)]
pub(crate) fn glyph_quad_geometry_for_cell(
    pixel_x: f32,
    pixel_y: f32,
    entry: &GlyphEntry,
    allocation_w: f32,
    allocation_h: f32,
) -> ([f32; 2], [f32; 2], [f32; 4]) {
    if entry.is_cell_graphic {
        let source_w = (entry.px_w as f32).max(1.0);
        let source_h = (entry.px_h as f32).max(1.0);
        let native_left = pixel_x + entry.left_offset;
        let native_top = pixel_y + entry.ascent_offset;
        let native_right = native_left + source_w;
        let native_bottom = native_top + source_h;
        let cell_right = pixel_x + allocation_w.max(1.0);
        let row_bottom = pixel_y + allocation_h.max(1.0);
        let draw_left = native_left.max(pixel_x);
        let draw_top = native_top.max(pixel_y);
        let draw_right = native_right.min(cell_right);
        let draw_bottom = native_bottom.min(row_bottom);
        if draw_right <= draw_left || draw_bottom <= draw_top {
            return ([pixel_x, pixel_y], [0.0, 0.0], entry.uv);
        }

        let uv_width = entry.uv[2] - entry.uv[0];
        let uv_height = entry.uv[3] - entry.uv[1];
        let u0 = entry.uv[0] + ((draw_left - native_left) / source_w) * uv_width;
        let u1 = entry.uv[0] + ((draw_right - native_left) / source_w) * uv_width;
        let v0 = entry.uv[1] + ((draw_top - native_top) / source_h) * uv_height;
        let v1 = entry.uv[1] + ((draw_bottom - native_top) / source_h) * uv_height;
        return (
            [draw_left, draw_top],
            [draw_right - draw_left, draw_bottom - draw_top],
            [u0, v0, u1, v1],
        );
    }

    glyph_quad_geometry_with_uv(pixel_x, pixel_y, entry)
}

struct AtlasEntry {
    entry: GlyphEntry,
    last_used: u64,
}

/// LRU-evicting cache. `lookup` promotes a key to the most-recently-used
/// position; `insert` pushes the least-recently-used out when at capacity
/// and returns the evicted key so the backend can free the texture slot.
pub struct GlyphAtlas {
    entries: HashMap<GlyphKey, AtlasEntry>,
    clock: u64,
    capacity: usize,
}

impl GlyphAtlas {
    /// Create an atlas with the given capacity. `capacity = 0` is a
    /// degenerate config that immediately rejects every insert; useful
    /// for testing the eviction path but not for production.
    pub fn new(capacity: usize) -> Self {
        Self {
            entries: HashMap::with_capacity(capacity),
            clock: 0,
            capacity,
        }
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn rebase_clock(&mut self) {
        let mut keys: Vec<_> = self.entries.keys().copied().collect();
        keys.sort_unstable_by_key(|key| {
            self.entries
                .get(key)
                .expect("rebased key must exist")
                .last_used
        });
        for (rank, key) in keys.into_iter().enumerate() {
            self.entries
                .get_mut(&key)
                .expect("rebased key must exist")
                .last_used = rank as u64 + 1;
        }
        self.clock = self.entries.len() as u64;
    }

    fn next_stamp(&mut self) -> u64 {
        if let Some(next) = self.clock.checked_add(1) {
            self.clock = next;
        } else {
            self.rebase_clock();
            self.clock += 1;
        }
        self.clock
    }

    /// Returns `Some(entry)` on hit and promotes the key to MRU. `None`
    /// on miss — caller is responsible for rasterizing + `insert`.
    pub fn lookup(&mut self, key: &GlyphKey) -> Option<GlyphEntry> {
        let stamp = self.next_stamp();
        let cached = self.entries.get_mut(key)?;
        cached.last_used = stamp;
        Some(cached.entry)
    }

    /// Insert a freshly-rasterized glyph. If the cache is at capacity,
    /// evicts the LRU entry and returns its key (caller frees the
    /// associated texture slot). A duplicate insert (same key) replaces
    /// the entry without eviction.
    pub fn insert(&mut self, key: GlyphKey, entry: GlyphEntry) -> Option<GlyphKey> {
        let stamp = self.next_stamp();
        if let Entry::Occupied(mut existing) = self.entries.entry(key) {
            let existing = existing.get_mut();
            existing.entry = entry;
            existing.last_used = stamp;
            return None;
        }

        if self.capacity == 0 {
            // No room ever; reject. Return the rejected key so the
            // caller knows it was not stored (mirrors eviction shape).
            return Some(key);
        }

        let evicted = if self.entries.len() >= self.capacity {
            self.evict_oldest().map(|(key, _)| key)
        } else {
            None
        };

        self.entries.insert(
            key,
            AtlasEntry {
                entry,
                last_used: stamp,
            },
        );
        evicted
    }

    /// Drop everything. Backend should free all atlas slots after this.
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Pop the LRU entry, returning both its key and its entry (so the
    /// caller can reclaim the entry's resources — e.g. a WebGPU
    /// texture-array layer index). Returns `None` when the atlas is
    /// empty.
    ///
    /// Why this exists separately from `insert`'s eviction path:
    /// `insert` returns the evicted KEY only and drops the entry.
    /// For backends that need to reuse the entry's owned resources
    /// (texture-array slot, vertex offset, …) BEFORE the next insert
    /// can fill that resource, a separate evict-then-insert flow is
    /// required.
    ///
    /// Typical pattern:
    /// ```text
    /// let (target_layer, entry) = if atlas.len() < CAPACITY {
    ///     (next_free_layer, /* fresh */ )
    /// } else {
    ///     let (_, freed) = atlas.evict_oldest().unwrap();
    ///     (freed.layer, /* reused */ )
    /// };
    /// queue.write_texture(target_layer, &new_bitmap, ...);
    /// atlas.insert(new_key, GlyphEntry { layer: target_layer, ... });
    /// ```
    pub fn evict_oldest(&mut self) -> Option<(GlyphKey, GlyphEntry)> {
        let key = self
            .entries
            .iter()
            .min_by_key(|(_, cached)| cached.last_used)
            .map(|(key, _)| *key)?;
        let cached = self.entries.remove(&key)?;
        Some((key, cached.entry))
    }
}

/// Pick a texture-array layer the caller can safely overwrite this frame.
///
/// The atlas is at capacity (every usable slot is occupied), so an entry
/// must be evicted to free a layer. Eviction rule: walk LRU → MRU, but
/// skip any entry whose `layer` is currently *pinned* (i.e. an earlier
/// instance in the caller's per-frame draw queue already references that
/// layer's pixels). Skipped entries are re-inserted afterward so they
/// stay live in the cache; their MRU rotation reflects the truth that
/// they are being actively sampled by the current frame.
///
/// Returns `None` only when **every** layer in the LRU is pinned — i.e.
/// the visible-unique-glyph count exceeds atlas capacity in a single
/// frame. The caller should fall back to its bg-only path for the cell
/// that triggered the miss; the next frame can re-rasterize once some
/// layers release their pins.
///
/// ## Why this lives in `glyph_atlas.rs`
///
/// The function is GPU-agnostic: it takes `&mut GlyphAtlas` and a
/// `&[bool]` indexed by layer. The pin-bitmap concept is general
/// ("don't pick these layers"), not WebGPU-specific. Hosting it in the
/// atlas module lets host `cargo test --lib` exercise the eviction walk
/// without any `wasm32 + webgpu` build, which is the only place a real
/// regression would otherwise be observable.
///
/// ## Bug history
///
/// Without the pinning skip, the WebGPU backend was reusing a layer
/// that an earlier instance in the same frame had already cited;
/// `queue.write_texture` then overwrote the layer's pixels before the
/// GPU sampled them, so the earlier cell rendered the *new* glyph. The
/// frame-to-frame variation produced the visible "Claude TUI 历史输出
/// 字符不停刷新" symptom.
/// Pick an LRU layer to reuse. The returned layer is guaranteed:
///   - Not in `pinned` (per-pane pin set for current frame)
///   - Not in `written` (global pin set: already written by any pane this frame)
///
/// Returns `None` when every layer is pinned or written (atlas exhausted).
pub fn pick_evictable_layer(
    atlas: &mut GlyphAtlas,
    pinned: &[bool],
    written: &[bool],
) -> Option<u32> {
    let mut blocked: Vec<(GlyphKey, u64)> = Vec::with_capacity(8);
    let mut chosen: Option<(GlyphKey, u64, u32)> = None;
    for (key, cached) in &atlas.entries {
        let layer = cached.entry.layer as usize;
        let is_pinned = pinned.get(layer).copied().unwrap_or(false);
        let is_written = written.get(layer).copied().unwrap_or(true);
        if is_pinned || is_written {
            blocked.push((*key, cached.last_used));
        } else if chosen
            .as_ref()
            .map_or(true, |(_, oldest, _)| cached.last_used < *oldest)
        {
            chosen = Some((*key, cached.last_used, cached.entry.layer as u32));
        }
    }

    let chosen_stamp = chosen.map(|(_, stamp, _)| stamp);
    let chosen_layer = chosen.map(|(_, _, layer)| layer);
    if let Some((key, _, _)) = chosen {
        atlas.entries.remove(&key);
    }

    // The old eviction walk requeued blocked entries seen before the victim,
    // making them MRU in their original LRU order. Reapply that rotation in
    // one bounded sort instead of repeatedly scanning the atlas.
    blocked.retain(|(_, stamp)| chosen_stamp.map_or(true, |oldest| *stamp < oldest));
    blocked.sort_unstable_by_key(|(_, stamp)| *stamp);
    for (key, _) in blocked {
        let stamp = atlas.next_stamp();
        if let Some(cached) = atlas.entries.get_mut(&key) {
            cached.last_used = stamp;
        }
    }
    chosen_layer
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(id: u32) -> GlyphKey {
        GlyphKey::new(0xabc, 1400, id, 0, 1.0)
    }

    fn entry(layer: u16) -> GlyphEntry {
        GlyphEntry {
            layer,
            uv: [0.0, 0.0, 1.0, 1.0],
            advance: 8.0,
            ascent_offset: 12.0,
            left_offset: 0.0,
            px_w: 8,
            px_h: 16,
            is_color: false,
            is_cell_graphic: false,
        }
    }

    #[test]
    fn lookup_returns_none_for_missing() {
        let mut a = GlyphAtlas::new(4);
        assert!(a.lookup(&key(1)).is_none());
    }

    #[test]
    fn insert_then_lookup_round_trips() {
        let mut a = GlyphAtlas::new(4);
        assert_eq!(a.insert(key(1), entry(0)), None);
        assert_eq!(a.lookup(&key(1)), Some(entry(0)));
        assert_eq!(a.len(), 1);
    }

    #[test]
    fn raster_density_is_part_of_cache_identity() {
        let low = GlyphKey::new(0xabc, 1400, 65, 0, 1.0);
        let high = GlyphKey::new(0xabc, 1400, 65, 0, 2.0);
        assert_ne!(low, high);
        assert_eq!(low.raster_dpr_q, 1000);
        assert_eq!(high.raster_dpr_q, 2000);
    }

    #[test]
    fn glyph_quad_preserves_native_bitmap_extent_and_vertical_offset_when_contained() {
        let glyph = GlyphEntry {
            ascent_offset: 3.0,
            left_offset: -2.0,
            px_w: 7,
            px_h: 11,
            ..entry(0)
        };
        assert_eq!(
            glyph_quad_geometry(10.0, 20.0, &glyph),
            ([8.0, 23.0], [7.0, 11.0])
        );
    }

    #[test]
    fn glyph_quad_keeps_oversized_fallback_bitmap_and_complete_uv() {
        let glyph = GlyphEntry {
            ascent_offset: 0.0,
            px_w: 16,
            px_h: 20,
            ..entry(0)
        };
        assert_eq!(
            glyph_quad_geometry(10.0, 20.0, &glyph),
            ([10.0, 20.0], [16.0, 20.0])
        );
        assert_eq!(
            glyph_quad_geometry_with_uv(10.0, 20.0, &glyph).2,
            [0.0, 0.0, 1.0, 1.0]
        );
    }

    #[test]
    fn wide_glyph_is_not_scaled_to_its_reserved_cell_box() {
        let glyph = GlyphEntry {
            ascent_offset: 0.0,
            px_w: 8,
            px_h: 8,
            ..entry(0)
        };
        assert_eq!(
            glyph_quad_geometry(0.0, 0.0, &glyph),
            ([0.0, 0.0], [8.0, 8.0])
        );
    }

    #[test]
    fn color_emoji_preserve_native_extent_and_overflow_cell_box() {
        let small = GlyphEntry {
            px_w: 8,
            px_h: 8,
            ascent_offset: 2.0,
            left_offset: 1.0,
            is_color: true,
            ..entry(0)
        };
        let decorated = GlyphEntry {
            px_w: 24,
            px_h: 24,
            ascent_offset: 2.0,
            left_offset: 1.0,
            is_color: true,
            ..entry(1)
        };

        let small_quad = glyph_quad_geometry_for_cell(10.0, 20.0, &small, 16.0, 18.0);
        let decorated_quad = glyph_quad_geometry_for_cell(10.0, 20.0, &decorated, 16.0, 18.0);
        assert_eq!(small_quad, ([11.0, 22.0], [8.0, 8.0], small.uv));
        assert_eq!(decorated_quad, ([11.0, 22.0], [24.0, 24.0], decorated.uv));
        assert!(decorated_quad.1[0] > 16.0);
        assert!(decorated_quad.1[1] > 18.0);
    }

    #[test]
    fn color_emoji_keep_native_aspect_ratio_and_complete_uv_when_overflowing() {
        let glyph = GlyphEntry {
            uv: [0.0, 0.0, 0.75, 0.5],
            px_w: 24,
            px_h: 12,
            is_color: true,
            ..entry(0)
        };

        let (xy, size, uv) = glyph_quad_geometry_for_cell(4.0, 6.0, &glyph, 16.0, 18.0);
        assert_eq!(xy, [4.0, 18.0]);
        assert_eq!(size, [24.0, 12.0]);
        assert_eq!(uv, glyph.uv);
        assert!(xy[0] + size[0] > 4.0 + 16.0);
        assert!(xy[1] + size[1] > 6.0 + 18.0);
    }

    #[test]
    fn monochrome_cell_geometry_remains_native() {
        let glyph = GlyphEntry {
            ascent_offset: 3.0,
            left_offset: -2.0,
            px_w: 7,
            px_h: 11,
            ..entry(0)
        };
        assert_eq!(
            glyph_quad_geometry_for_cell(10.0, 20.0, &glyph, 16.0, 18.0),
            ([8.0, 23.0], [7.0, 11.0], glyph.uv)
        );
    }

    #[test]
    fn box_drawing_crops_to_snapped_cell_width_without_scaling() {
        let glyph = GlyphEntry {
            px_w: 11,
            px_h: 3,
            ascent_offset: 7.0,
            is_cell_graphic: true,
            ..entry(0)
        };
        assert_eq!(
            glyph_quad_geometry_for_cell(10.0, 20.0, &glyph, 10.0, 18.0),
            ([10.0, 27.0], [10.0, 3.0], [0.0, 0.0, 10.0 / 11.0, 1.0])
        );
    }

    #[test]
    fn box_drawing_clips_to_snapped_row_without_vertical_scaling() {
        let glyph = GlyphEntry {
            uv: [0.0, 0.0, 0.5, 0.75],
            px_w: 11,
            px_h: 23,
            ascent_offset: 0.0,
            is_cell_graphic: true,
            ..entry(0)
        };
        assert_eq!(
            glyph_quad_geometry_for_cell(10.0, 20.0, &glyph, 9.0, 22.0),
            (
                [10.0, 20.0],
                [9.0, 22.0],
                [0.0, 0.0, 0.5 * 9.0 / 11.0, 0.75 * 22.0 / 23.0]
            )
        );
    }

    #[test]
    fn box_drawing_clips_negative_top_bearing_to_current_row() {
        let glyph = GlyphEntry {
            uv: [0.0, 0.0, 0.5, 0.75],
            px_w: 11,
            px_h: 24,
            ascent_offset: -1.0,
            is_cell_graphic: true,
            ..entry(0)
        };
        assert_eq!(
            glyph_quad_geometry_for_cell(10.0, 20.0, &glyph, 10.0, 23.0),
            (
                [10.0, 20.0],
                [10.0, 23.0],
                [0.0, 0.75 / 24.0, 0.5 * 10.0 / 11.0, 0.75]
            )
        );
    }

    #[test]
    fn box_drawing_clips_native_horizontal_overhang_at_one_to_one_density() {
        let glyph = GlyphEntry {
            left_offset: -1.0,
            px_w: 12,
            px_h: 3,
            ascent_offset: 7.0,
            is_cell_graphic: true,
            ..entry(0)
        };
        assert_eq!(
            glyph_quad_geometry_for_cell(10.0, 20.0, &glyph, 10.0, 18.0),
            (
                [10.0, 27.0],
                [10.0, 3.0],
                [1.0 / 12.0, 0.0, 11.0 / 12.0, 1.0]
            )
        );
    }

    #[test]
    fn eviction_when_over_capacity() {
        let mut a = GlyphAtlas::new(2);
        assert_eq!(a.insert(key(1), entry(0)), None);
        assert_eq!(a.insert(key(2), entry(1)), None);
        // key(1) is LRU, gets evicted on the third insert.
        assert_eq!(a.insert(key(3), entry(2)), Some(key(1)));
        assert!(a.lookup(&key(1)).is_none());
        assert_eq!(a.lookup(&key(2)), Some(entry(1)));
        assert_eq!(a.lookup(&key(3)), Some(entry(2)));
        assert_eq!(a.len(), 2);
    }

    #[test]
    fn lookup_promotes_to_mru() {
        let mut a = GlyphAtlas::new(2);
        a.insert(key(1), entry(0));
        a.insert(key(2), entry(1));
        // Promote key(1); now key(2) is LRU.
        for _ in 0..8 {
            assert_eq!(a.lookup(&key(1)), Some(entry(0)));
        }
        assert_eq!(a.insert(key(3), entry(2)), Some(key(2)));
        assert_eq!(a.lookup(&key(1)), Some(entry(0)));
        assert!(a.lookup(&key(2)).is_none());
    }

    #[test]
    fn lru_eviction_follows_repeated_lookup_order() {
        let mut a = GlyphAtlas::new(3);
        a.insert(key(1), entry(1));
        a.insert(key(2), entry(2));
        a.insert(key(3), entry(3));
        for _ in 0..4 {
            assert!(a.lookup(&key(1)).is_some());
        }
        for _ in 0..4 {
            assert!(a.lookup(&key(2)).is_some());
        }

        assert_eq!(a.insert(key(4), entry(4)), Some(key(3)));
        assert_eq!(a.insert(key(5), entry(5)), Some(key(1)));
        assert!(a.lookup(&key(2)).is_some());
        assert!(a.lookup(&key(4)).is_some());
        assert!(a.lookup(&key(5)).is_some());
    }

    #[test]
    fn duplicate_insert_replaces_without_evicting() {
        let mut a = GlyphAtlas::new(2);
        a.insert(key(1), entry(0));
        a.insert(key(2), entry(1));
        // Re-insert key(1) with a different layer — should replace, not
        // evict, and key(2) should still be present.
        let updated = entry(99);
        assert_eq!(a.insert(key(1), updated), None);
        assert_eq!(a.lookup(&key(1)), Some(updated));
        assert_eq!(a.lookup(&key(2)), Some(entry(1)));
        assert_eq!(a.len(), 2);
    }

    #[test]
    fn capacity_zero_never_admits() {
        let mut a = GlyphAtlas::new(0);
        // Insert fails immediately, returning the rejected key as "evicted".
        assert_eq!(a.insert(key(1), entry(0)), Some(key(1)));
        assert!(a.lookup(&key(1)).is_none());
        assert_eq!(a.len(), 0);
    }

    #[test]
    fn clock_overflow_rebases_without_changing_lru_order() {
        let mut a = GlyphAtlas::new(3);
        a.insert(key(1), entry(1));
        a.insert(key(2), entry(2));
        a.insert(key(3), entry(3));
        a.clock = u64::MAX;

        assert!(a.lookup(&key(2)).is_some());
        assert_eq!(a.insert(key(4), entry(4)), Some(key(1)));
    }

    #[test]
    fn clear_drops_everything() {
        let mut a = GlyphAtlas::new(4);
        a.insert(key(1), entry(0));
        a.insert(key(2), entry(1));
        a.clear();
        assert_eq!(a.len(), 0);
        assert!(a.lookup(&key(1)).is_none());
    }

    #[test]
    fn evict_oldest_returns_lru_pair() {
        // Insert in MRU-from-newest order; evict_oldest must return
        // the FIRST inserted (= LRU) pair. This is the load-bearing
        // ordering pin for WebGpuBackend's texture-layer reuse path.
        let mut a = GlyphAtlas::new(4);
        a.insert(key(1), entry(10));
        a.insert(key(2), entry(20));
        a.insert(key(3), entry(30));
        let (k, e) = a.evict_oldest().unwrap();
        assert_eq!(k, key(1));
        assert_eq!(e, entry(10));
        assert_eq!(a.len(), 2);
        // Subsequent calls evict in age order.
        let (k2, _) = a.evict_oldest().unwrap();
        assert_eq!(k2, key(2));
    }

    #[test]
    fn evict_oldest_returns_none_when_empty() {
        let mut a = GlyphAtlas::new(4);
        assert!(a.evict_oldest().is_none());
        a.insert(key(1), entry(0));
        a.evict_oldest();
        // Now empty again.
        assert!(a.evict_oldest().is_none());
    }

    #[test]
    fn evict_oldest_respects_lookup_promotion() {
        // lookup() promotes a key to MRU. After that, evict_oldest
        // must NOT pick it — it should evict the next-oldest instead.
        let mut a = GlyphAtlas::new(4);
        a.insert(key(1), entry(10));
        a.insert(key(2), entry(20));
        a.insert(key(3), entry(30));
        // Promote key(1) — now key(2) is LRU.
        let _ = a.lookup(&key(1));
        let (evicted, _) = a.evict_oldest().unwrap();
        assert_eq!(evicted, key(2));
    }

    // ─── pick_evictable_layer ────────────────────────────────────────
    //
    // Regression coverage for the "Claude TUI 历史输出字符不停刷新"
    // bug: WebGpuBackend's draw_row was reusing a layer that an earlier
    // instance in the same frame had already cited, so the GPU sampled
    // overwritten pixels and the earlier cell visually morphed into a
    // different glyph. `pick_evictable_layer` enforces the invariant
    // that pinned layers are never returned for reuse.

    #[test]
    fn pick_evictable_layer_skips_pinned_and_preserves_lookup() {
        let mut a = GlyphAtlas::new(5);
        // Insert 5 glyphs; LRU→MRU order is [0, 1, 2, 3, 4].
        for i in 0..5u16 {
            a.insert(key(i as u32), entry(i));
        }
        // Pin layers 0, 2, 4 — simulate an in-frame draw_row that
        // looked up keys with those layer ids first this frame.
        let pinned = [true, false, true, false, true];
        let written = [false; 5];

        // First eviction must return an unpinned layer (1 or 3).
        let first = pick_evictable_layer(&mut a, &pinned, &written);
        assert!(
            matches!(first, Some(1) | Some(3)),
            "expected an unpinned layer (1 or 3), got {first:?}"
        );

        // Second eviction returns the OTHER unpinned layer.
        let second = pick_evictable_layer(&mut a, &pinned, &written);
        assert!(
            matches!(second, Some(1) | Some(3)),
            "expected the remaining unpinned layer (1 or 3), got {second:?}"
        );
        assert_ne!(first, second, "must not return the same layer twice");

        // Third call: every remaining layer is pinned → None.
        let third = pick_evictable_layer(&mut a, &pinned, &written);
        assert_eq!(third, None, "all remaining layers pinned → must be None");

        // Critically: pinned keys must STILL be in the atlas after the
        // eviction walk. Without this invariant the bug morphs into
        // "pinned glyphs disappear" — also a regression but a different
        // visual symptom.
        assert!(a.lookup(&key(0)).is_some(), "pinned key 0 must survive");
        assert!(a.lookup(&key(2)).is_some(), "pinned key 2 must survive");
        assert!(a.lookup(&key(4)).is_some(), "pinned key 4 must survive");
    }

    #[test]
    fn pick_evictable_layer_returns_lru_when_nothing_pinned() {
        let mut a = GlyphAtlas::new(3);
        a.insert(key(10), entry(0)); // LRU after insert
        a.insert(key(11), entry(1));
        a.insert(key(12), entry(2));
        let pinned = [false, false, false];
        let written = [false; 3];
        // No pins → standard LRU rule wins → layer 0 (key 10).
        assert_eq!(pick_evictable_layer(&mut a, &pinned, &written), Some(0));
    }

    #[test]
    fn pick_evictable_layer_skips_written_layers() {
        let mut a = GlyphAtlas::new(3);
        a.insert(key(20), entry(0));
        a.insert(key(21), entry(1));
        a.insert(key(22), entry(2));
        let pinned = [false; 3];
        let written = [true, false, false];

        assert_eq!(pick_evictable_layer(&mut a, &pinned, &written), Some(1));
        // Written layer 0 was skipped before layer 1 and re-promoted. On a
        // fresh pin set, layer 2 remains older and must be selected first.
        assert_eq!(
            pick_evictable_layer(&mut a, &[false; 3], &[false; 3]),
            Some(2)
        );
        assert!(a.lookup(&key(20)).is_some());
        assert!(a.lookup(&key(21)).is_none());
        assert!(a.lookup(&key(22)).is_none());
    }

    #[test]
    fn pick_evictable_layer_returns_none_for_empty_atlas() {
        let mut a = GlyphAtlas::new(4);
        let pinned = [false; 4];
        let written = [false; 4];
        assert_eq!(pick_evictable_layer(&mut a, &pinned, &written), None);
    }

    #[test]
    fn pick_evictable_layer_re_insertion_does_not_corrupt_lru_order() {
        // After picking an unpinned layer past one pinned entry, the
        // remaining live atlas should still resolve all surviving keys
        // without dups or losses.
        let mut a = GlyphAtlas::new(4);
        a.insert(key(100), entry(0)); // pinned, will be skipped
        a.insert(key(101), entry(1)); // chosen for eviction
        a.insert(key(102), entry(2));
        a.insert(key(103), entry(3));
        let pinned = [true, false, false, false];
        let written = [false; 4];

        let chosen = pick_evictable_layer(&mut a, &pinned, &written);
        assert_eq!(chosen, Some(1));

        // Atlas should now contain keys 100, 102, 103 — exactly the
        // non-evicted ones, with no duplication of the re-inserted
        // pinned key.
        assert_eq!(a.len(), 3);
        assert!(a.lookup(&key(100)).is_some());
        assert!(a.lookup(&key(101)).is_none());
        assert!(a.lookup(&key(102)).is_some());
        assert!(a.lookup(&key(103)).is_some());
    }
}
