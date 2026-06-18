# 交接:首行选不中 + 选区持续闪烁(缺陷①)—— 待运行时取证后修复

> 日期:2026-06-18 ｜ 状态:**已定位到根因层,未修复(刻意)**
> 同批 ②③④ 已修复并入 develop(见 [2026-06-18-render-fixes-batch-design.md](2026-06-18-render-fixes-batch-design.md))。
> ① 因唯一候选修复需动「为正确性刻意设的」WebGPU 全帧机制、且无运行时无法验证(可能回退),经与用户确认**暂不盲改**,先交接 + 留可见 TODO(TASKS.md §1.36)。

## 用户报告的现象
1. Shell 模式下**首行(活动 prompt 行)无法被鼠标选中**。
2. **选区一旦出现就持续闪烁**,"好像在反复渲染整帧"。
3. 复现场景:用户正在 prompt 行输入路径(`C:\wo|kcode\supabase`),即活动输入行在视口顶部时最明显。

## 已排除的假设(静态证据,勿重复走)
- **选区被门控清除 —— 证伪**:`packages/ridge-term/src/selection.rs` 无任何门控逻辑(grep `gate/history/live/committed` 零匹配);`src/lib/terminal/tuiGate.ts` 管的是**键盘输入路由**(给 TUI 还是 shell),不碰选区。
- **命中测试 floor 越界 —— 证伪**:`manager.ts::computeCell`(:1238)与 `cellFromEvent`(:2911)对 row 用 `Math.max(0, Math.min(rows-1, floor(y/cellH)))`,即使点到顶部上沿(y<0)也钳到 row 0。单分屏整数缩放下命中原点(容器 rect+pad)与绘制原点(host scissor)一致(见 ④ 调查)。
- **RAF 循环无故 60fps 自旋 —— 证伪**:`manager.ts` RAF 循环(:4461–4909)已有 `nextBlinkDeadlineMs` 休眠(:4843–4905):无渲染时 sleep 到下一个 blink 边界。仅在「有 pane 实际渲染」时才保持 60fps RAF 节律(:4872–4875)。

## 收敛的根因(两症状同源)
活动 prompt 行被 PSReadLine 每次按键/预测**高频重画** → 每帧 `Renderer::is_dirty`(`renderer.rs:591`)返回 true → 桌面默认 WebGPU 后端 `WebGpuPaneBackend::requires_full_frame()` **恒返回 true**(`packages/ridge-term/src/render/webgpu.rs:339-365`)→ `renderer.rs:387` 置 `full_redraw_pending` → **每帧整屏 `LoadOp::Clear` + 全量重绘** → 在 WebView2 上表现为闪烁。
- 活动输入行就在视口顶部(首行),所以"首行"闪得最凶,且选它时与实时重画/可能的 scrollback 滚动相争 → 主观"首行选不中"。
- `requires_full_frame=true` 是**刻意**的:webgpu.rs:340-365 注释记载,WebView2 148.0.3967.70 的 `LoadOp::Load` 会丢交换链像素,当初正是为消除"历史行每 500ms 随光标闪烁"才强制全帧。注释明确预留:"若未来 WebView2 让 LoadOp::Load 可靠,可在运行时能力探测后恢复脏行快路径"。

## 为何暂不修(风险)
唯一真正消除闪烁的办法是让 WebGPU 不再每帧全清屏(恢复 Canvas2D 那样的脏行快路径)。但:
- **环境相关且相反**:webgpu.rs 注释指出 dev 的 WebView2 148 上 LoadOp::Load 不可靠(会闪),而 e2e-shell **release exe** 上可靠。盲目 default-on 很可能在 dev 上**回退到原本的"历史行闪烁"老 bug**。
- **无运行时无法验证**:常驻 dev 未开 CDP,无法测当前 WebView2 版本上 LoadOp::Load 是否可靠、也无法测真实 RAF 频率。

## 建议的修复路径(需运行时)
1. 用 `pnpm tauri:dev:cdp` 起 CDP 版 dev,连 DevTools。
2. **取证**:
   - 在 prompt 行输入时,测 RAF tick 实际频率(确认是否 60fps 持续全帧;`localStorage.RIDGE_TICK_TRACE='1'` 可看 per-frame trace,见 manager.ts:4766)。
   - 写一次性探针:用 `LoadOp::Load` 画一帧后读回若干像素,判断本机 WebView2 版本是否保留了上帧内容。
