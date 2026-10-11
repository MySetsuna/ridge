//! GPU presentation over a read-only viewport; owns no terminal parser or history.
use wasm_bindgen::prelude::*;
pub use ridge_term::render;
#[wasm_bindgen(start)]
pub fn init() { console_error_panic_hook::set_once(); }
#[cfg(all(target_arch = "wasm32", feature = "webgpu"))]
mod renderer_js {
    use super::*;
    use crate::render::backend::RenderBackend;
    use crate::render::webgpu::WebGpuPaneBackend;
    use crate::render::{FrameMetrics, Renderer, Theme};
    use web_sys::HtmlCanvasElement;

    #[wasm_bindgen]
    pub struct RenderHandle {
        renderer: Renderer<WebGpuPaneBackend>,
        view: ridge_term::presentation::RenderView,
    }

    #[wasm_bindgen]
    impl RenderHandle {
        /// Async WebGPU-first constructor with wgpu WebGL2 fallback.
        /// `surface_host` owns the shared presentation surface; failures are returned to
        /// JavaScript with the stable `WEBGPU_INIT_FAILED:` prefix.
        #[wasm_bindgen(js_name = newWithWebgpuFirst)]
        pub async fn new_with_webgpu_first(
            _canvas: HtmlCanvasElement,
            surface_host: SurfaceHostHandle,
        ) -> Result<RenderHandle, JsValue> {
            let backend = WebGpuPaneBackend::new(surface_host.host_rc())
                .await
                .map_err(|error| JsValue::from_str(&format!("WEBGPU_INIT_FAILED: {error}")))?;
            let metrics = FrameMetrics {
                cell_w: 8.0,
                cell_h: 16.0,
                dpr: 1.0,
                tui_mode: false,
            };
            let renderer = Renderer::new(backend, metrics, Theme::default_dark());
            Ok(RenderHandle {
                view: ridge_term::presentation::RenderView::default(), renderer })
        }

        /// Configure font + measure cell dimensions. Returns [cell_w, cell_h]
        /// in CSS pixels so JS can calculate cols/rows for a target
        /// container size.
        pub fn configure(
            &mut self,
            font_family: String,
            font_size_px: f32,
            dpr: f32,
        ) -> Result<Vec<f32>, JsValue> {
            // Keep font family and size together so the glyph atlas uses
            // the selected system-font fallback chain.
            self.renderer
                .backend_mut()
                .set_font_config(font_family.clone(), font_size_px);
            let (w, h) = self
                .renderer
                .backend_mut()
                .measure_font(&font_family, font_size_px)
                .map_err(JsValue::from)?;
            self.renderer.set_metrics(FrameMetrics {
                cell_w: w,
                cell_h: h,
                dpr,
                tui_mode: false,
            });
            Ok(vec![w, h])
        }

        pub fn resize(&mut self, width_css: u32, height_css: u32, dpr: f32) -> Result<(), JsValue> {
            self.renderer
                .backend_mut()
                .resize_surface(width_css, height_css, dpr)
                .map_err(JsValue::from)?;
            self.renderer.set_dpr(dpr);
            self.renderer.invalidate_all();
            Ok(())
        }

        /// Phase B: record the pane's `(x, y)` position on the host
        /// canvas in **device pixels**. JS calls this from
        /// `manager.ts::_recomputeViewport` whenever the splitter drag
        /// moves the pane's container without changing its size.
        ///
        /// Forward the host-canvas offset to WebGPU. Does **not**
        /// trigger a redraw on its own — the pane content is unchanged
        /// on a positional shift; JS calls `surfaceHost.invalidate()`
        /// after layout settle to clear the old area.
        #[wasm_bindgen(js_name = setViewportOffset)]
        pub fn set_viewport_offset(&mut self, x: u32, y: u32) {
            self.renderer.backend_mut().set_viewport_offset(x, y);
        }

        /// Apply a theme. Accepts a JS object with any subset of:
        ///   { background, foreground, cursor, cursorAccent,
        ///     black, red, green, yellow, blue, magenta, cyan, white,
        ///     brightBlack, brightRed, brightGreen, brightYellow,
        ///     brightBlue, brightMagenta, brightCyan, brightWhite }
        /// Each value is a CSS hex color "#rgb" / "#rrggbb" / "#rrggbbaa".
        /// Keys not provided keep their existing palette entry.
        ///
        /// Note: this is **partial overrides on top of the current theme**,
        /// not a full replace. To reset, call `applyDefaultTheme()` first.
        #[wasm_bindgen(js_name = applyTheme)]
        pub fn apply_theme(&mut self, theme_obj: JsValue) -> Result<(), JsValue> {
            // We use Reflect rather than serde-wasm-bindgen to avoid the
            // dep cost — the API is small enough that field-by-field
            // lookup is fine.
            let mut t = self.current_theme();
            t.apply_partial(|key| {
                let val = js_sys::Reflect::get(&theme_obj, &JsValue::from_str(key)).ok()?;
                val.as_string()
            });
            self.renderer.set_theme(t);
            Ok(())
        }

