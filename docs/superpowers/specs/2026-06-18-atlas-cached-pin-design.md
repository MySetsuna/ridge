# 切换工作区瞬间乱码 — 字形图集「缓存层纳入帧内驱逐守卫」修复设计

> 用 superpowers:executing-plans 在本会话内顺序落地。步骤用 `- [ ]` 跟踪。

**Goal:** 消除切换工作区瞬间的字形乱码,让 WebGPU 在共享图集驱逐压力下也始终采样到正确字形。

**Architecture:** 全部 pane 共享单张 host canvas + 单个字形图集(纹理数组,1 字形/层,上限 1024)。
同一 host 帧内,`frame_written` 掩码禁止驱逐「本帧任何 pane 用过的层」,防止 A pane 已录 draw 仍要采样的层被 B pane 的 `write_texture` 覆盖。缺口:走 `record_cached_only` 快路重放的 pane **不标记**它引用的层 → 被同帧全量渲染的 pane 驱逐覆盖 → 提交时采样到错字。修复 = 让缓存 pane 在任何驱逐发生前把自己引用的层补进 `frame_written`,顺序无关、零坏帧。

**Tech Stack:** Rust → wasm(`packages/ridge-term`,wasm-pack 产出 `pkg/`)+ TypeScript(`src/lib/terminal/manager.ts`,RAF 渲染循环)。

## Global Constraints

- 代码注释沿用 `ridge-term`(英文 + 章节标记如 `§4b`)与 `manager.ts`(英文)既有风格。
- 不破坏 Canvas2D 后端:`pin_cached_layers` 对 Canvas2D 是 no-op。
- 不破坏旧 wasm 兼容:JS 侧用 `typeof handle.pinCachedLayers === 'function'` 守卫(同 `recordCachedOnly` 的既有降级模式)。
- 测试约束:webgpu.rs 多为 wasm32-only,主机单测 + 无头 GPU 均不可用(见 memory `project_test_harness_limitation`)。本修复无可运行的自动化测试;以 `wasm-pack` 构建通过 + 应用内目视切换无乱码为验收。不写手测清单。

## 根因证据链(已逐行核实)

| 事实 | 位置 |
|---|---|
| 共享图集 ≤1024 层,1 字形/层 | `gpu_context.rs:76` `ATLAS_LAYERS_MAX`、`:81` `ATLAS_RESERVED_LAYERS=1` |
| 跨 pane 帧内守卫 `frame_written`,host 帧起点 reset 一次 | `gpu_context.rs:165`、`surface_host.rs:277`(在 `begin_frame`) |
| 驱逐跳过 `frame_pinned`(本 pane)+ `frame_written`(本帧任意 pane) | `gpu_context.rs:673` `pick_evictable_layer(...)` |
| 全量渲染逐格 admit 时标记 `frame_written` | `gpu_context.rs:660-661,676-678`;`webgpu.rs:633-635,648-649` |
| **`record_cached_only` 重放缓存实例,不重历字形 → 不标 `frame_written`** | `webgpu.rs:1400-1449` |
| **`end_frame` 只存 `cached_n_cells`/`cached_evictions_seen`,不存引用层集合** | `webgpu.rs:1361-1365` |
| `cached_evictions_seen` 守卫只能下一帧纠正(事后探测,非事前预防) | `webgpu.rs:1421-1424` |

切换工作区时新可见 + 仍在刷日志的 pane 批量 admit 新字形 → 高驱逐;同帧的静态 pane 走缓存重放,其层未受保护 → 被驱逐覆盖 → 乱码。过渡期反复发生,稳定后自愈,与现象吻合。

## File Structure

| 文件 | 职责 / 改动 |
|---|---|
| `packages/ridge-term/src/render/webgpu.rs` | `WebGpuPaneBackend` 新增 `cached_layers: Vec<u16>` 字段;`end_frame` 填充;新增 inherent `pin_cached_layers()` 把层补进 `ctx.frame_written` |
| `packages/ridge-term/src/render/mod.rs` | `AnyBackend` inherent 块新增 `pin_cached_layers()` 分发(Webgpu→转发,Canvas2d→no-op) |
| `packages/ridge-term/src/render/renderer.rs` | `Renderer<AnyBackend>` wasm32 inherent 块新增 `pin_cached_layers()` 透传 |
| `packages/ridge-term/src/lib.rs` | `RenderHandle` 新增 `#[wasm_bindgen(js_name = pinCachedLayers)]` 导出 |
| `src/lib/terminal/manager.ts` | `ensureHostFrame()` 首次成功开帧后,对所有「可见且非脏」host pane 调 `pinCachedLayers()` |

