# 替换 xterm 与 Bug 修复整合计划

> 本文档回答：**"替换 xterm 完成后，痛点和 bug 是否自动消失？"**
>
> 答案：**部分消失，部分不消失**。
> 本文档把每个问题映射到具体修复路径，区分"靠替换自然解决"与"必须独立修复"。
>
> 阅读顺序：先读 `OVERVIEW.md` 了解替换工作整体，再读本文档。`BUGFIX.md` 仍然有效（独立 patch），本文档是它的扩展，把 BUGFIX 的内容并入替换时间线。

---

## 1. 一图看完：每个问题的归属

```
┌──────────────────────────────────────────────────────────────────────────┐
│  问题分类决策树                                                            │
│                                                                          │
│  问题源于：                                                                │
│   ├─ Tauri 后端 (pty.rs / lib.rs / state.rs)                              │
│   │    └─► 替换工作不动后端 → 必须独立修 [BUGFIX]                         │
│   │                                                                      │
│   ├─ Pane.svelte 自己写的逻辑 (ResizeObserver / setInterval / listen)     │
│   │    ├─ 接入后我会重写 Pane.svelte                                      │
│   │    │   └─► 新 Pane.svelte 不会引入这些 bug                            │
│   │    └─ 但接入前过渡期老 Pane.svelte 还在跑                             │
│   │        └─► 推荐先打 [BUGFIX] patch 改善过渡期体验                     │
│   │                                                                      │
│   └─ xterm 内核 / 渲染器架构本身                                          │
│        └─► 替换工作直接消除 [REPLACE]                                      │
└──────────────────────────────────────────────────────────────────────────┘
```

---

## 2. 完整问题对照表

| ID | 问题 | 根因层级 | 修复路径 | 时机 |
|---|---|---|---|---|
| 痛点 1 | 输入响应慢（25-30ms） | 多层叠加 | BUG-4 patch + 后续 round 优化 | 立即 + 长期 |
| 痛点 2 | 渲染抖动 / resize 抽搐 | xterm + Pane.svelte | round 2.4 重写 + BUG-5 patch | 立即 + round 2.4 |
| 痛点 3 | 多 pane（10+）UI 假死 | xterm 架构 | round 2.4 共享 surface | round 2.4 |
| 痛点 4 | 内存膨胀（每 xterm ~5MB） | xterm + 双 scrollback | round 2.4 + BUG-6 patch | 立即 + round 2.4 |
| 痛点 5 | 选择/复制/搜索差 | xterm cell-based | round 4 重新设计 | round 4 |
| BUG-1 | 双 listener git diff 风暴 | Pane.svelte 逻辑 | BUG-1 patch | 立即 |
| BUG-2 | 轮询无 backoff | Pane.svelte 逻辑 | BUG-2 patch | 立即 |
| BUG-3 | reader 阻塞 send | 后端 pty.rs | BUG-3 patch | 立即 |
| BUG-4 | 固定 4ms 合批 | 后端 lib.rs | BUG-4 patch | 立即 |
| BUG-5 | 连环 rAF + clearTextureAtlas | Pane.svelte 逻辑 | BUG-5 patch | 立即 |
| BUG-6 | 双 scrollback 重复存储 | xterm 配置 | BUG-6 patch | 立即 |

**"立即"= 不依赖替换工作，可以现在 cherry-pick。**
**"round X"= 必须等到对应替换里程碑完工。**

---

## 3. 痛点逐条分析：替换后是否自动好

### 痛点 1：输入响应慢（按键到屏幕回显延迟 25-30ms）

**端到端延迟分解**：

```
键按下 (t=0)
  → IPC write_to_pty                  ~5ms       ← 后端 IPC 链路
  → 内核 echo                          ~1ms
  → PTY reader 唤醒 + read              ~1ms
  → utf8 解码                          ~0ms
  → event_tx.send (block_on!)         ~0-3ms     ← BUG-3 高负载下卡死
  → coalesce 4ms 窗口                  4ms        ← BUG-4 单字符纯延迟
  → app.emit (JSON 序列化)             ~2ms
  → webview IPC 接收 + listener        ~3ms
  → term.write → vt parser + render    ~1ms
  → 等下一帧                           0-16ms     ← rAF 等待
```

**替换后哪些消失？**

- "term.write → render" 这一段：换成 wasm 内核后理论上更快（vte crate 比 xterm 的 JS parser 略快），但收益 < 1ms，几乎不可见。
- "等下一帧" 这一段：替换后渲染时序自己控制，**可能**改善（新内核可以决定立即 swap 而不是等 rAF），但仍受限于浏览器 vsync。
- 其它部分**全部和替换无关**——它们是 IPC 和后端的事情。