        /// Reset to the default dark theme.
        #[wasm_bindgen(js_name = applyDefaultTheme)]
        pub fn apply_default_theme(&mut self) {
            self.renderer.set_theme(Theme::default_dark());
        }

        /// Drive one frame from the kernel's current grid. Returns true
        /// if anything was drawn (caller can use this to decide whether
        /// to schedule another frame). Selection range comes from the
        /// kernel's `selection` field — when set, the renderer paints a
        /// translucent overlay over those cells. Wall-clock comes from
        /// `Date.now()` for cursor-blink phase.
        pub fn render(&mut self) -> bool {
            let scroll_ops = std::mem::take(&mut self.view.scroll_ops);
            self.renderer.tick_with_scroll(&self.view, self.view.selection(), js_sys::Date::now(), &scroll_ops)
        }

        #[wasm_bindgen(js_name = applyPresentation)]
        pub fn apply_presentation(&mut self, bytes: &[u8]) -> Result<u32, JsValue> {
            if self.view.apply(bytes).map_err(|error| JsValue::from_str(&error))? { self.renderer.request_full_redraw(); }
            Ok(self.view.revision())
        }

        /// Force a full redraw on the next render() — useful after
        /// invalidating external state without using the dedicated setters.
        #[wasm_bindgen(js_name = invalidateAll)]
        pub fn invalidate_all(&mut self) {
            self.renderer.invalidate_all();
        }

        /// Force one complete viewport repaint while preserving the shared
        /// glyph atlas. Used after SurfaceHost structurally seeds its backing
        /// store; unlike `invalidateAll`, this does not discard valid glyphs.
        #[wasm_bindgen(js_name = repaintAll)]
        pub fn repaint_all(&mut self) {
            self.renderer.request_full_redraw();
        }

        /// Multi-pane hosts call this when the active pane changes. When
        /// `focused` is false, the renderer skips cursor draw entirely so
        /// only the truly active terminal blinks. Idempotent.
        #[wasm_bindgen(js_name = setFocused)]
        pub fn set_focused(&mut self, focused: bool) {
            self.renderer.set_focused(focused);
        }

        #[wasm_bindgen(js_name = setLinkUnderlines)]
        pub fn set_link_underlines(&mut self, regions: JsValue) -> Result<(), JsValue> {
            #[derive(serde::Deserialize)]
            struct Region { row: usize, c0: usize, c1: usize }
            let regions: Vec<Region> = serde_wasm_bindgen::from_value(regions)
                .map_err(|error| JsValue::from_str(&error.to_string()))?;
            if regions.len() > 16_384 { return Err(JsValue::from_str("too many link regions")); }
            self.renderer.set_link_underlines(regions.into_iter()
                .filter(|region| region.c1 > region.c0)
                .map(|region| (region.row, region.c0, region.c1)).collect());
            Ok(())
        }

        /// Presentation-only gate for a short native inline-TUI repaint walk.
        /// Grid cells still render immediately; while set, the renderer
        /// freezes the cursor at its last presented position until the
        /// browser compositor settles. Name retained for JS compatibility.
        #[wasm_bindgen(js_name = setPresentationCursorSuppressed)]
        pub fn set_presentation_cursor_suppressed(&mut self, suppressed: bool) {
            self.renderer.set_presentation_cursor_suppressed(suppressed);
        }

        /// DEV/e2e probe for the cell currently presented, including a freeze.
        /// `-1` means no cursor is on screen. Not used by the compositor loop.
        #[wasm_bindgen(js_name = presentedCursorRow)]
        pub fn presented_cursor_row(&self) -> i32 {
            self.renderer
                .presented_cursor_cell()
                .map(|(row, _)| row as i32)
                .unwrap_or(-1)
        }

        #[wasm_bindgen(js_name = presentedCursorCol)]
        pub fn presented_cursor_col(&self) -> i32 {
            self.renderer
                .presented_cursor_cell()
                .map(|(_, col)| col as i32)
                .unwrap_or(-1)
        }

