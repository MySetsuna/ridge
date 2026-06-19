# 渲染缺陷批量修复设计 (2026-06-18)

四个独立的终端渲染缺陷,根因均已在代码中核验。每个缺陷单独 commit。

## 最终状态(2026-06-18)
- **③ TUI 重进残留** —— ✅ 已修并入 develop(`9ec8ae4`)。`cargo test -p ridge-term` 364 绿。
- **② 反复 resize 错位** —— ✅ 已修并入 develop(`8d2124e`)。回流重写为幂等/纳入 scrollback/宽字符成对;inline-TUI 测试全保绿;多行 PSReadLine prompt 边角延后(见 ② 节)。
- **④ IME 顶偏 + 定位不准** —— ✅ 已修并入 develop(`8b07992`)。顶偏根治 + scissor 同源精确对齐;`imeAnchor` 9/9 绿。**定位精度待运行时核验**(100/125/150% 缩放 + 多分屏)。
- **① 首行选不中 + 选区闪烁** —— ⏳ **未修(刻意)**。唯一候选修复需动 WebGPU「恒全帧」正确性机制、无运行时无法验证且可能回退,经用户确认先交接 + 留 TODO。交接文档 [2026-06-18-selection-flash-firstline-handoff.md](2026-06-18-selection-flash-firstline-handoff.md);追踪 TASKS.md §1.36。

架构前提:Rust 原生终端引擎 `packages/ridge-term`(grid/parser/renderer)+ 后端 `src-tauri/src/engine/parser.rs`(`PaneParser` 跑真 vte 并 diff 出 `GridDelta`)+ 前端镜像 `terminal.rs::apply_delta` 重放 + Svelte 面板 `RidgePane.svelte`(桌面)/`TerminalCanvas.svelte`(remote)。桌面默认 WebGPU host 单画布,`requires_full_frame()` 恒 true。

---

## ③ TUI 退出再进残留(最干净,先做)

**根因**:`src-tauri/src/engine/parser.rs::diff_into_frame` 的 `ScreenSwitch` 分支(`:321-327`)只 push delta、**不重置 cell-diff 基线 `self.snapshot`**;而 resize(`:273`)与 RIS(`:310`)都重置了。alt 重进时只发增量 Cells;前端镜像 `apply_delta(ScreenSwitch→alt)` 走 `enter_alt_screen(false)` 不清屏(`terminal.rs:263`),`leave_alt_screen` 也只翻标志(`grid.rs:716-722`)→ 镜像 alt 网格残留上次内容。resize 能修复正是因为它重置了基线强制全量重发。

**修复(两处都要,已推演确认缺一不可)**:
1. 后端 `diff_into_frame` 的 `ScreenSwitch` 分支检测到切屏时,把 `self.snapshot` 重置为全 blank(复用 resize/RIS 机制)。仅此一处时,新会话留白处仍残留。
2. 前端镜像 `terminal.rs::apply_delta` 进入 alt 改为 `enter_alt_screen(true)` 清屏。仅此一处时,与旧 primary 同字符的格子会因后端不发而丢内容。
- 两者合并:镜像进入即清空 + 后端按 blank 基线全量重发非空格 → 无残留、无丢失。

**测试**:`src-tauri` 侧对 `PaneParser` 加单测:feed `?1049h`+内容 → `?1049l` → 再 `?1049h`,断言第二次进入帧包含覆盖首次残留区域的 Cells(基线被重置)。前端 `terminal.rs` 加单测:`apply_delta(ScreenSwitch{is_alt:true})` 后镜像 alt 网格为空。

---

## ④ IME 定位不准 + 最左侧顶偏

**根因 A(顶偏)**:`RidgePane.svelte:710-713` composition 期把隐藏 textarea 撑宽成 `(charCount+1)*cellW`;它在 col 0 时 `left:0` 贴左边界且被聚焦 → 浏览器对 `overflow:hidden` 祖先隐式设 `scrollLeft` → 整区左移。remote 端 hidden-input 固定 `width:1px` 故无。

**根因 B(不准)**:桌面 textarea 用容器坐标系定位(`repositionImeHelper:627`,`pos = manager.inputAnchorResolved`),而内容画在**共享全局 host canvas** 的 scissor 偏移处;两坐标系靠多次相对运算 + 不一致 DPR 取整对齐,非整数缩放(125/150%)或多分屏下偏。remote 每分区独立 canvas、同原点故准。