**结论**：替换 xterm 对这条链路 **影响 < 5ms**。真正改善输入响应必须做：
1. BUG-3（后端不阻塞）—— 立即修，独立的
2. BUG-4（自适应合批）—— 立即修，独立的
3. 长期：考虑 SharedArrayBuffer 旁路 IPC（架构级改造，目前不在路线里）

✅ **行动建议**：BUG-3 和 BUG-4 先做。替换工作完成后，**这条还是慢**，不要期望它自动好。

---

### 痛点 2：渲染抖动 / resize 抽搐

两个独立子问题：

**子问题 A — `clearTextureAtlas` 每帧清掉缓存（BUG-5）**

xterm 特定的 GPU 资源管理 bug。新内核没有 `clearTextureAtlas` 这个 API，**自然消失**。

**子问题 B — Pane.svelte 的连环 rAF（BUG-5 同号 patch 的另一段）**

```ts
resizeObserver = new ResizeObserver(() => {
    requestAnimationFrame(() => {
        fitAddon?.fit();
        // ... outer rAF
    });
    setTimeout(() => {
        void fitAndSyncPty();   // 内部又一次 rAF
    }, 200);
});
```

这是 Pane.svelte 自己的代码。**round 2.4 我会重写 Pane.svelte，不会引入这个 pattern**——manager 内部统一管 fit 节流。

**结论**：

- ⏳ round 2.4 之前：你的过渡期 Pane.svelte 仍然连环 rAF。建议先打 BUG-5 patch（删掉每帧 clearTextureAtlas）。
- ✅ round 2.4 之后：彻底解决。

---

### 痛点 3：多 pane（10+）UI 假死

**根因**：每 pane 一个 WebGL context（Chromium 16 个上限）+ 各自字形 atlas + 完整 VT 解析器。

**替换后**：round 2.4 的共享 surface 架构直接砍掉这条——1 个 GPU context 给所有 pane，1 份 atlas 全局共享。

✅ **完全靠替换解决**，独立 patch 帮不上。

⚠️ 但有个隐患：在 round 2.4 完工前，你的现状没有缓解手段。如果你日常就要开 10+ pane，建议把 round 2.4 当头号优先做。

---

### 痛点 4：内存膨胀

旧测算：10 pane = 50MB 前端 + 40MB 后端 ≈ 90MB

```
每 pane 旧方案分布：
  xterm Terminal buffer        1.2 MB
  WebGL atlas                  1-2 MB    ← round 2.4 共享后只 1 份
  WebGL VBO/UBO                ~200 KB   ← 同上
  Addon 实例                   ~500 KB   ← 替换后消失
  Svelte 反应式订阅            ~100 KB
  后端 scrollback              4 MB      ← BUG-6 也无法降低后端，只降前端
  前端 scrollback (2000 行)    ~1.2 MB   ← BUG-6 patch 降到 500 行 ~300 KB
```

**替换后**：

- atlas / VBO 全局共享：减 ~9MB（10 pane 时）
- xterm Terminal buffer 替换为 wasm grid：每 pane ~600KB（更紧凑），减 ~6MB
- Addon 实例消失：减 ~5MB
- 后端 4MB scrollback：**不变**（后端代码不动）
- 前端 buffer：被新 wasm 内核接管，需要在 manager 配置里指定保留多少行

**净效果（10 pane）**：90MB → ~50MB（替换工作）+ BUG-6 patch 再降 ~10MB → ~40MB

⚠️ 但要注意：wasm 内核如果不主动控制内存就会有自己的膨胀——比如 grid 缓冲设太大、scrollback 设太多。**round 2.4 的 manager 默认参数我会按"每 pane <500KB 内存"来调**。

✅ **替换主要解决，BUG-6 patch 是补充**。

---

### 痛点 5：选择 / 复制 / 搜索差

xterm cell-based 选择跨软换行断裂，所以 ridge 写了 `stripSoftWraps()` 后处理。这本身是 workaround，根因在 xterm。

**替换后**：round 4 我设计的选择基于"逻辑行"——软换行连续 cell 是一个逻辑行。`getSelection(paneId)` 直接返回粘合后的字符串，不需要 stripSoftWraps。

✅ **完全靠替换 + round 4 解决**。

---

## 4. 时间线视图：什么时候你能感受到什么改善

