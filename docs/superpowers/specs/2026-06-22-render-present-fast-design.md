# 渲染：IME 合成闪烁 / 选区闪烁 / 切屏字形乱码 —— `requires_full_frame` 运行时快路径(flag 门控)设计

> 日期：2026-06-22 ｜ 状态：**根因已定位（CDP 取证 + 逐行代码核实），落实现**
> 验证约束：dev 的 WebView2（148/149）**不暴露**这几个 present 期视觉症状（与 release 行为不同，见交接文档 [2026-06-18-selection-flash-firstline-handoff.md](2026-06-18-selection-flash-firstline-handoff.md)）。本修复**默认零行为变化**，由运行时 flag 在 **release** 上开启并验证。

## 用户报告

1. **切工作区瞬间字形乱码**（一闪而过、几帧自愈，多 pane / CJK 密集 / TUI 下明显）。
2. **打中文（唤出输入法合成）时到处闪烁**，尤其多 pane、TUI（claude/opencode 等 Ink 应用）下最明显；显示缩放 100%（dpr=1）。

## CDP 自测取证（dev，dpr=1）

逐项用 `pnpm tauri:dev:cdp` + chrome-devtools/playwright 驱动取证（注入大量中文、分屏、来回切、模拟 `Input.imeSetComposition` 合成、`window.__ridgeTraceBuf` 逐帧抓内核 cells）：

- **两个症状下内核 cells 始终有效**（无 U+FFFD、内容是正确的 CJK/prompt）→ **纯渲染层问题**，排除 parser/镜像/cells（即排除 9ec8ae4 那类 diff 基线方向）。
- IME 合成期 preedit 走 `RidgePane.onCompositionUpdate → manager.setPreedit()` 的**渲染器叠加层**（不经 PTY、不改 cells；ffd=0 已证），每个拼音键唤醒 ~1 次重绘。
- IME textarea 在 shell 与 alt-screen(vim) 下**均保持聚焦、定位正确**；合成期锚点被 `composingAnchor` 锁定（§P5.IME 已修「输入域到处乱跑」）→ **定位本身不是 bug**。
- dev 下两症状均**重现不出视觉闪/乱**（render loop 合成期仅 ~7–11fps、无滚动 nudge）。结合交接文档记录的 dev/release WebView2 `LoadOp` 差异，确认是 **release 特定的 present 期视觉症状**。

## 收敛的共同根因：`requires_full_frame()` 恒 `true`

`packages/ridge-term/src/render/webgpu.rs::requires_full_frame`（:339-373）当前**恒返回 `true`**。它是 2026-06-18 为「dev WebView2 148 的 `LoadOp::Load` 会丢交换链像素」刻意设的正确性兜底（注释 + TODO §1.36 + 交接文档明载，并预留「运行时能力探测后改回 `needs_initial_clear` 脏行快路径」）。

恒 `true` 的两个后果，正好对应两个症状：

1. **每帧整屏全量重编码 + 整屏 `LoadOp::Clear`** → 任何高频重绘触发器（PSReadLine 活动行重画 = 选区闪烁；IME 合成每键 `setPreedit` = 合成闪烁；TUI spinner）都让**整张共享 host canvas 每次都 Clear+全量呈现**；多 pane 共享 canvas → **所有 pane 一起闪**（「到处闪烁」）。
2. **每帧重新 admit 全部可见字形** → 图集（共享纹理数组，1024 层）**驱逐 churn 被拉满**；切工作区瞬间多个密集 pane 同帧全量 admit → 跨 pane 驱逐竞态暴露最大化 → 瞬时字形乱码（447aec8 的 pin 守卫对「切屏返回 pane 因驱逐计数不匹配回退全量渲染」基本 no-op，故未根治）。

→ 把 `requires_full_frame` 改回脏行快路径，**同时**消除每帧整屏 Clear（治闪烁）并大幅降低每帧字形 re-admit（降低乱码的 churn 暴露）。

## 方案：flag 门控的运行时快路径

不做「自动能力探测」（需 GPU readback、dev 无法验证、默认开启有回退风险），改为**运行时 flag 门控**，默认保持现状、可逆、零默认风险：

- `webgpu.rs`：进程级 `thread_local PRESENT_FAST: Cell<bool>`（默认 `false`）。`requires_full_frame` 改为：`if PRESENT_FAST { self.needs_initial_clear } else { true }`。`false` 时**逐字保持现有恒-true 行为**。
- `webgpu.rs`：`pub fn set_present_fast(on: bool)` 写该 thread_local。
- `lib.rs`：`#[wasm_bindgen(js_name = setPresentFast)] pub fn set_present_fast(on)` 转发（webgpu feature + wasm32 门控；非 webgpu 构建为 no-op 导出，保持 JS 调用面一致）。
- `manager.ts`：`ready()`/`init` 成功后读 `localStorage.RIDGE_PRESENT_FAST === '1'`，是则调 `setPresentFast(true)`（`typeof` 守卫旧 wasm 降级）。

### 验证（release，用户执行）

1. `pnpm tauri:build`（或 `tauri:build:debug`）出包，安装/运行（与发布版宿主分开）。
2. 默认（未设 flag）：行为与现状完全一致（回归基线）。
3. DevTools/console 设 `localStorage.RIDGE_PRESENT_FAST='1'; location.reload()`，复测：
   - 打中文 / TUI spinner / 选区：**不再到处闪**。
   - 切 CJK 密集多工作区：**瞬时乱码消失或显著减轻**。
   - 回归确认：空闲 + 滚动历史行 **不**回退「历史行随光标 500ms 闪烁」老 bug（若回退 → 说明该 release WebView2 的 `LoadOp::Load` 仍不可靠，关闭 flag 即恢复）。
4. 若 release 上 flag 开启无副作用 → 后续可把默认翻转或加「初始化一次性能力探测」自动判定。

### 退化 / 风险

- 默认 `false`：行为零变化，绝不可能回归。
- 开启后唯一风险 = 该 release WebView2 的 `LoadOp::Load` 不可靠 → 回退历史行闪烁老 bug；**关闭 flag 即恢复**，完全可逆。
- Canvas2D 后端不受影响（其 `requires_full_frame` 另有实现）。

### 乱码的残留说明

逐行核实图集守卫（`frame_written` 跨 pane、pin 缓存层、驱逐计数、generation）在静态分析上**自洽**，未能定位到确切的越界孔；cells 全有效证明是渲染侧。本方案通过**降低每帧 re-admit churn** 大幅压缩乱码暴露面；若 release 验证后仍偶现，再针对「切屏首屏的缓存重放 / 驱逐计数边界」做二次取证（需 release 环境）。
