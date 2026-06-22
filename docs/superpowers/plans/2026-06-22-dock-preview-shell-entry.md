# 停靠提示加强 + 切 shell 入口 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 拖拽停靠提示改方向半区预览填充（明确落点）；修复 pane header 切 shell 入口"永不显示"的 bug 并抽共享模块；pane 右键菜单加"切换终端类型"子菜单。

**Architecture:** SvelteKit 前端（Svelte 5 runes）。停靠覆盖层在 `SplitContainer.svelte` 叶级、由 `$paneDockHover` store 驱动。切 shell 逻辑抽到新共享模块 `src/lib/terminal/paneShell.ts`（缓存的 `getShells` + `changePaneShell`），由 header 组件 `PaneShellSwitcher.svelte` 与 pane 右键菜单 `RidgePane.svelte` 共用。

**Tech Stack:** Svelte 5、Tailwind v4、Tauri v2、lucide-svelte 图标。

## Global Constraints

- 思考用英文、回复/报告用中文；注释沿用各文件既有语言风格（中文）。
- 单独 commit；commit message 末尾加 `Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>`。当前分支 `develop`。
- 前端类型检查用 `pnpm check`（在 `C:\code\wind` 下）必须 0 errors。本机有常驻 `tauri dev`——**不要**自己起 dev/build/cargo。
- IDE 可能对从 `.svelte` 文件具名 import（如 `alertDialog`）报 TS2614，那是 IDE 误报；`pnpm check`（svelte-check）才是权威。
- 只改各 Task 列出的文件。运行时视觉/交互需真 Tauri 窗口目测（由控制者/用户做）。

---

### Task 1: 停靠提示改方向半区预览填充（A）

**Files:**
- Modify: `src/lib/components/SplitContainer.svelte`（`dockHintClass` 函数 `:131-138`；叶级覆盖层块 `:524-531`）

**Interfaces:**
- Consumes: 现有 `$paneDragSourceId` / `$paneDockHover`（store，`{paneId, region}|null`）、`DockRegion` 类型。
- Produces: 无（组件内部）。

- [ ] **Step 1: 把 `dockHintClass` 替换为 `dockRegionClass`**

将 `SplitContainer.svelte` 的：

```svelte
  function dockHintClass(h: DockRegion | null): string {
    if (!h) return '';
    if (h === 'left') return 'shadow-[inset_5px_0_0_0_var(--rg-accent)]';
    if (h === 'right') return 'shadow-[inset_-5px_0_0_0_var(--rg-accent)]';
    if (h === 'top') return 'shadow-[inset_0_5px_0_0_var(--rg-accent)]';
    if (h === 'bottom') return 'shadow-[inset_0_-5px_0_0_var(--rg-accent)]';
    return 'ring-2 ring-[var(--rg-accent)] ring-inset';
  }
```

替换为（返回预览块的定位 class）：

```svelte
  // 返回方向半区预览块的定位 class：明确显示拖拽 pane 将落入的区域。
  function dockRegionClass(h: DockRegion | null): string {
    if (!h) return '';
    if (h === 'left') return 'inset-y-0 left-0 w-1/2';
    if (h === 'right') return 'inset-y-0 right-0 w-1/2';
    if (h === 'top') return 'inset-x-0 top-0 h-1/2';
    if (h === 'bottom') return 'inset-x-0 bottom-0 h-1/2';
    return 'inset-[20%]';
  }
```

- [ ] **Step 2: 重构叶级覆盖层（淡遮罩 + 定位预览块）**

将覆盖层块：

```svelte
          {#if $paneDragSourceId && $paneDragSourceId !== node.id}
            {@const hover = $paneDockHover && $paneDockHover.paneId === node.id ? $paneDockHover.region : null}
            <div
              class="absolute inset-0 z-30 rounded-lg bg-black/25 transition-shadow pointer-events-none {dockHintClass(hover)}"
              role="region"
              aria-label={$t('workspace.dockHereLabel')}
            ></div>
          {/if}
```

替换为：

```svelte
          {#if $paneDragSourceId && $paneDragSourceId !== node.id}
            {@const hover = $paneDockHover && $paneDockHover.paneId === node.id ? $paneDockHover.region : null}
            <div
              class="absolute inset-0 z-30 rounded-lg bg-black/15 pointer-events-none"
              role="region"
              aria-label={$t('workspace.dockHereLabel')}
            >
              {#if hover}
                <div
                  class="absolute bg-[var(--rg-accent)]/25 border-2 border-[var(--rg-accent)] rounded transition-all duration-100 {dockRegionClass(hover)}"
                ></div>
              {/if}
            </div>
          {/if}
```

