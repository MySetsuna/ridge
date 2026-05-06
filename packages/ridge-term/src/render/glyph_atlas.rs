//! Glyph atlas — content-addressed cache of rasterized glyphs.
//!
//! ## Purpose (TASKS §4.2 + OVERVIEW §D1)
//!
//! Today every Canvas2D backend pays the browser to rasterize each glyph
//! every frame via `fillText`. With WebGPU (§4.1) we'll upload glyph
//! bitmaps once into a texture array and reference them by `(layer, uv)`
//! at draw time — that's the speedup. The atlas IS the cache that maps
//! `GlyphKey → GlyphEntry`; texture management lives in `WebGpuBackend`.
//!
//! ## Decoupling from the renderer
//!
//! `GlyphAtlas` knows nothing about wgpu / web-sys / Canvas2D. It's a
//! pure data structure: insert + lookup + LRU eviction. This lets host
//! `cargo test --lib` run the eviction logic without a GPU and lets a
//! future Canvas2D path opt-in to atlas-based draw if metrics ever prove
//! that's faster than `fillText`.
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

use std::collections::HashMap;
use std::collections::VecDeque;

/// Cache key. Identifies a glyph variant by (font, size, codepoint
/// or font-internal id, weight/slant flags).
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
}

impl GlyphKey {
    pub const STYLE_BOLD: u8 = 0b01;
    pub const STYLE_ITALIC: u8 = 0b10;
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
    /// Bitmap pixel dimensions (pre-DPR). Backend may cross-check
    /// against the atlas slot it was uploaded to.
    pub px_w: u16,
    pub px_h: u16,
}

/// LRU-evicting cache. `lookup` promotes a key to the most-recently-used
/// position; `insert` pushes the least-recently-used out when at capacity
/// and returns the evicted key so the backend can free the texture slot.
pub struct GlyphAtlas {
    entries: HashMap<GlyphKey, GlyphEntry>,
    /// MRU at the back, LRU at the front. `O(n)` find on lookup; for
    /// realistic cache sizes (hundreds of unique glyphs per terminal
    /// session) this beats the constant-factor cost of an indexmap on
    /// stable Rust without an external dep.
    order: VecDeque<GlyphKey>,
    capacity: usize,
}

impl GlyphAtlas {
    /// Create an atlas with the given capacity. `capacity = 0` is a
    /// degenerate config that immediately rejects every insert; useful
    /// for testing the eviction path but not for production.
    pub fn new(capacity: usize) -> Self {
        Self {
            entries: HashMap::with_capacity(capacity),
            order: VecDeque::with_capacity(capacity),
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

    /// Returns `Some(entry)` on hit and promotes the key to MRU. `None`
    /// on miss — caller is responsible for rasterizing + `insert`.
    pub fn lookup(&mut self, key: &GlyphKey) -> Option<GlyphEntry> {
        let entry = *self.entries.get(key)?;
        if let Some(pos) = self.order.iter().position(|k| k == key) {
            self.order.remove(pos);
        }
        self.order.push_back(*key);
        Some(entry)
    }

    /// Insert a freshly-rasterized glyph. If the cache is at capacity,
    /// evicts the LRU entry and returns its key (caller frees the
    /// associated texture slot). A duplicate insert (same key) replaces
    /// the entry without eviction.
    pub fn insert(&mut self, key: GlyphKey, entry: GlyphEntry) -> Option<GlyphKey> {
        if self.entries.contains_key(&key) {
            self.entries.insert(key, entry);
            if let Some(pos) = self.order.iter().position(|k| *k == key) {
                self.order.remove(pos);
            }
            self.order.push_back(key);
            return None;
        }

        if self.capacity == 0 {
            // No room ever; reject. Return the rejected key so the
            // caller knows it was not stored (mirrors eviction shape).
            return Some(key);
        }

        let evicted = if self.entries.len() >= self.capacity {
            let victim = self.order.pop_front();
            if let Some(v) = victim {
                self.entries.remove(&v);
            }
            victim
        } else {
            None
        };

        self.entries.insert(key, entry);
        self.order.push_back(key);
        evicted
    }

    /// Drop everything. Backend should free all atlas slots after this.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.order.clear();
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
        let key = self.order.pop_front()?;
        let entry = self.entries.remove(&key)?;
        Some((key, entry))
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
pub fn pick_evictable_layer(atlas: &mut GlyphAtlas, pinned: &[bool]) -> Option<u32> {
    // Hold pinned entries we walk past so we can re-insert them after
    // we either find an evictable layer or exhaust the cache. Pre-
    // reserve 8: typical pin density is small (LRU is almost always
    // unpinned, so requeue stays length 0 on the steady-state hot
    // path). Under thrash the Vec grows in place.
    let mut requeue: Vec<(GlyphKey, GlyphEntry)> = Vec::with_capacity(8);
    let mut chosen: Option<u32> = None;
    while let Some((k, e)) = atlas.evict_oldest() {
        let layer = e.layer as usize;
        let is_pinned = pinned.get(layer).copied().unwrap_or(false);
        if !is_pinned {
            chosen = Some(e.layer as u32);
            break;
        }
        requeue.push((k, e));
    }
    // Restore every pinned entry we skipped so they remain in the
    // cache. Re-insertion places them at MRU which is correct: they're
    // being actively sampled by the current frame's draw queue.
    for (k, e) in requeue {
        atlas.insert(k, e);
    }
    chosen
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(id: u32) -> GlyphKey {
        GlyphKey {
            font_family_hash: 0xabc,
            font_size_q: 1400,
            glyph_id: id,
            style_flags: 0,
        }
    }

    fn entry(layer: u16) -> GlyphEntry {
        GlyphEntry {
            layer,
            uv: [0.0, 0.0, 1.0, 1.0],
            advance: 8.0,
            ascent_offset: 12.0,
            px_w: 8,
            px_h: 16,
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
        let _ = a.lookup(&key(1));
        assert_eq!(a.insert(key(3), entry(2)), Some(key(2)));
        assert_eq!(a.lookup(&key(1)), Some(entry(0)));
        assert!(a.lookup(&key(2)).is_none());
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

        // First eviction must return an unpinned layer (1 or 3).
        let first = pick_evictable_layer(&mut a, &pinned);
        assert!(
            matches!(first, Some(1) | Some(3)),
            "expected an unpinned layer (1 or 3), got {first:?}"
        );

        // Second eviction returns the OTHER unpinned layer.
        let second = pick_evictable_layer(&mut a, &pinned);
        assert!(
            matches!(second, Some(1) | Some(3)),
            "expected the remaining unpinned layer (1 or 3), got {second:?}"
        );
        assert_ne!(first, second, "must not return the same layer twice");

        // Third call: every remaining layer is pinned → None.
        let third = pick_evictable_layer(&mut a, &pinned);
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
        // No pins → standard LRU rule wins → layer 0 (key 10).
        assert_eq!(pick_evictable_layer(&mut a, &pinned), Some(0));
    }

    #[test]
    fn pick_evictable_layer_returns_none_for_empty_atlas() {
        let mut a = GlyphAtlas::new(4);
        let pinned = [false; 4];
        assert_eq!(pick_evictable_layer(&mut a, &pinned), None);
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

        let chosen = pick_evictable_layer(&mut a, &pinned);
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
