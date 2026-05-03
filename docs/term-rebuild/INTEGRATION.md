# 接入指南 — Pane.svelte 替换 xterm.js

> **范围**：所有 round（1 → 7）完工后的最终接入形态。
> **不含**：分阶段过渡（实验开关、xterm 并存验证）。
> **前置**：阅读 `OVERVIEW.md` 了解整体架构。

> **状态（2026-05-03 末次复核）**：rounds 1 → 7 全部落地。本文档是当时的接入契约——具体 API 名最终被实现采用：`TerminalManager` (`src/lib/terminal/manager.ts`)、`feed/onData/resize/encodeKey/render`（wasm-bindgen 暴露）、`RidgePane.svelte` 替代了原来描述的"新 Pane.svelte"。`Pane.svelte` 整文件已删除。文档体保持 verbatim 用于设计契约延续。当前现行接口入口见 `packages/ridge-term/README.md` + CLAUDE.md「Frontend」段。

---

## ⚠️ 关于本文档的诚实声明

写这份文档时，**round 2.2 / 2.3 / 2.4 还没实现**。下面所有 `ridgeTerm.xxx()`、`createTerminalManager()` 等 API 是 **我承诺会做出的最终接口形状**，不是你今天能 `npm install` 用上的东西。

我会用 ⚠️ 标出每段当前不存在的代码。文档定的是契约——后续 round 我会照这个契约写实现，如果实现时发现某个签名不可行，我会**回头改这份文档**而不是悄悄改实现。

> **2026-05-03 复核更新**：rounds 1-7 全部完工。所有 ⚠️ 标记均已交付：`createTerminalManager()` 在 `manager.ts` 实现、`feed`/`onData`/`resize`/`encodeKey`/`render` 在 `lib.rs` 通过 wasm-bindgen 暴露、`Pane.svelte` 重写后又在 round 7 整体删除（被 `RidgePane.svelte` 取代，`SplitContainer` 直接 `import RidgePane from './RidgePane.svelte'`）。详见 `OVERVIEW.md` §3 + `TASKS.md` §0 + `packages/ridge-term/README.md`。

---

## 1. 接入后的最终架构（Pane.svelte 视角）

旧（xterm）：每个 Pane 组件持有一个完整的 Terminal 实例 + WebGL context + 一堆 addon。

新（ridge-term）：

```
┌─ App.svelte / +layout.svelte ─────────────────────────────────────────┐
│                                                                        │
│  <RidgeTerminalRoot />     ← 全局唯一，持有 wasm 模块 + 共享 canvas   │
│                                                                        │
│  <SplitPanes>                                                          │
│    <Pane paneId="A" workspaceId="..." />                              │
│    <Pane paneId="B" workspaceId="..." />                              │
│    <Pane paneId="C" workspaceId="..." />                              │
│  </SplitPanes>                                                         │
└────────────────────────────────────────────────────────────────────────┘
```

`<RidgeTerminalRoot>` 在 DOM 顶层（fixed positioning），它内部有：
- 1 个全屏 `<canvas>`（用于 WebGPU/Canvas2D 渲染）
- 1 个全局 wasm `TerminalManager` 实例
- 1 套全局键盘/鼠标事件路由

`<Pane>` 不再持有 Terminal 实例。Pane 只做三件事：
1. 提供一个**矩形容器** —— 渲染器会在这个容器对应的屏幕区域绘制该 paneId 的 grid
2. 把容器尺寸 + 位置上报给 manager（manager 用它算 scissor rectangle）
3. 路由 PTY 字节流到 manager

---

## 2. 全局组件：`<RidgeTerminalRoot>`

⚠️ **当前不存在，round 2.4 实现。**

### 用法

放在应用根布局里，整个应用生命周期只有一个：

```svelte
<!-- src/routes/+layout.svelte 或 App.svelte -->
<script lang="ts">
  import RidgeTerminalRoot from '$lib/terminal/RidgeTerminalRoot.svelte';
</script>

<RidgeTerminalRoot>
  <slot />
</RidgeTerminalRoot>
```

### 它内部做什么

