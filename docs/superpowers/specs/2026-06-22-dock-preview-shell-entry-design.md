# 停靠提示加强 + 切 shell 入口设计 (2026-06-22)

对 2026-06-22 UI 批次（拖拽停靠改指针事件 + per-pane 切 shell）的两点迭代细化。方向已与用户确认。每点单独 commit。

架构前提：指针拖拽停靠由 `src/lib/actions/paneDockDrag.ts` + 纯助手 `paneDockResolve.ts` + store `paneDockHover`/`dragHoverWorkspaceId` 驱动；停靠覆盖层在 `SplitContainer.svelte` 叶级渲染。per-pane 切 shell 组件 `PaneShellSwitcher.svelte` 在 pane header；pane 右键菜单在 `RidgePane.svelte`（`showContextMenu`，`ContextMenuItem` 支持 `children` 子菜单 + `icon`）。

---

## A. 拖拽停靠提示 → 方向半区预览填充

**问题**：当前叶级覆盖层提示 = `bg-black/25` 整面遮罩 + `dockHintClass` 的 5px 强调色内嵌边条（center 为 2px ring）。太弱，且不显示新 pane 的落点方向。运行时指针拖拽本身工作正常（用户能看到弱提示），只是提示需加强。

**方案**：`SplitContainer.svelte` 叶级覆盖层重构为方向半区预览：
- 外层 `absolute inset-0 z-30 ... pointer-events-none` 遮罩减淡为 `bg-black/15`（标明此 pane 是拖拽目标）。
- 内层：仅当 `hover`（= `$paneDockHover.paneId === node.id` 时的 region）非空，渲染一个定位预览块：
  `class="absolute bg-[var(--rg-accent)]/25 border-2 border-[var(--rg-accent)] rounded transition-all duration-100 {dockRegionClass(hover)}"`
- 新函数 `dockRegionClass(region)` 取代旧 `dockHintClass`（旧的删除）：
  - `left` → `inset-y-0 left-0 w-1/2`
  - `right` → `inset-y-0 right-0 w-1/2`
  - `top` → `inset-x-0 top-0 h-1/2`
  - `bottom` → `inset-x-0 bottom-0 h-1/2`
  - `center` → `inset-[20%]`（居中合并/覆盖框）
- `transition-all duration-100` 使指针在区域间移动时预览块平滑切换。

**影响文件**：`src/lib/components/SplitContainer.svelte`（叶级覆盖层块 + 替换 dockHintClass→dockRegionClass）。

**测试**：纯视觉，运行时目测（拖拽时各方向预览块正确定位、落点直观）。

---

## B. 切 shell 入口（修 header + 加右键菜单）

**根因（确诊 bug）**：`PaneShellSwitcher.svelte` 的触发按钮 `{#if shells.length > 0}`（:101）才渲染，而 `shells` 仅在 `toggle()`（按钮 onclick，:47）里 `loadShells()` 加载。按钮没显示→无法点击→shells 永不加载→按钮永不出现。鸡生蛋死循环，入口从未出现过。

**方案**：

### B1. 新建共享模块 `src/lib/terminal/paneShell.ts`（DRY，header 与右键菜单共用）

```ts
import { invoke, isTauri } from '@tauri-apps/api/core';
import { get } from 'svelte/store';
import { activeWorkspaceId } from '$lib/stores/paneTree';
import { TerminalManager } from '$lib/terminal/manager';

export interface ShellInfo {
  id: string;
  label: string;
  program: string;
  args: string[];
}

let cache: ShellInfo[] | null = null;
let inflight: Promise<ShellInfo[]> | null = null;

/** 检测已装 shell，进程级缓存：只调一次后端，N 个 pane 的 header 与右键菜单共享。 */
export async function getShells(): Promise<ShellInfo[]> {
  if (cache) return cache;
  if (!isTauri()) return [];
  if (!inflight) {
    inflight = invoke<ShellInfo[]>('detect_available_shells')
      .then((s) => { cache = s; return s; })
      .catch((e) => { console.warn('detect_available_shells failed', e); return []; })
      .finally(() => { inflight = null; });
  }
  return inflight;
}

/** 原地切换某 pane 的 shell（拆 PTY→带 args 重建）。 */
export async function changePaneShell(paneId: string, shell: ShellInfo): Promise<void> {
  if (!isTauri()) return;
  const wsId = get(activeWorkspaceId);
  if (!wsId) return;
  const manager = TerminalManager.instance();
  manager.clearScrollback(paneId);
  await invoke('change_pane_shell', { paneId, shell: shell.program, args: shell.args ?? [] });
  await invoke('activate_pane_pty', {
    workspaceId: wsId,
    paneId,
    rows: manager.rows(paneId),
    cols: manager.cols(paneId),
  });
  manager.forceFullRedraw(paneId);
}
```