```
现在  ─┬─ 当前状态：所有痛点都在
       │
       │ 第 1 步（建议立即做，1-2 天）
       ▼
       ├─ apply BUG-1 patch  → git diff 风暴消失
       ├─ apply BUG-3 patch  → 大输出时输入不再卡
       ├─ apply BUG-4 patch  → 慢速输入 echo 延迟降 4ms
       ├─ apply BUG-5 patch  → resize 抖动减半
       │
       │ 第 2 步（同步进行，2-3 天，可选）
       │
       ├─ apply BUG-2 patch  → idle pane IPC 减 75%
       ├─ apply BUG-6 patch  → 内存降 30MB（10 pane 时）
       │
       │ 第 3 步（替换工作 round 2.2 + 2.3，~2 周）
       │
       ├─ Canvas2D 渲染器跑通
       ├─ JS 表面 API 完工
       │ （此时 wasm 包能渲染基本字符，但还没接入 Pane.svelte）
       │
       │ 第 4 步（替换工作 round 2.4，~1 周）
       │
       ├─ TerminalManager + 共享 canvas
       ├─ Pane.svelte 重写为新版本
       │   └─ 痛点 2 (resize 抽搐) 彻底解决
       │   └─ 痛点 3 (10+ pane 假死) 彻底解决
       │   └─ 痛点 4 (内存) 大部分解决
       │
       │ 第 5 步（round 3，~2 周）
       │
       ├─ WebGPU 渲染器替换 Canvas2D
       │   └─ 渲染性能从"和 xterm 持平"提升到"明显优于"
       │
       │ 第 6 步（round 4，~2 周）
       │
       ├─ IME / 选择 / 搜索 / 链接
       │   └─ 痛点 5 (选择/搜索差) 彻底解决
       │
       │ 第 7 步（round 5-7，~2 周）
       │
       └─ OSC 集成 + parking lot 重构 + 删 xterm 依赖
```

**总时长估算**：8-10 周（仅替换工作）+ 1 周（独立 bug patch）。
**我的不确定度**：±30%。沙箱限制 + IME 等专项的边角踩坑可能拉长。

---

## 5. 有意推迟的取舍

下列改善我刻意**没**放进路线里，简短说明取舍：

### 痛点 1 的根本治理（< 5ms 端到端）

**没做**。需要 SharedArrayBuffer 替换 Tauri IPC，工作量约等于整个替换计划的一半。当前的 BUG-4 patch（自适应合批）能让单字符延迟从 ~25ms 降到 ~21ms，**降不到 < 10ms**。如果你强需求超低延迟（< 10ms），告诉我，我们再讨论是否启动这条改造。

### 后端 4MB scrollback 降低

**没做**。后端 block 存储设计是允许 ridge 用户翻很久的历史，4MB 是有意为之。如果降到 1MB 会影响这个功能。BUG-6 patch 只降前端 buffer，不动后端。

### 字号 / 主题切换延迟优化

**没做**。当前主题切换会触发 atlas 重建（~50-100ms）。新内核 round 3 的 atlas 设计可以做到主题切换零重建（fg/bg 是 fragment shader 参数而不是烤进 bitmap），但这是 nice-to-have 不是问题。

---

## 6. 推荐执行计划

### 给你的具体行动

**本周（先打独立 bug patch）：**
1. apply BUG-3 patch（后端，无副作用）
2. apply BUG-1 patch（git diff 风暴，最痛快的修复）
3. apply BUG-5 patch（删一行，立竿见影）

**下周：**
4. apply BUG-2 patch（自适应轮询，需要回归测试）
5. apply BUG-4 patch（自适应合批，需要回归测试）
6. （可选）apply BUG-6 patch（前端 scrollback 降到 500，团队对齐后再做）

**与此同时**，替换工作继续 round-by-round 推进。每打完 round，你可以看到对应痛点改善：

- round 2.4 完工 → 多 pane 假死 + resize 抖动 + 内存大头解决
- round 3 完工 → 渲染性能提升
- round 4 完工 → 选择/搜索/IME 解决
- round 5-7 → 收尾

### 不推荐的做法

❌ **不要**只做替换不打 patch，期望"替换完都好了"——后端 bug、Pane.svelte 自己的 bug 不会自动好。
❌ **不要**只打 patch 不做替换——痛点 3 / 5 没法靠 patch 修。
✅ **两条路并行**——独立 bug 立即打 patch，替换工作按 round 推进。

---

## 7. 一句话总结

> **替换 xterm 解决的是 xterm 架构限制造成的问题**（多 pane 假死、cell-based 选择限制、atlas 失效抖动）。
> **独立 bug patch 解决的是后端/前端 svelte 自己写的逻辑问题**（git diff 风暴、reader 阻塞、轮询风暴、合批延迟）。
> 这两类**没有覆盖关系**——必须并行做。

如果你只能选一条，**先打 patch**（1-2 周内见效），同时让我把替换继续往下做。