```svelte
<!-- ⚠️ 这是设计稿，实现见 round 2.4 -->
<script lang="ts">
  import { onMount } from 'svelte';
  import init, { TerminalManager } from '@ridge/term-wasm';
  import { setManagerContext } from '$lib/terminal/managerContext';

  let canvas: HTMLCanvasElement;
  let manager: TerminalManager;

  onMount(async () => {
    await init();   // 加载 wasm 模块
    manager = new TerminalManager(canvas, {
      preferWebGpu: true,
      fallbackToCanvas2d: true,
    });
    setManagerContext(manager);   // 通过 svelte context 暴露给所有 Pane
    return () => manager.destroy();
  });
</script>

<canvas
  bind:this={canvas}
  class="rg-terminal-surface"
  style="position: fixed; inset: 0; z-index: 0; pointer-events: none;"
/>
<slot />

<style>
  /* canvas 在最底层，pane 容器盖在上面控制点击事件路由 */
  :global(.rg-terminal-surface) { display: block; }
</style>
```

要点：
- canvas 用 `position: fixed; inset: 0` 撑满视口，z-index 为 0
- `pointer-events: none` —— 点击事件由 Pane 容器接，再 dispatch 到 manager
- 设置 `setManagerContext` 后，每个 Pane 通过 `getManagerContext()` 取得同一个 manager 引用

---

## 3. 替换后的 Pane.svelte（最小骨架）

⚠️ **基于 round 2.4 完工后的 API。**

下面是最小骨架，**不含** IME、选择、搜索、滚动按钮——这些细节在第 6 节单独讲。

```svelte
<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { invoke, isTauri } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { getManagerContext } from '$lib/terminal/managerContext';
  import { activePaneId } from '$lib/stores/paneTree';

  interface Props {
    paneId: string;
    workspaceId: string;
  }
  let { paneId, workspaceId }: Props = $props();

  const manager = getManagerContext();

  /** 容器：决定 manager 在哪块屏幕区域绘制这个 pane 的 grid */
  let container: HTMLElement;

  let alive = true;
  let resizeObserver: ResizeObserver | undefined;
  let ptyUnlisten: (() => void) | undefined;
  let ptyClosedUnlisten: (() => void) | undefined;

  // Pane 在 manager 中的句柄。所有后续操作都通过 manager + paneId 调用。
  // manager 内部维护 paneId → 内核实例 + viewport rect 的映射。
  onMount(() => {
    if (!isTauri()) return;

    void (async () => {
      // 1) 先告诉 manager: 这个 paneId 要一个内核
      manager.attach(paneId, container);

      // 2) 创建 PTY 后端进程
      try {
        await invoke('create_pane', { paneId, shell: null });
      } catch (e) {
        console.error('create_pane failed', e);
        return;
      }

      // 3) 订阅 PTY 输出，转发到 manager.feed
      const outCh = `pty-output-${workspaceId}-${paneId}`;
      ptyUnlisten = await listen<{ data: string }>(outCh, (e) => {
        if (!alive) return;
        // ⚠️ 注意: data 是 string (Tauri JSON 序列化结果)，
        // manager.feed 接受 string OR Uint8Array
        manager.feed(paneId, e.payload.data);
      });

      // 4) 重放 scrollback
      try {
        const chunk = await invoke<{
          bytes: string;
          start_seq: number;
          at_oldest: boolean;
        }>('get_pane_scrollback_tail', { paneId, maxBytes: 256 * 1024 });
        if (alive && chunk.bytes) {
          manager.feed(paneId, chunk.bytes);
        }
      } catch {
        /* 后端老版本兜底 */
        try {
          const sb = await invoke<string>('get_pane_scrollback', { paneId });
          if (alive && sb) manager.feed(paneId, sb);
        } catch {
          /* 无 scrollback 历史 */
        }
      }

      // 5) 激活 PTY (现在 reader 才开始正常 emit 数据)
      try {
        await invoke('activate_pane_pty', {
          workspaceId,
          paneId,
          rows: manager.rows(paneId),
          cols: manager.cols(paneId),
        });
      } catch (e) {
        console.error('activate_pane_pty failed', e);
      }

      // 6) PTY 关闭事件
      ptyClosedUnlisten = await listen<{ workspaceId: string; paneId: string }>(
        'pane-pty-closed',
        (e) => {
          if (!alive) return;
          if (e.payload.workspaceId !== workspaceId || e.payload.paneId !== paneId) return;
          // 重建 PTY (你原来的 recoverPtySession 逻辑)
          void invoke('create_pane', { paneId, shell: null });
        }
      );

      // 7) 键盘 → PTY: manager 暴露统一回调
      manager.onData(paneId, (bytes: Uint8Array) => {
        if (!alive) return;
        // 走原来 write_to_pty 接口；data 用 binary string 还是 base64
        // 看你后端命令签名，目前是 string 接收
        const s = new TextDecoder().decode(bytes);
        void invoke('write_to_pty', { paneId, data: s }).catch((err) => {
          console.error('write_to_pty', err);
        });
      });

      // 8) 容器大小变化 → 通知 manager 重算 scissor + IPC resize_pane
      resizeObserver = new ResizeObserver(() => {
        if (!alive) return;
        manager.viewportChanged(paneId);
        // manager 内部会自己 debounce + 调用 resize_pane
      });
      resizeObserver.observe(container);

      // 9) 焦点
      const onFocus = () => activePaneId.set(paneId);
      container.addEventListener('pointerdown', onFocus);

      // unmount 时清理
      onDestroy(() => {
        alive = false;
        resizeObserver?.disconnect();
        ptyUnlisten?.();
        ptyClosedUnlisten?.();
        container.removeEventListener('pointerdown', onFocus);
        // detach 会从 manager 删除该 paneId 的内核 + 释放 GPU 资源
        manager.detach(paneId);
      });
    })();
  });

  // 焦点高亮
  $effect(() => {
    if (!container) return;
    container.dataset.rgPaneActive = String($activePaneId === paneId);
  });
</script>

<div
  bind:this={container}
  class="rg-pane-container h-full w-full min-h-0 min-w-0"
  data-rg-pane-id={paneId}
  data-rg-pane-active={false}
  role="application"
  aria-label="终端"
  tabindex="-1"
></div>

<style>
  /* 容器本身透明 — 真正的字符渲染由全局 canvas 完成
     容器只是占位 + 接收事件。manager 用这个 div 的 getBoundingClientRect()
     算屏幕坐标，告诉 GPU 在哪个 scissor rectangle 里画 paneId 对应的 grid */
  .rg-pane-container { background: var(--rg-term-bg); }
</style>
```