- [ ] **Step 3: 类型检查**

Run: `pnpm check`
Expected: PASS（0 errors；`dockHintClass` 已无引用、`dockRegionClass` 已被覆盖层使用，无未用告警）

- [ ] **Step 4: Commit**

```bash
git add src/lib/components/SplitContainer.svelte
git commit -m "fix(workspace): 停靠预览改方向半区填充（明确落点）

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 2: 抽共享 `paneShell` 模块 + 修 PaneShellSwitcher 永不显示（B1+B2）

**Files:**
- Create: `src/lib/terminal/paneShell.ts`
- Modify: `src/lib/components/PaneShellSwitcher.svelte`

**Interfaces:**
- Produces:
  - `export interface ShellInfo { id: string; label: string; program: string; args: string[] }`
  - `export async function getShells(): Promise<ShellInfo[]>`（进程级缓存）
  - `export async function changePaneShell(paneId: string, shell: ShellInfo): Promise<void>`

- [ ] **Step 1: 新建 `src/lib/terminal/paneShell.ts`**

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
      .then((s) => {
        cache = s;
        return s;
      })
      .catch((e) => {
        console.warn('detect_available_shells failed', e);
        return [];
      })
      .finally(() => {
        inflight = null;
      });
  }
  return inflight;
}

/** 原地切换某 pane 的 shell（拆 PTY → 带 args 重建）。 */
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

- [ ] **Step 2: 重写 `PaneShellSwitcher.svelte` 的 `<script>` 段**

把整个 `<script lang="ts"> ... </script>`（约 `:1-99`）替换为：

```svelte
<script lang="ts">
  import { t, tr } from '$lib/i18n';
  import { ChevronDown, Terminal } from 'lucide-svelte';
  import { portal } from '$lib/actions/portal';
  import { getShells, changePaneShell, type ShellInfo } from '$lib/terminal/paneShell';

  interface Props {
    paneId: string;
    currentShell?: string;
  }
  let { paneId, currentShell }: Props = $props();

  // 切换成功后立即记下选中的 ShellInfo.id（乐观）；layout 回传的 shell_kind(program)
  // 在 WSL 多发行版同 program 时不足以区分，故优先用 selectedId。
  let selectedId = $state<string | null>(null);
  let open = $state(false);
  let shells = $state<ShellInfo[]>([]);
  let changing = $state(false);
  let btnEl: HTMLButtonElement | undefined = $state();
  let popupStyle = $state('');

  // 挂载即预加载（共享缓存）。修复旧 bug：旧实现仅在点击 toggle 时加载 shells，
  // 而按钮 {#if shells.length>0} 才渲染 → 按钮永不出现、永不可点。
  $effect(() => {
    void getShells().then((s) => {
      shells = s;
    });
  });

  function toggle() {
    if (btnEl) {
      const r = btnEl.getBoundingClientRect();
      popupStyle = `top:${r.bottom + 4}px;left:${r.left}px`;
    }
    open = !open;
  }

  function getCurrentLabel(): string {
    if (selectedId) {
      const byId = shells.find((s) => s.id === selectedId);
      if (byId) return byId.label;
    }
    if (currentShell) {
      const byProg = shells.find((s) => s.program === currentShell);
      if (byProg) return byProg.label;
    }
    if (shells.length > 0) return shells[0].label;
    return tr('workspace.shellFallback');
  }

  // 菜单内"当前项"判定：优先 selectedId，否则匹配 program。
  function isCurrent(s: ShellInfo): boolean {
    if (selectedId) return s.id === selectedId;
    return !!currentShell && s.program === currentShell;
  }

  async function selectShell(shell: ShellInfo) {
    open = false;
    if (isCurrent(shell)) return;
    changing = true;
    try {
      await changePaneShell(paneId, shell);
      selectedId = shell.id;
    } catch (e) {
      console.warn('change_pane_shell failed', e);
    } finally {
      changing = false;
    }
  }