镜像现有 `record_cached_only` 的三层包装(inherent on backend → inherent on `AnyBackend` → wasm32 inherent on `Renderer<AnyBackend>` → `RenderHandle` 导出),不上 `RenderBackend` trait。

---

## Task 1: WebGpuPaneBackend 记录并补钉缓存层

**Files:**
- Modify: `packages/ridge-term/src/render/webgpu.rs`

**Interfaces:**
- Produces: `WebGpuPaneBackend::pin_cached_layers(&mut self)` — 把 `cached_layers` 里的层在 `ctx.frame_written` 置位;字段 `cached_layers: Vec<u16>`。

- [ ] **Step 1: 加字段**。在 `cached_evictions_seen: u64,`(`webgpu.rs:215`)后新增:
```rust
    /// Distinct non-reserved atlas layers referenced by this pane's last
    /// successful `end_frame` instance upload. `pin_cached_layers` ORs
    /// these into the shared `frame_written` mask BEFORE any pane's
    /// full-render eviction runs this frame, so a cached-replay pane's
    /// already-recorded draw can't have its atlas slots evicted +
    /// overwritten mid-frame by another pane admitting new glyphs.
    cached_layers: Vec<u16>,
```

- [ ] **Step 2: 构造初始化**。在 `new()` 的 `Ok(Self { ... cached_evictions_seen: 0,`(`webgpu.rs:290`)后加 `cached_layers: Vec::new(),`。

- [ ] **Step 3: `end_frame` 填充**。在主路径尾部 `self.cached_evictions_seen = ctx.atlas_eviction_count;`(`webgpu.rs:1365`)后追加:
```rust
        // Record the distinct glyph layers this frame's instances cite so
        // `pin_cached_layers` can protect them next time we replay via
        // `record_cached_only`. Reserved layer 0 (backgrounds/clears) is
        // never an eviction candidate — skip it to keep the list tight.
        let mut layers: Vec<u16> = Vec::new();
        for inst in &self.pending_instances {
            let l = inst.atlas_layer;
            if l >= crate::render::gpu_context::ATLAS_RESERVED_LAYERS {
                let lu = l as u16;
                if !layers.contains(&lu) {
                    layers.push(lu);
                }
            }
        }
        self.cached_layers = layers;
```
并在 early-return 分支(`webgpu.rs:1336` `self.cached_n_cells = 0;` 之后)加 `self.cached_layers.clear();`。

- [ ] **Step 4: 实现 `pin_cached_layers`**。在 `record_cached_only`(`webgpu.rs:1400`)所在的 inherent `impl WebGpuPaneBackend` 块内新增:
```rust
    /// Re-pin this pane's cached glyph layers into the shared per-frame
    /// `frame_written` mask. Called by the host loop right after the host
    /// frame opens (mask just reset) and before any pane's full render —
    /// so eviction in `rasterize_and_admit` won't reclaim a layer that
    /// this pane's upcoming `record_cached_only` replay still samples.
    /// No-op when the cache is empty/invalid (then `record_cached_only`
    /// itself falls back to full render and re-marks layers as it admits).
    pub fn pin_cached_layers(&mut self) {
        if self.cached_n_cells == 0 || self.cached_layers.is_empty() {
            return;
        }
        let mut ctx = self.ctx.borrow_mut();
        for &l in &self.cached_layers {
            let idx = l as usize;
            if idx < ctx.frame_written.len() {
                ctx.frame_written[idx] = true;
            }
        }
    }
```

- [ ] **Step 5:** `cargo build` 通过(见 Task 6 的统一构建)。

## Task 2: AnyBackend 分发

**Files:**
- Modify: `packages/ridge-term/src/render/mod.rs`

**Interfaces:**
- Consumes: `WebGpuPaneBackend::pin_cached_layers`。
- Produces: `AnyBackend::pin_cached_layers(&mut self)`。

- [ ] **Step 1:** 在 inherent `impl AnyBackend` 块里 `record_cached_only`(`mod.rs:452-458`)之后新增:
```rust
    /// §atlas-pin: protect a cached pane's glyph layers from mid-frame
    /// eviction. No-op for Canvas2D (no shared atlas / frame_written).
    pub fn pin_cached_layers(&mut self) {
        match self {
            AnyBackend::Canvas2d(_) => {}
            #[cfg(feature = "webgpu")]
            AnyBackend::Webgpu(b) => b.pin_cached_layers(),
        }
    }
```

## Task 3: Renderer 透传

**Files:**
- Modify: `packages/ridge-term/src/render/renderer.rs`

**Interfaces:**
- Consumes: `AnyBackend::pin_cached_layers`。
- Produces: `Renderer<AnyBackend>::pin_cached_layers(&mut self)`。