3. **实现(能力探测版)**:把 `webgpu.rs::requires_full_frame` 从恒 `true` 改为读一个**初始化时一次性探测**得到的能力位——`LoadOp::Load` 可靠 → 返回 `self.needs_initial_clear`(脏行快路径,选区/光标行不再每帧整屏重画 → 消除闪烁);不可靠 → 保持 `true`(现状,正确)。这正是 webgpu.rs:361-364 注释预留的方案。
4. **首行命中**:取证确认首行"选不中"是否纯粹由上面的实时重画/滚动相争导致(预计是)。若是,闪烁修好后首行选中自然恢复;若仍有偏差,再查 host scissor Y 原点。

## 关键文件索引
- `packages/ridge-term/src/render/webgpu.rs:339-365`(`requires_full_frame` 恒 true + 预留探测方案 + 本 TODO 锚点)
- `packages/ridge-term/src/render/renderer.rs:387`(WebGPU 强制全帧)、`:591-647`(`is_dirty` 驱动 RAF 唤醒)
- `src/lib/terminal/manager.ts:4461-4909`(RAF 循环 / blink 休眠 / host 渲染门控)、`:1238`/`:2911`(命中测试)
- `packages/ridge-term/src/selection.rs`(无门控,排除项)、`src/lib/terminal/tuiGate.ts`(输入路由,排除项)

## 跟进(2026-06-18,第二轮:取证尝试 + 根因深化)

> 触发:用户要求清扫 TODO 并用 `dev:cdp` + chrome-devtools 取证。结论:**取证被环境阻断,根因分析已深化并纠正本文早先的简化框架,决定继续保持现状(零回归),不盲改渲染核心。**

### A. 纠正:并非"每帧整屏 LoadOp::Clear"
P1.1(2026-05-19)已把 `surface_host.rs::begin_frame` 的整屏 `LoadOp::Clear` 改成**仅 `needs_initial_clear` 为真时**才发(只由 `resize()`/`invalidate()`/surface-lost 置位),其余帧 `record_pane` 一律 `LoadOp::Load`。因此 `requires_full_frame()=true` 的真实代价是**每帧把所有可见行重新编码**(全标脏),**不是每帧清屏**。本文 §收敛的根因 中"每帧整屏 LoadOp::Clear"一句应据此修正。

### B. 进一步证伪
- **选区不会每帧误判变化**:`renderer.rs::selection_eq`(:743) 是 normalized **值比较**;稳定选区 → `sel_changed=false` → 不会每帧 `on_full_invalidate`。故"选区存在→每帧整屏清"不成立。
- **JS 侧已补偿 Load 不可靠**:`manager.ts:4740-4755` 注释明确——任一 pane 脏即所有可见 pane 重录(脏走 `render()`、其余 `recordCachedOnly()`),正是为补偿 WebView2 "邻 pane scissor 区 present 后呈 fresh-zero"。
- 推论:残留闪烁最可能是 **WebView2 148 交换链 present 语义在 60fps 全帧 churn 下的呈现**(取证级现象),而非可静态定位的逻辑 bug。

### C. 取证被环境阻断(关键)
`pnpm tauri:dev:cdp` 成功构建并启动 dev 应用(正常建 pane、远控可开),但 **WebView2 148 的远程调试端口未初始化**:
- 9222 无 listener;
- 进程命令行确带 `--remote-debugging-port=9222`,但 user-data-dir(`.webview2-dev-cdp/EBWebView`)下**不生成 `DevToolsActivePort`**;
- 无 Edge `RemoteDebuggingAllowed` 策略封禁。

即:**当前 WebView2 版本上整套 CDP forensics 工作流不可用**——这正解释了第一轮"无运行时无法验证"的根本原因。已给 `scripts/tauri-dev-cdp.mjs` 补 `--remote-allow-origins=*`(chrome-devtools-mcp 在 Chromium 111+ 连接所必需),但它治的是 WS 握手 403,**不解决 148 上端口根本不初始化**的问题。

### D. 决定
保持 `requires_full_frame()=true`(刻意、正确、零回归)。能力探测版的真实成本被低估:需把 surface usage 加 `COPY_SRC` + 初始化期**异步 GPU 回读**判断 `LoadOp::Load` 是否保留——侵入渲染核心且脆弱;且即便实现,dev(Load 不可靠)上会**正确回退到现状**,无法取得正向验证,真正收益只在 release exe。**修复仍 gated 在**:① 一个 CDP 可用的 WebView2 版本(以取证) 或 ② 直接在 release exe 上做视觉验证。在此之前不动。