</script>
```

模板部分（`{#if shells.length > 0}` 起，约 `:101` 之后）**不改**——它已用 `shells`/`getCurrentLabel`/`isCurrent`/`toggle`/`selectShell`/`{#each shells as s (s.id)}`，与新 script 完全兼容。

- [ ] **Step 3: 类型检查**

Run: `pnpm check`
Expected: PASS（0 errors。新 script 不再直接用 `invoke`/`isTauri`/`activeWorkspaceId`/`TerminalManager`，确认这些 import 已从组件移除，无未用告警）

- [ ] **Step 4: Commit**

```bash
git add src/lib/terminal/paneShell.ts src/lib/components/PaneShellSwitcher.svelte
git commit -m "fix(terminal): 修 PaneShellSwitcher 永不显示 + 抽共享 paneShell 模块

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 3: pane 右键菜单加"切换终端类型"子菜单（B3）

**Files:**
- Modify: `src/lib/components/RidgePane.svelte`（import 区；新增本地 `shells` 状态 + 挂载加载；`onContextMenu` 的菜单项数组 `:1495-1505` 区间）
- Modify: `src/lib/i18n/messages/workspace.ts`（zh `:37-39` 与 en `:108-110` 各加 `ctxSwitchShell`）

**Interfaces:**
- Consumes: `getShells` / `changePaneShell` / `ShellInfo`（Task 2）；`ContextMenuItem` 支持 `children?: ContextMenuItem[]` 与 `icon?`（已存在）。

- [ ] **Step 1: i18n 加 `ctxSwitchShell`**

在 `src/lib/i18n/messages/workspace.ts` 的中文块（`ctxSplitDown: '向下拆分',` 之后）加：

```ts
  ctxSwitchShell: '切换终端类型',
```

在英文块（`ctxSplitDown: 'Split down',` 之后）加：

```ts
  ctxSwitchShell: 'Switch shell type',
```

- [ ] **Step 2: RidgePane import + 本地 shells 状态 + 挂载加载**

在 `RidgePane.svelte` 的 import 区（`import { TerminalManager } from '$lib/terminal/manager';` 附近）加：

```ts
import { getShells, changePaneShell, type ShellInfo } from '$lib/terminal/paneShell';
import { Terminal } from 'lucide-svelte';
```

在脚本中（靠近其它 `$state` 声明处）加本地状态与挂载预加载：

```ts
// 右键菜单"切换终端类型"子菜单需要 shell 列表；挂载预加载（共享缓存），
// 使 onContextMenu 能同步构建子菜单项。
let shells = $state<ShellInfo[]>([]);
$effect(() => {
  void getShells().then((s) => {
    shells = s;
  });
});
```

- [ ] **Step 3: onContextMenu 插入子菜单**

在 `showContextMenu(...)` 的菜单项数组里，`term-split-down` 项与 `term-sep3` 分隔符之间插入：

```ts
			{ id: 'term-split-down', label: tr('workspace.ctxSplitDown'), action: () => {
				void splitPane(paneId, 'vertical');
			}},
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
			{ id: 'term-sep3', divider: true },
```

- [ ] **Step 4: 类型检查**

Run: `pnpm check`
Expected: PASS（0 errors。`ContextMenuItem[]` 接受 `children`/`icon`；spread 的对象数组类型兼容）

- [ ] **Step 5: Commit**

```bash
git add src/lib/components/RidgePane.svelte src/lib/i18n/messages/workspace.ts
git commit -m "feat(terminal): pane 右键菜单加\"切换终端类型\"子菜单

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Self-Review

**Spec coverage:**
- A 方向半区预览 → Task 1（dockRegionClass + 覆盖层重构）。✓
- B1 共享 paneShell.ts → Task 2 Step 1。✓
- B2 修 PaneShellSwitcher 永不显示（挂载预加载）→ Task 2 Step 2。✓
- B3 右键菜单子菜单 + i18n → Task 3。✓

**Placeholder scan:** 无 TBD/TODO/"类似上文"；每步含完整代码。

**Type consistency:**
- `ShellInfo { id, label, program, args }` 在 paneShell.ts（Task 2）定义，PaneShellSwitcher（Task 2）与 RidgePane（Task 3）均 `import type` 复用，一致。✓
- `getShells(): Promise<ShellInfo[]>` / `changePaneShell(paneId: string, shell: ShellInfo)` 签名在 Task 2 定义，Task 3 调用一致。✓
- `dockRegionClass(h: DockRegion | null)` 在 Task 1 定义并在同任务覆盖层使用，无跨任务漂移。✓
- `tr('workspace.ctxSwitchShell')`（Task 3 Step 3）↔ i18n key（Task 3 Step 1）一致。✓

**已知限制（刻意，非缺口）：** 右键菜单切 shell 不更新 header `selectedId`，切后靠 layout 同步 `currentShell` 刷新标签（WSL 多发行版经右键切时标签按 program 取首个，与 follow-up #8 同源限制）。I-2、FileTree DnD 不在范围。