        /// Install an IME preedit overlay at the given cell. The renderer
        /// will paint `text` on top of the cell grid each frame until
        /// `clearPreedit` is called. Cells themselves are NOT modified,
        /// so a TUI re-rendering into the overlay's row mid-composition
        /// can't corrupt the preedit, and the preedit can't corrupt the
        /// TUI's rendered cells. JS calls this on `compositionupdate`.
        #[wasm_bindgen(js_name = setPreedit)]
        pub fn set_preedit(&mut self, text: &str, row: usize, col: usize) {
            self.renderer.set_preedit(text.to_string(), row, col);
        }

        /// Remove the preedit overlay (JS calls on `compositionend` after
        /// shipping the committed string to the PTY).
        #[wasm_bindgen(js_name = clearPreedit)]
        pub fn clear_preedit(&mut self) {
            self.renderer.clear_preedit();
        }

        /// §1.34 (2026-05-22) — install the shell-history popup overlay.
        /// `items` is the JS-pre-windowed VISIBLE slice (filtered, newest
        /// first). `selected_index` is `-1` for "no row picked" or a
        /// slice-relative `0..items.len()-1`. `(anchor_row, anchor_col)`
        /// is the input anchor in viewport cell coords; `place_above`
        /// chooses growth direction.
        ///
        /// §history-scroll — `total_items` is the FULL filtered count and
        /// `first_visible` is the index of `items[0]` within it; the
        /// renderer draws a scrollbar thumb from these when the list is
        /// longer than the window (Warp-style: many entries reachable by
        /// scrolling, with a position indicator). JS owns the windowing so
        /// the renderer just paints the slice + the thumb.
        #[wasm_bindgen(js_name = setHistoryOverlay)]
        pub fn set_history_overlay(
            &mut self,
            items: js_sys::Array,
            selected_index: i32,
            anchor_row: u32,
            anchor_col: u32,
            place_above: bool,
            total_items: u32,
            first_visible: u32,
            viewport_cols: u32,
            viewport_rows: u32,
        ) {
            let items: Vec<String> = items.iter().filter_map(|v| v.as_string()).collect();
            // JS pre-windows to the visible slice; render all of it (capped
            // to a sane ceiling as a floor-to-ceiling guard).
            let max_visible_rows = items.len().min(40);
            self.renderer
                .set_history_overlay(crate::render::renderer::HistoryOverlay {
                    items,
                    selected_index,
                    anchor_row: anchor_row as usize,
                    anchor_col: anchor_col as usize,
                    place_above,
                    max_visible_rows,
                    total_items: total_items as usize,
                    first_visible: first_visible as usize,
                    viewport_cols: viewport_cols as usize,
                    viewport_rows: viewport_rows as usize,
                });
        }

        /// §1.34 — remove the history overlay (Enter / ArrowRight / Esc).
        #[wasm_bindgen(js_name = clearHistoryOverlay)]
        pub fn clear_history_overlay(&mut self) {
            self.renderer.clear_history_overlay();
        }

        /// Non-mutating mirror of `render`'s early-exit conditions:
        /// returns `true` when the next `render` call would do any
        /// drawing work, `false` when the renderer has nothing to
        /// redraw and the JS caller can sleep its RAF loop. `now_ms`
        /// must use the same epoch as the value passed to `render`
        /// (`Date.now()` in JS).
        ///
        /// Cost: one row-revision compare per visible row plus selection /
        /// scroll / blink checks. A revision-mismatched row additionally
        /// compares its exact painted content so a settled TUI rewrite does
        /// not schedule a redundant compositor frame.
        #[wasm_bindgen(js_name = isDirty)]
        pub fn is_dirty(&self, now_ms: f64) -> bool {
            !self.view.scroll_ops.is_empty() || self.renderer.is_dirty(&self.view, self.view.selection(), now_ms)
        }

        /// Milliseconds until the next cursor-blink phase boundary. JS
        /// callers use this to schedule a `setTimeout` wake-up while
        /// the RAF loop is paused. Returns a very large number
        /// (effectively infinity) when the cursor isn't blinking — the
        /// caller should treat any value > some reasonable cap (e.g.
        /// 1000 ms) as "no blink, sleep at most a second on a watchdog".
        #[wasm_bindgen(js_name = nextBlinkDeadlineMs)]
        pub fn next_blink_deadline_ms(&self, now_ms: f64) -> f64 {
            self.renderer.next_blink_deadline_ms(&self.view, now_ms)
        }