- [ ] **Step 1:** 在 wasm32 inherent 块(`renderer.rs:718-723`,含 `record_cached_only`)内新增:
```rust
    pub fn pin_cached_layers(&mut self) {
        self.backend.pin_cached_layers()
    }
```

## Task 4: RenderHandle wasm 导出

**Files:**
- Modify: `packages/ridge-term/src/lib.rs`

**Interfaces:**
- Consumes: `Renderer::pin_cached_layers`。
- Produces: JS 侧 `handle.pinCachedLayers(): void`。

- [ ] **Step 1:** 在 `record_cached_only`(`lib.rs:1325-1328`)之后新增:
```rust
        /// §atlas-pin: before this frame's panes full-render, pin every
        /// visible cached pane's glyph layers so another pane's glyph
        /// admission can't evict + overwrite a layer this pane's
        /// `recordCachedOnly` replay still samples. Caller: `manager.ts`
        /// host loop, right after the host frame opens.
        #[wasm_bindgen(js_name = pinCachedLayers)]
        pub fn pin_cached_layers(&mut self) {
            self.renderer.pin_cached_layers();
        }
```

## Task 5: manager.ts 在开帧后、渲染前 pin 所有缓存 pane

**Files:**
- Modify: `src/lib/terminal/manager.ts`(`ensureHostFrame`,`manager.ts:4541-4546`)

**Interfaces:**
- Consumes: `handle.pinCachedLayers()`;预存的 `dirtyByPane`、`frameOrder`。

- [ ] **Step 1:** 将 `ensureHostFrame` 改为:
```ts
				const ensureHostFrame = (): boolean => {
					if (hostFrameOpen) return true;
					if (!activeHost) return false;
					hostFrameOpen = activeHost.beginFrame(themeBg);
					if (hostFrameOpen) {
						// §atlas-pin: beginFrame just reset the shared
						// `frame_written` mask. Pin every visible NOT-dirty
						// host pane's cached atlas layers NOW — before any
						// dirty pane's full render can evict + overwrite a
						// layer a cached replay still samples. Order-
						// independent: all cached panes protected before the
						// first eviction this frame. Eliminates the garbled
						// glyphs seen for a few frames right after a
						// workspace switch (cached pane's slot stolen by the
						// newly-visible pane's glyph admission).
						for (const e of frameOrder) {
							if (e.parked) continue;
							if (dirtyByPane.get(e.paneId) !== false) continue;
							const h = e.handle as unknown as {
								pinCachedLayers?: () => void;
							} | null;
							if (h !== null && typeof h.pinCachedLayers === 'function') {
								try {
									h.pinCachedLayers();
								} catch {
									/* old wasm bundle w/o export → skip */
								}
							}
						}
					}
					return hostFrameOpen;
				};
```
说明:`dirtyByPane.get(e.paneId) !== false` 精确选中「在 map 内且为 false」= 当前活动工作区、可见、host 模式、非脏的 pane(脏 pane 走全量渲染时自标 `frame_written`,无需预钉;其它工作区/隐藏/canvas2d pane 不在 map 内,返回 undefined 被跳过)。

## Task 6: 构建 wasm + 验证

- [ ] **Step 1:** 重建 ridge-term wasm(确认仓库实际脚本,优先 `package.json` 里的 wasm 构建命令;否则 `wasm-pack build packages/ridge-term --target web --out-dir pkg <既有 features>`)。
- [ ] **Step 2:** 构建零错误(尤其确认 `ATLAS_RESERVED_LAYERS` 路径可达、三层透传签名一致)。
- [ ] **Step 3:** 应用内验证:打开多 pane、CJK 密集的两个工作区,反复来回切换 → 切换瞬间不再出现错字乱码。
- [ ] **Step 4:** 回归确认:单工作区打字、其它 pane 刷日志时无新乱码;Canvas2D 回退路径(`RIDGE_WEBGPU=0`)正常。

## Self-Review

- 覆盖:根因(缓存路径不标 `frame_written`)由 Task 1(记录层 + 补钉)+ Task 5(开帧后渲染前补钉)闭合,顺序无关。
- 签名一致:`pin_cached_layers` 全链同名;JS `pinCachedLayers`。
- 退化路径:Canvas2D no-op;旧 wasm 由 `typeof` 守卫降级;缓存失效时 `pin_cached_layers` 早返回、`record_cached_only` 自行回退全量渲染。
- 边界:补钉可能保护到「即将回退全量渲染的陈旧层」,极端满图集下或挤占新字形 → bg-only。但切换后旧工作区层已可驱逐、腾出空间,单工作区可见字形通常远低于 1024,风险可忽略,且远优于乱码。
