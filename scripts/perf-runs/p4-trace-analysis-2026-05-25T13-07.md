P4 Trace Analysis Report
========================

Date: 2026-05-25T13:07
Trace file: scripts/perf-runs/p4-trace-2026-05-25T10-00.json
Workload: writePty '1..5000 | %{ ESC+SGR + "line N".PadRight(80,"X") + ESC+reset }'
Trace method: CDP Performance.trace, 15s capture

## 1. 91% Wall Time Distribution (Top 3 categories)

The trace captured events across three key threads in the WebView2 process. Our JS
perf marks (`performance.mark`/`measure` for `rg.frame.tick`, `rg.ptyDelta.apply`)
appear on **CrRendererMain** as `UserTiming::Measure` and account for only **363ms
(0.9%)** of the ~39.6s of non-wrapper work tracked on that thread.

### Top 1: JS Execution on CrRendererMain — ~60%

**23,831ms / 39,626ms on CrRendererMain**

| Component | Time | Share |
|---|---|---|
| RunMicrotasks (Promise queue) | 10,050ms | 25.4% |
| v8.callFunction | 5,152ms | 13.0% |
| FunctionCall (JS execution) | 4,708ms | 11.9% |
| FireAnimationFrame (rAF tick) | 3,921ms | 9.9% |
| TimerFire (setTimeout/setInterval) | 851ms | 2.1% |

The rAF tick (our `manager.ts::tick()`) takes 3.9s but our perf mark only captures
~2ms of in-tick work. The gap is Svelte reactivity, DOM diffing, and browser
internals triggered by our state changes.

### Top 2: Garbage Collection on CrRendererMain — ~15%

**~6,000ms / 39,626ms on CrRendererMain**

| Component | Time | Count |
|---|---|---|
| MajorGC + V8.GC_MARK_COMPACTOR | 1,322ms | 68× |
| V8.GC_MC_INCREMENTAL + IncrementalMarking | 881ms | 8,000× |
| MinorGC + GCScavenger + GC_SCAVENGER | 1,289ms | 165× |
| V8.GCFinalizeMC | 448ms | 22× |
| Other GC (background marking, sweeping) | ~2,000ms | — |

GC is the primary p99 long-tail driver. A single MajorGC takes up to **662ms**,
which can stall rendering for 6+ frames. 34 MajorGC events in 15s = ~2/sec.

### Top 3: Tauri IPC (loading/URLLoader) — ~25%

**10,055ms / 39,626ms on CrRendererMain**

Tauri's URLLoader dispatching Channel<DeltaPayload> events accounts for 25% of
renderer thread work. This confirms P4.3 (binary channel) is active, but the
deserialization + dispatch overhead is still significant.

### GPU/Compositor Threads (parallel, not additive to frame time)

| Thread | Total RunTask | Notes |
|---|---|---|
| GpuVSyncThread | 51,732ms | VSync + GPU back-pressure |
| CrGpuMain | 4,665ms | GPU command processing |
| VizCompositorThread | 1,935ms | Visual compositing |
| Compositor (renderer process) | 1,078ms | Renderer compositing |

The GPU VSync thread is heavily loaded (3.4× realtime), indicating the GPU is
back-pressured. This is a **symptom** of the renderer overloading the GPU queue,
not a root cause.

### CrBrowserMain (Tauri host)

| Category | Time | Share |
|---|---|---|
| RunTask (wrapper) | 11,129ms | 86.5% |
| loading (URLLoader) | 1,734ms | 13.5% |

The browser process main thread is mostly idle by comparison.

## 2. Recommended Next Step: P4.9 OffscreenCanvas Worker

**Move the entire WebGPU rendering pipeline to an OffscreenCanvas worker thread.**

The evidence chain:
1. CrRendererMain is 3× overloaded (46s work in 15s realtime)
2. GC stalls (662ms MajorGC) directly cause the p99=107ms frame tail
3. rAF callbacks (FireAnimationFrame) take 3.9s — offloading them frees the
   renderer thread

Implementation:
- The `transferControlToOffscreen` path already exists in `manager.ts`
- `RenderHandle::newFromOffscreen` is scaffolded in `packages/ridge-term/src/render/`
- Memory log shows Tier 1+2 scaffolded, need Tier 3 (full worker dispatch)

Expected impact:
- Remove rAF + FunctionCall + microtasks from CrRendererMain: ~60% reduction
- GC pressure isolated to worker thread: MajorGC won't stall CrRendererMain
- Tauri IPC (loading) overhead remains on CrRendererMain but no longer contends
  with rendering

## 3. Not Recommended Directions

| Direction | Reason Against |
|---|---|
| JS micro-optimizations | Our code is ~2ms per tick; optimizing it won't fix 662ms GC stalls |
| Throttle frame rate (e.g. 30fps) | Reduces throughput but doesn't fix GC p99 tail; also worsens UX |
| Accept status quo | p99=107ms exceeds original P4 target (20-25ms) and GC will worsen with scale |
| Reduce workload (fewer lines) | 5k lines is representative; 500 doesn't stress the pipeline |
| Optimize Tauri IPC | P4.3 binary channel already reduced text path to 0; remaining loading overhead is architectural and would require WebCodecs or shared memory |
| WebGPU per-frame batch optimization | tick body is only ~2ms; GPU submit is async from CrRendererMain perspective; batch optimization won't help the GC problem |