        /// Internal: snapshot the current theme so apply_partial can layer
        /// overrides on top of it. Renderer doesn't expose `theme()`
        /// directly — we reach in via a fresh default + reapply isn't
        /// right either (would lose previous overrides). Solution:
        /// renderer needs a getter. Add one.
        fn current_theme(&self) -> Theme {
            self.renderer.theme().clone()
        }

        /// Diagnostic: return the kernel-side renderer.theme.{bg, fg,
        /// cursor_color, tui_bg} as a Uint8Array of 16 bytes (4×RGBA).
        /// Lets JS confirm whether `applyTheme` actually propagated into
        /// the renderer state — the JS-side `opts.theme` snapshot only
        /// proves the manager *received* the theme, not that the
        /// wasm renderer accepted it. Cheap (one Theme clone, 16 bytes
        /// copied) so callers may poll without harm.
        #[wasm_bindgen(js_name = currentThemeProbe)]
        pub fn current_theme_probe(&self) -> Box<[u8]> {
            let t = self.current_theme();
            let mut out = Vec::with_capacity(16);
            out.extend_from_slice(&t.bg);
            out.extend_from_slice(&t.fg);
            out.extend_from_slice(&t.cursor_color);
            out.extend_from_slice(&t.tui_bg);
            out.into_boxed_slice()
        }

        /// Return the active rendering backend name.
        #[wasm_bindgen(js_name = backendName)]
        pub fn backend_name(&self) -> String {
            self.renderer.backend().backend_name().to_string()
        }
    }

    // ──────────────────────────────────────────────────────────────
    // SurfaceHostHandle (§A.8 per-workspace)
    //
    // JS-facing wrapper around a `SurfaceHost`. One instance per
    // workspace tab — `manager.ts` holds a Map<workspaceId, handle>
    // and passes the matching handle to each pane's
    // `RenderHandle.newWithWebgpuFirst(canvas, host)` so the pane's
    // WebGPU draws land on its workspace's canvas.
    //
    // Per-frame protocol from JS RAF (active workspace only):
    //   1. host.beginFrame(themeBg)         — acquire swap-chain texture
    //   2. for each dirty pane in this workspace: handle.render(kernel)
    //   3. host.endFrame()                   — submit + present
    // ──────────────────────────────────────────────────────────────

    #[cfg(feature = "webgpu")]
    #[wasm_bindgen]
    #[derive(Clone)]
    pub struct SurfaceHostHandle {
        host: std::rc::Rc<std::cell::RefCell<crate::render::surface_host::SurfaceHost>>,
    }

    #[cfg(feature = "webgpu")]
    impl SurfaceHostHandle {
        /// Internal accessor: the per-workspace SurfaceHost Rc. Used by
        /// `RenderHandle::new_with_webgpu_first` so newly-constructed
        /// `WebGpuPaneBackend`s share the same SurfaceHost (and thus
        /// the same `<canvas data-rg-ws-host>`) as their workspace tab.
        pub(crate) fn host_rc(
            &self,
        ) -> std::rc::Rc<std::cell::RefCell<crate::render::surface_host::SurfaceHost>> {
            self.host.clone()
        }
    }

    #[cfg(feature = "webgpu")]
    #[wasm_bindgen]
    impl SurfaceHostHandle {
        /// JS-callable clone: produces a new `SurfaceHostHandle` JS
        /// wrapper that bumps the inner `Rc` refcount. Required because
        /// `RenderHandle::newWithWebgpuFirst(canvas, host)` consumes
        /// its `host` parameter (wasm-bindgen `Option<T>` semantics —
        /// the JS-side wrapper is freed after the call). When N panes
        /// in the same workspace each call attach, JS must
        /// `host.clone()` per call so the manager's stored handle
        /// stays alive.
        #[wasm_bindgen(js_name = clone)]
        pub fn js_clone(&self) -> SurfaceHostHandle {
            Clone::clone(self)
        }
    }

    #[cfg(feature = "webgpu")]
    #[wasm_bindgen]
    impl SurfaceHostHandle {
        /// Async constructor: create one swap chain bound to `canvas`.
        /// One SurfaceHostHandle per workspace tab — JS holds a Map
        /// keyed by workspace id and passes the matching handle to
        /// each pane's `RenderHandle.newWithWebgpuFirst(canvas, host)`.
        ///
        /// Returns `Err` (rejected promise on the JS side) when both browser
        /// GPU backends fail adapter/device acquisition or
        /// `instance.create_surface` rejects the canvas. JS catches
        /// and surfaces a structured initialization error to JavaScript.
        #[wasm_bindgen(js_name = init)]
        pub async fn init(canvas: HtmlCanvasElement) -> Result<SurfaceHostHandle, JsValue> {
            let host = crate::render::surface_host::SurfaceHost::init(canvas)
                .await
                .map_err(JsValue::from)?;
            Ok(SurfaceHostHandle { host })
        }