---

## 4. API 表面对照

### 旧 xterm API → 新 ridgeTerm API

| 旧 (xterm) | 新 (manager) | 备注 |
|---|---|---|
| `new Terminal({...})` | `manager.attach(paneId, el)` | 容器在哪、字号字体多少由 manager 全局配置 |
| `term.dispose()` | `manager.detach(paneId)` | |
| `term.write(data)` | `manager.feed(paneId, data)` | string 或 Uint8Array |
| `term.onData(cb)` | `manager.onData(paneId, cb)` | cb 收到 Uint8Array |
| `term.resize(c, r)` | `manager.viewportChanged(paneId)` | manager 自己根据容器大小算 |
| `term.cols / .rows` | `manager.cols(paneId) / rows(paneId)` | |
| `term.focus() / blur()` | `manager.setActive(paneId \| null)` | 全局只有一个 active pane |
| `term.options.fontSize = n` | `manager.setFontSize(n)` | **全局**，所有 pane 同步改 |
| `term.options.theme = {...}` | `manager.setTheme({...})` | **全局** |
| `term.getSelection()` | `manager.getSelection(paneId)` | 跨软换行已粘合，无需 stripSoftWraps |
| `term.selectAll()` | `manager.selectAll(paneId)` | |
| `term.clear()` | `manager.clear(paneId)` | 等价 ED 2 + 光标到 (0,0) |
| `term.paste(text)` | `manager.paste(paneId, text)` | 受 bracketed paste 模式影响 |
| `term.refresh(0, rows-1)` | — | manager 内部脏区追踪，不暴露 |
| `FitAddon.fit()` | — | manager 自动 fit（容器尺寸变化触发） |
| `Unicode11Addon.register(...)` | — | 内核内置 wcwidth |
| `WebLinksAddon` | `manager.onLinkClick(cb)` | OSC 8 + URL 启发式识别 |
| `SearchAddon.findNext(q, opts)` | `manager.search(paneId, q, opts)` | |
| `WebglAddon + clearTextureAtlas` | — | 全局 atlas，主题/字号变化时 manager 自动失效 |
| `term.attachCustomKeyEventHandler` | `manager.setKeyHandler(paneId, fn)` | 同样可拦截 |
| `windowsPty: { backend: 'conpty', ... }` | `manager.setWindowsConpty(true)` | 影响 reflow 协议 |