**修复**:
1. (根因 A,确定做)删掉 `onCompositionUpdate` 的动态 `width` 增宽;CSS `.rg-ime-helper` 固定窄宽(对齐 remote 的 `width:1px` 思路)。直接根治顶偏。
2. (根因 B)先做**精确对齐**:让 textarea 的 `left/top` 与 scissor 同源——`manager.ts` 新增/改 anchor resolver,host 模式下返回与 `_recomputeViewport` 的 `xDev/yDev` 同源(除以 DPR)的坐标,并统一 `floor(cssX*dpr)/dpr` 取整。
3. (兜底)若非整数缩放下仍飘,退**左下角**:`repositionImeHelper` 固定 `left:2px; bottom:2px; top:auto`,关掉每帧跟随。用户已认可此兜底。

**测试**:逻辑能下沉的(坐标换算 helper)加 TS 单测;定位精度本身需运行时(DevTools,100/125/150% 缩放)验证。

---

## ② 反复 resize 内容错位(最难,TDD)

**根因**:`grid.rs:836-844` shell 模式(`!inline_tui`)用 `reflow_boundary=cursor.row` 启用 `reflow_primary_screen_at`(`:1112-1286`),该实现有损非幂等:① 完全不碰 `self.scrollback`(跨 scrollback↔可见区的段落错位);② `wrapped` 末行无条件 `r_idx<pn-1` 重建 + 整段尾部空白裁剪(段落分组逐次漂移);③ 溢出段落从顶部直接丢弃、不回灌 scrollback(内容丢失);④ 宽字符/簇/超链接丢失。误差跨多次 resize 累积。

**决策(用户拍板)**:保留并**修正** reflow,而非禁用——shell 与 inline-TUI 共用此实现(唯一调用点 `:843`,仅 `boundary` 不同),修正实现对两条路径都是净改善,不会让 inline-TUI 退化(§1.25 担心的 shell-redraw race 已被 `boundary=cursor.row` 规避,残留多行 prompt 边角用"纳入活区不回流"兜)。

**修复方向**(逐项,TDD 驱动):
1. **幂等性**:`resize A→B→A` 必须还原。核心是保留"硬换行 vs 软换行"信息——不要无条件 `r_idx<pn-1`,并避免有损的整段尾部空白裁剪破坏列对齐。
2. **scrollback 纳入**:回流把 scrollback 末尾相关行与可见区作为一个文档重排;溢出的最旧行回灌 scrollback 而非丢弃。
3. **宽字符成对**:re-split 时不在行尾切断 width==2 单元,推到下一行首并补 blank。
4. 多行 PSReadLine:逻辑 prompt 跨 cursor.row 以上时,把整段编辑区纳入"活区"不回流(类比 inline-TUI 的 frame_top)。

**测试(先写失败用例)**:
- `reflow_idempotent_round_trip`:宽→窄→宽还原。
- `reflow_spans_scrollback`:跨 scrollback 边界的段落正确重排、不错行。
- `reflow_overflow_goes_to_scrollback`:缩窄溢出行进 scrollback 可回看。
- `reflow_preserves_wide_char`:CJK 宽字符不被拆行。
- **保持绿**:`inline_tui_resize_reflows_history_above_frame`(`:2889`)等既有 inline-TUI 测试。
- 改完用 Claude Code 运行时验证 inline-TUI 不退化。

---

## ① 首行选不中 + 选区持续闪烁(需运行时确认)

**已确认(闪烁,架构级)**:WebGPU `requires_full_frame()` 恒 true(`webgpu.rs:365`);`is_dirty`(`renderer.rs:591-647`)在光标 blink 时每 500ms 相位边界判脏 → 唤醒 RAF → WebGPU 整屏 clear 重绘;shell 持续吐字时每帧判脏 → 60fps 全清屏 + WebView2 交换链可见闪烁。

**待运行时确认**:
- 闪烁的确切高频驱动源(是 blink 2Hz 还是 shell 内容每帧判脏 60Hz)——DevTools 看 RAF tick 频率。
- 首行选不中:纯坐标数学能到 row 0,疑似 host 单画布命中基准(容器 rect)与画布真实绘制原点(scissor)不同源,或首行被 shell 自滚出视口。需 DevTools 实测 `cellFromEvent` 输出。

**候选修复**(待确认后定):
- 给 WebGPU 后端加 `LoadOp::Load` 运行时能力探测(`webgpu.rs:361-364` 已预留),可靠环境恢复脏行快路径,选区/光标行不必每 blink 整屏重画。
- 命中测试基准改用 pane 真实绘制矩形(复用 `_recomputeViewport` 的 viewport 偏移)。

此项放最后,先用 DevTools 取证再下刀,不凭静态猜测改。