        /// Resize the host swap chain. JS drives this from a
        /// ResizeObserver on the host canvas's parent so the surface
        /// always matches the visible workspace area.
        pub fn resize(&self, width_css: u32, height_css: u32, dpr: f32) -> Result<(), JsValue> {
            self.host
                .borrow_mut()
                .resize(width_css, height_css, dpr)
                .map_err(JsValue::from)
        }

        /// Mark the next frame for a fresh `LoadOp::Clear`. JS calls
        /// this when a pane detaches / parks / unparks (so departed
        /// pixels don't linger), when the theme changes, and after
        /// splitter settle moves pane boundaries.
        pub fn invalidate(&self) {
            self.host.borrow_mut().invalidate();
        }

        /// True when the compositor-owned frame store must be seeded before
        /// the next present. This also covers device/surface recovery that did
        /// not originate from a JS `invalidate()` call.
        #[wasm_bindgen(js_name = needsFullSeed)]
        pub fn needs_full_seed(&self) -> bool {
            self.host.borrow().needs_full_seed()
        }

        /// Begin one host frame: acquire swap-chain texture + create
        /// encoder. Returns `true` on success, `false` on surface-lost
        /// — JS skips the rest of the frame and lets the next RAF
        /// retry. `theme_bg` is a 4-byte RGBA buffer; values outside
        /// `[0..255]` get clamped at the byte boundary by
        /// `Uint8Array.set`.
        ///
        /// Idempotent guard: a second call without an intervening
        /// `endFrame` drops the stale frame and starts fresh (defense
        /// against JS bugs that skip the end half).
        #[wasm_bindgen(js_name = beginFrame)]
        pub fn begin_frame(&self, theme_bg: &[u8]) -> bool {
            let mut rgba = [0u8; 4];
            // Clamp short input to opaque-on-black so we never panic on
            // a malformed slice; JS callers are expected to send
            // exactly 4 bytes.
            for (i, &b) in theme_bg.iter().take(4).enumerate() {
                rgba[i] = b;
            }
            if theme_bg.len() < 4 {
                rgba[3] = 255;
            }
            self.host.borrow_mut().begin_frame(rgba)
        }

        /// Finish the host's command encoder + queue.submit + present.
        /// One call per frame after all dirty panes have rendered. Safe
        /// to call without a matching `beginFrame` — internal guard
        /// returns early.
        #[wasm_bindgen(js_name = endFrame)]
        pub fn end_frame(&self) {
            self.host.borrow_mut().end_frame();
        }

        /// Upload a wallpaper image to the GPU and enable the wallpaper
        /// pass. `rgba` must be a packed RGBA byte slice
        /// (`width * height * 4` bytes). `opacity` is in `[0.0, 1.0]`;
        /// 0 = full theme background colour, 1 = fully opaque image.
        ///
        /// Forwards to `SurfaceHost::set_wallpaper`, which in turn
        /// delegates to `GpuContext::set_wallpaper` (texture upload +
        /// bind-group rebuild) and calls `invalidate()` so the change
        /// takes effect on the next `beginFrame`.
        ///
        /// Only available under `#[cfg(feature = "webgpu")]` — the JS
        /// side must guard accordingly.
        #[wasm_bindgen(js_name = setWallpaper)]
        pub fn set_wallpaper(&self, rgba: &[u8], w: u32, h: u32, opacity: f32) {
            self.host.borrow_mut().set_wallpaper(rgba, w, h, opacity);
        }

        /// Remove the wallpaper and revert to plain theme-colour clear
        /// on the next `beginFrame`. Idempotent — safe to call even
        /// when no wallpaper is currently loaded.
        ///
        /// Forwards to `SurfaceHost::clear_wallpaper` → `GpuContext::
        /// clear_wallpaper` (drops texture + bind group) and calls
        /// `invalidate()`.
        #[wasm_bindgen(js_name = clearWallpaper)]
        pub fn clear_wallpaper(&self) {
            self.host.borrow_mut().clear_wallpaper();
        }
    }
}

#[cfg(all(target_arch = "wasm32", feature = "webgpu"))]
pub use renderer_js::RenderHandle;

#[cfg(all(target_arch = "wasm32", feature = "webgpu"))]
pub use renderer_js::SurfaceHostHandle;