### Pane.svelte 当前的关键行为如何映射

**复制 / 粘贴**（旧代码 line 660-679）

```ts
// 新代码
manager.setKeyHandler(paneId, (ev) => {
  if (ev.type !== 'keydown') return true;
  const mod = ev.ctrlKey || ev.metaKey;

  // Ctrl+C: 有选区时复制，无选区透传 SIGINT
  if (mod && !ev.shiftKey && !ev.altKey && (ev.key === 'c' || ev.key === 'C')) {
    const sel = manager.getSelection(paneId);
    if (sel) {
      void writeText(sel);
      ev.preventDefault();
      return false;
    }
    return true;  // 让默认编码器送 \x03
  }

  // Ctrl+V
  if (mod && !ev.shiftKey && !ev.altKey && (ev.key === 'v' || ev.key === 'V')) {
    void readText().then((t) => { if (t) manager.paste(paneId, t); });
    ev.preventDefault();
    return false;
  }

  // ... 其他快捷键同理
  return true;
});
```

**主题切换**（旧代码 line 644-650 把 settingsStore 订阅起来）

```ts
unsubXtermTheme = settingsStore.subscribe((s) => {
  manager.setTheme(xtermThemeFor(s.theme));
  // 不需要 clearTextureAtlas + refresh —— manager 内部自动失效 atlas
});
```

**字号同步**（旧代码 line 633-637）

```ts
unsubFontSize = termFontSize.subscribe((size) => {
  manager.setFontSize(size);
  // 不需要 fit —— 字号变化是全局的，manager 一次重算所有 pane
});
```

---

## 5. PTY 数据流（端到端）

整体保持不变，只是消费端从 xterm 换成 manager：

```
shell (子进程)
   │ stdout
   ▼
Tauri pty-reader-{N} 线程   ──┐
   │ utf8 解码                │
   │ append_pty_scrollback    │  (这部分不动)
   │ event_tx.send             │
   ▼                            │
Tauri 全局事件循环            │
   │ COALESCE 4ms               │
   ▼                            │
app.emit("pty-output-{ws}-{p}")─┘
   │
   ▼ (IPC, JSON serialized)
Pane.svelte: listen(...)
   │
   ▼
manager.feed(paneId, data)
   │
   ▼
TerminalKernel.feed(bytes)
   │
   ▼
vte::Parser → grid mutations + dirty rows
   │
   ▼
(下一帧) renderer 读 grid → 绘制到 shared canvas
```

**注意**：你后端的 resize-silence 协议、scrollback 4MB block 存储、OSC 7 cwd 提取等，所有 Tauri 后端逻辑 **完全不变**。本接入工作只动前端。

---

## 6. 替换 xterm 各专项功能的方案

### 6.1 IME（中日韩输入法）

⚠️ **round 4 实现。**

旧方案：xterm 创建一个 `xterm-helper-textarea`，IME 候选窗口跟着这个 textarea 走。你做了一堆修复（pin textarea 位置 / compositionend 后清空 textarea 防 DEL）。

新方案设计：

- manager 维护一个全局隐藏 `<textarea class="rg-ime-helper">`
- 同一时刻只有 active pane 可见焦点
- 这个 textarea 的位置自动 pin 到 active pane 的左下角（不跟光标移动）—— **直接吸收你的 pin 经验**
- compositionstart：发出 `manager.imeStart(paneId)` 事件，渲染层不更新该 pane 的光标位置
- compositionend：取最终文本 → 调 `manager.feed(paneId, ...)`
- 与你原来 `helperTextarea.value = ''` 修复同等效果，但发生在 manager 内部