### B2. 修 `PaneShellSwitcher.svelte`

- 删除本地 `ShellInfo` 接口、本地 `loadShells`/`changePaneShell` 内嵌逻辑，改 import 共享模块的 `ShellInfo`/`getShells`/`changePaneShell`。
- **挂载时预加载**（修复 bug 的核心）：`$effect(() => { void getShells().then((s) => { shells = s; shellsLoaded = true; }); })`（或等价 onMount），使 `shells.length > 0` 在无需点击时即成立 → 按钮出现、持续显示当前 shell。
- `toggle()` 不再负责加载（shells 已在挂载时就绪），只算 popup 位置 + 翻 open。
- `selectShell(shell)`：`open=false`；`isCurrent(shell)` 则 return；否则 `await changePaneShell(paneId, shell)` + `selectedId = shell.id`（保留乐观）。`changing` 包裹。
- `getCurrentLabel`/`isCurrent` 逻辑不变（优先 `selectedId`，回退 `currentShell` 的 program 匹配）。

### B3. `RidgePane.svelte` 右键菜单加"切换终端类型 ▸"子菜单

- import `getShells`/`changePaneShell` + `Terminal`（lucide-svelte）。
- 挂载时 `getShells().then((s) => shells = s)` 填本地 `let shells = $state<ShellInfo[]>([])`（用于同步构建子菜单）。
- 在 `onContextMenu` 的 split 项与 close 项之间插入（仅当 `shells.length > 0`）：
  ```ts
  ...(shells.length > 0 ? [
    { id: 'term-sep-shell', divider: true },
    {
      id: 'term-shell',
      label: tr('workspace.ctxSwitchShell'),
      icon: Terminal,
      children: shells.map((s) => ({
        id: `term-shell-${s.id}`,
        label: s.label,
        action: () => { void changePaneShell(paneId, s); },
      })),
    },
  ] : []),
  ```
- 新增 i18n `workspace.ctxSwitchShell`（中："切换终端类型"；英："Switch shell type"），加在 `ctxSplitRight`/`ctxClosePanel` 同处（`src/lib/i18n/messages.ts` 的 workspace 命名空间 zh + en 两份）。

**影响文件**：新建 `src/lib/terminal/paneShell.ts`；改 `PaneShellSwitcher.svelte`、`RidgePane.svelte`、`src/lib/i18n/messages.ts`。

**测试**：`pnpm check` 0 errors；运行时目测——header 切换器出现并显示当前 shell；右键菜单出现"切换终端类型 ▸"子菜单、各项可切换；切后两入口标签随 layout 同步。

---

## 提交计划（每点单独 commit）
1. `fix(workspace): 停靠预览改方向半区填充（明确落点）` —— A
2. `fix(terminal): 修 PaneShellSwitcher 永不显示 + 抽共享 paneShell 模块` —— B1+B2
3. `feat(terminal): pane 右键菜单加"切换终端类型"子菜单` —— B3

## 开放项 / 不在范围
- I-2（`selectedId` 远端一致性）仍为 follow-up（Task #8）；右键菜单切换不更新 header 的 `selectedId`，切后靠 layout 同步 `currentShell` 刷新标签（WSL 多发行版经右键切时标签按 program 匹配取首个，属同一已知限制）。
- FileTree HTML5 DnD 不在范围。