ridge 现有 IME pin 代码（`Pane.svelte:738-805`）在 manager 内部以更通用的方式重做。Pane.svelte 不再需要任何 IME 处理代码。

### 6.2 选择 / 复制

⚠️ **round 4 实现。**

旧方案：xterm cell-based 选择 + `stripSoftWraps()` 后处理。

新方案：
- manager 内部记录 (start: 字符索引, end: 字符索引)，索引基于"逻辑行" —— 软换行的连续 cell 共享一个逻辑行
- `getSelection(paneId)` 直接返回粘合后的字符串，**不需要 `stripSoftWraps`**
- 鼠标拖选：manager 接管 mousedown/mousemove/mouseup（在 RidgeTerminalRoot 层全局监听，按坐标命中 paneId）
- 双击选词、三击选行：内置

### 6.3 搜索

⚠️ **round 4 实现。**

旧方案：SearchAddon incremental 全 buffer 扫描。

新方案：
- `manager.search(paneId, query, { caseSensitive, regex })` 返回 `Iterator<MatchRange>`
- 实现：增量扫描 visible buffer + scrollback ring；后端 4MB block 那部分按需 IPC 拉取
- 你 Pane.svelte 现有的搜索 UI（`termSearchOpen` / `searchInputEl` 那段）UI 部分保留不变，只把 `searchAddon.findNext(...)` 改成 `manager.search(paneId, ...)`

### 6.4 OSC 标题 / cwd

⚠️ **round 5 实现，但你可以先保持现状（后端已经在 emit 这些事件）。**

你的后端 `pty.rs` 直接在 reader 线程里识别 OSC 0/1/2 标题和 OSC 7 cwd，然后 emit 单独事件。这套机制和 xterm 没关系，**接入新内核后不变**：

- `pane-title-changed-{ws}-{pane}` 继续监听
- `pane-cwd-changed-{ws}-{pane}` 继续监听
- 你的 `paneOscTitleStore` / `paneCwdStore` 完全不动

唯一区别：round 5 之后，wasm 内核也会自己解析 OSC（必要的，因为 OSC 8 超链接需要内核知道哪段字符是链接）。但 ridge 项目侧不依赖这条。

### 6.5 链接（Ctrl+Click 打开）

⚠️ **round 5 实现。**

旧方案：WebLinksAddon 用正则扫描可见行，cell 上挂 hover handler。

新方案：
- 内核解析 OSC 8（显式超链接）
- 启发式 URL 正则在 manager 层做（每帧扫脏行）
- `manager.onLinkClick(cb)` 一个全局事件，cb 收到 `{ paneId, uri }`
- ridge 侧把回调连到 Tauri opener 即可

### 6.6 滚动按钮 / 滚动到底

旧方案：你监听 `xterm-viewport` 元素的 scroll 事件 + `term.onScroll`，决定是否显示"滚动到底"按钮。

新方案：manager 暴露每个 pane 的滚动状态：

```ts
// ⚠️ round 2.4 API
const offset = manager.getScrollOffset(paneId);  // 0 = at bottom
manager.scrollToBottom(paneId);
manager.onScroll(paneId, (offset) => { /* update UI */ });
```

你 Pane.svelte 那个浮动按钮的 UI 保留，逻辑改成调 manager。

### 6.7 跨 split 保活（parking lot）

⚠️ **round 6 重新设计。**

旧方案：split 时 Pane 组件 destroy → recreate，xterm 实例存到 `terminalRegistry` 的隐藏 div 保 GL context。

新方案下问题不存在了 —— **manager 的内核与 Pane 组件解耦**。Split 时 Pane 组件 destroy/recreate，但 manager 里 paneId 对应的内核**根本没动**。新组件 mount 时 `manager.attach(paneId, container)` 拿同一个内核引用，自动恢复。

`terminalRegistry.ts` round 6 删掉。

---

## 7. 后端 API 兼容性矩阵

| 后端 IPC 命令 | 接入后是否还用 |
|---|---|
| `create_pane` | ✅ 保留 |
| `activate_pane_pty` | ✅ 保留 |
| `write_to_pty` | ✅ 保留 |
| `resize_pane` | ✅ 保留（manager 内部调） |
| `get_pane_scrollback_tail` | ✅ 保留 |
| `get_pane_scrollback_before` | ✅ 保留（深翻历史） |
| `get_pane_scrollback`（旧 shim） | ⚠️ 可删（manager 不调） |
| `get_pane_foreground_process` | ✅ 保留 |
| `get_pane_cwd` | ✅ 保留 |
| `pane-pty-closed` 事件 | ✅ 保留 |
| `pane-title-changed-*` 事件 | ✅ 保留 |
| `pane-cwd-changed-*` 事件 | ✅ 保留 |
| `pane-mode-changed-*` 事件 | ✅ 保留 |

**所有 Tauri 后端代码不需要改。**

---

## 8. 渐进 rollout（接入完成后还要做的事）

虽然这份文档说的是"最终态"，但实操推荐分两步切：

1. **第一次接入 (round 2.4)**：留一个 settings 开关 `experimentalRenderer: 'xterm' | 'ridge'`，默认 xterm。让你日常先开 1-2 个 pane 试 ridge。
2. **稳定后 (round 7)**：移除 xterm 依赖、删 terminalRegistry、删 IME pin 代码、从 package.json 删 `@xterm/*` 全套。

第一步的代码大概是：

```svelte
{#if useExperimentalRenderer}
  <RidgePane {paneId} {workspaceId} />
{:else}
  <XtermPane {paneId} {workspaceId} />
{/if}
```

把当前 Pane.svelte 重命名 `XtermPane.svelte`，新的 Pane.svelte 按本文档第 3 节写成 `RidgePane.svelte`。

---

## 9. wasm 包构建与发布

⚠️ **round 7 之前都用本地 path 依赖。**

```bash
# 在 ridge-term/ 目录
wasm-pack build --target web --out-dir pkg --release
# 体积优化 (可选)
wasm-opt -Oz -o pkg/ridge_term_bg.opt.wasm pkg/ridge_term_bg.wasm
mv pkg/ridge_term_bg.opt.wasm pkg/ridge_term_bg.wasm
```

ridge 项目侧 `package.json`：

```json
{
  "dependencies": {
    "@ridge/term-wasm": "file:../ridge-term/pkg"
  }
}
```

vite 配置需要 wasm + top-level await：

```ts
// vite.config.ts
export default defineConfig({
  optimizeDeps: { exclude: ['@ridge/term-wasm'] },
  build: { target: 'esnext' },
});
```

---

## 10. 测试与验收

接入完成后用下面这个清单验收，每条对应过去 ridge 用 xterm 时已经能跑的场景：

- [ ] 打开终端，能看到 prompt
- [ ] 输入命令 + 回车，能看到输出
- [ ] `vim foo` 进 alt screen，编辑保存退出，shell 内容完整恢复
- [ ] `less /path/to/log` 翻页正常，退出后不污染历史
- [ ] `htop` 显示正常，刷新不闪
- [ ] 中文 / emoji / CJK 输入输出宽度正确
- [ ] IME 输入中文不丢字
- [ ] Ctrl+C 中断、Ctrl+D 退出、Ctrl+L 清屏
- [ ] Ctrl+F 打开搜索、Enter 跳下一个
- [ ] 鼠标拖选、Ctrl+C 复制（包括跨软换行的 URL）
- [ ] Ctrl+Click 链接打开
- [ ] 拖动 splitpanes 边界，终端实时跟随、不黑屏
- [ ] split 一个 pane，原 pane 内容保持
- [ ] 打开 10 个 pane，跑 `cat /usr/share/dict/words` 同时不假死
- [ ] DevTools Memory 看 10 pane 总占用 < 50MB（旧方案 ~150MB）
- [ ] 主题切换实时生效
- [ ] 字号 +/- 实时生效，跨 pane 同步
