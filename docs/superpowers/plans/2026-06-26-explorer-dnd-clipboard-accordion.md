# 资源管理器三项改造 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让"文件拖入终端"走 bracketed paste（图片可被 TUI 识别为附件）、修复系统剪贴板复制被内部剪贴板遮住、并把文件树手风琴改成"顶层节点常驻、多开各自滚动"。

**Architecture:** 三块互相独立，各自一个 commit。#1 把 OS 拖放 + 文件树拖到终端两条路径统一改走 `TerminalManager.instance().paste()`；#2 用 Windows 剪贴板序列号判定内部剪贴板是否过期，外加右键"粘贴"；#3 把 `Explorer.svelte` 从"整树单滚动 + sticky"改为 flex 列布局（顶层头 flex 固定、展开 body 各自内部滚动）。

**Tech Stack:** Svelte 5 (runes)、TypeScript、Tauri 2、Rust（`clipboard-win` 5.4.1）、vitest。

## Global Constraints

- 思考用英文，回复用中文；代码注释沿用各仓库既有语言风格（本仓库注释多为中文）。
- 一功能一 commit，commit message 用中文。plan 不写人工手测清单（用户偏好）。
- 剪贴板 CF_HDROP / 序列号均 Windows 实现，非 Windows 退化（读空 / seq=0），与既有 `clipboard_files.rs` 约定一致。
- 终端管理器单例：`TerminalManager.instance()`（见 `src/lib/components/RidgePane.svelte:81`）。其方法 `paste(paneId: string, text: string): void` 会在 `?2004` 模式下用 bracketed-paste 包裹。
- 拖放粘贴一律裸路径、空格连接、**无引号、无末尾空格**（与既有 clipboard paste 管线一致；用户已确认接受含空格路径不加引号的代价）。

---

## Feature 1：文件拖入终端 → bracketed paste

### File Structure
- Create: `src/lib/terminal/dropPaste.ts` —— 纯函数：把拖入路径格式化成可粘贴文本。
- Create: `src/lib/terminal/dropPaste.test.ts` —— 单测。
- Modify: `src/routes/+page.svelte` —— `insertDroppedPaths()`（约 1098-1111）。
- Modify: `src/lib/components/FileTree.svelte` —— `pasteToTerminal()`（约 433-441）。

### Task 1.1：拖放路径格式化纯函数

**Files:**
- Create: `src/lib/terminal/dropPaste.ts`
- Test: `src/lib/terminal/dropPaste.test.ts`

**Interfaces:**
- Produces: `formatDroppedPathsForPaste(paths: string[]): string` —— trim 每条、去空串、空格连接，无末尾空格。

- [ ] **Step 1: 写失败测试**

`src/lib/terminal/dropPaste.test.ts`:
```ts
import { describe, it, expect } from 'vitest';
import { formatDroppedPathsForPaste } from './dropPaste';

describe('formatDroppedPathsForPaste', () => {
	it('单个路径原样返回（不加引号、不补空格）', () => {
		expect(formatDroppedPathsForPaste(['C:\\a\\img.png'])).toBe('C:\\a\\img.png');
	});
	it('含空格路径也不加引号', () => {
		expect(formatDroppedPathsForPaste(['C:\\my pics\\a.png'])).toBe('C:\\my pics\\a.png');
	});
	it('多个路径用空格连接', () => {
		expect(formatDroppedPathsForPaste(['a.png', 'b.jpg'])).toBe('a.png b.jpg');
	});
	it('trim 并丢弃空串', () => {
		expect(formatDroppedPathsForPaste([' a.png ', '', '  ', 'b.png'])).toBe('a.png b.png');
	});
	it('全空返回空串', () => {
		expect(formatDroppedPathsForPaste([])).toBe('');
	});
});
```

- [ ] **Step 2: 跑测试确认失败**

Run: `npx vitest run src/lib/terminal/dropPaste.test.ts`
Expected: FAIL（`formatDroppedPathsForPaste` 未定义 / 模块不存在）

- [ ] **Step 3: 写实现**

`src/lib/terminal/dropPaste.ts`:
```ts
// 把"拖入终端"的文件路径格式化成可经 bracketed-paste 粘进 PTY 的文本。
// 裸路径、空格连接、无引号、无末尾空格——与 clipboard paste 管线一致，
// 裸路径对 TUI（Claude Code 等）的图片附件识别最可靠。
export function formatDroppedPathsForPaste(paths: string[]): string {
	return paths
		.map((p) => p.trim())
		.filter((p) => p.length > 0)
		.join(' ');
}
```

- [ ] **Step 4: 跑测试确认通过**

Run: `npx vitest run src/lib/terminal/dropPaste.test.ts`
Expected: PASS（5 个用例）

- [ ] **Step 5: 接入 OS 拖放 `insertDroppedPaths`**

Modify `src/routes/+page.svelte`：在文件顶部已有 import 区加：
```ts
import { TerminalManager } from '$lib/terminal/manager';
import { formatDroppedPathsForPaste } from '$lib/terminal/dropPaste';
```
把 `insertDroppedPaths`（约 1098-1111）函数体改为：
```ts
  function insertDroppedPaths(paths: string[], position: { x: number; y: number }): void {
    if (!paths.length) return;
    const dpr = window.devicePixelRatio || 1;
    const el = document.elementFromPoint(position.x / dpr, position.y / dpr);
    const paneEl = el?.closest('[data-rg-pane-id]') as HTMLElement | null;
    const pid = paneEl?.getAttribute('data-rg-pane-id') || get(activePaneId);
    if (!pid) return;
    const text = formatDroppedPathsForPaste(paths);
    if (!text) return;
    activePaneId.set(pid);
    // 走 bracketed-paste（而非 write_to_pty 原样写）：TUI 据此把图片路径识别为附件。
    TerminalManager.instance().paste(pid, text);
  }
```

- [ ] **Step 6: 接入文件树拖到终端 `pasteToTerminal`**

Modify `src/lib/components/FileTree.svelte`：在 import 区加：
```ts
import { TerminalManager } from '$lib/terminal/manager';
import { formatDroppedPathsForPaste } from '$lib/terminal/dropPaste';
```
把 `pasteToTerminal`（约 433-441）改为：
```ts
	// 落到终端 pane：走 bracketed-paste（与 OS 拖放 insertDroppedPaths 行为统一）。
	function pasteToTerminal(paneId: string, paths: string[]): void {
		if (!isTauri() || paths.length === 0) return;
		const text = formatDroppedPathsForPaste(paths);
		if (!text) return;
		activePaneId.set(paneId);
		TerminalManager.instance().paste(paneId, text);
	}
```

- [ ] **Step 7: 类型检查 + 跑测试**

Run: `npx vitest run src/lib/terminal/dropPaste.test.ts`
Expected: PASS
（若仓库有 `npm run check`，可顺带跑一次确认两个组件无类型错误。）

- [ ] **Step 8: Commit**

```bash
git add src/lib/terminal/dropPaste.ts src/lib/terminal/dropPaste.test.ts src/routes/+page.svelte src/lib/components/FileTree.svelte
git commit -m "feat(terminal): 文件拖入终端改走 bracketed paste（OS 拖放 + 文件树拖拽统一），图片可被 TUI 识别为附件"
```

---

## Feature 2：剪贴板序列号判过期 + 右键粘贴

### File Structure
- Modify: `src-tauri/src/commands/clipboard_files.rs` —— 新增 `read_clipboard_sequence`。
- Modify: `src-tauri/src/lib.rs:697` —— 注册命令。
- Modify: `src/lib/stores/fileExplorer.ts` —— `ExplorerClipboard` 加 `seq`；新增纯函数 `resolveActiveClipboard`。
- Create: `src/lib/stores/clipboardResolve.test.ts` —— `resolveActiveClipboard` 单测。
- Modify: `src/lib/components/Explorer.svelte` —— 复制/剪切写 seq；`pasteClipboard` 用 seq 判定 + 接受 target 参数；cwd 右键加"粘贴"；FileTree 实例传 `onPaste`。
- Modify: `src/lib/components/FileTree.svelte` —— 加 `onPaste` prop + 节点右键"粘贴"项。
- Modify: `src/lib/i18n/messages/explorer.ts` —— 加 `ctxPaste`（zh + en）。

### Task 2.1：后端读剪贴板序列号命令

**Files:**
- Modify: `src-tauri/src/commands/clipboard_files.rs`
- Modify: `src-tauri/src/lib.rs:697`

**Interfaces:**
- Produces: Tauri 命令 `read_clipboard_sequence() -> u32`（Windows 取 `GetClipboardSequenceNumber`，非 Windows / 取不到返回 0）。

- [ ] **Step 1: 加命令实现**

在 `src-tauri/src/commands/clipboard_files.rs` 末尾追加：
```rust
/// 读 Windows 剪贴板序列号（内容每次变化即自增；无需打开剪贴板）。
/// 用于判定 ridge 内部文件剪贴板是否已被外部应用改写而过期。
/// 非 Windows / 取不到时返回 0。
#[tauri::command]
pub fn read_clipboard_sequence() -> u32 {
    read_clipboard_sequence_impl()
}

#[cfg(windows)]
fn read_clipboard_sequence_impl() -> u32 {
    // clipboard_win::seq_num() -> Option<NonZeroU32>，包装 GetClipboardSequenceNumber。
    clipboard_win::seq_num().map(|n| n.get()).unwrap_or(0)
}

#[cfg(not(windows))]
fn read_clipboard_sequence_impl() -> u32 {
    0
}
```

- [ ] **Step 2: 注册命令**

Modify `src-tauri/src/lib.rs`：在 `clipboard_files::write_clipboard_file_paths,`（约 697 行）下一行加：
```rust
            clipboard_files::read_clipboard_sequence,
```

- [ ] **Step 3: 编译确认**

Run: `cargo check -p ridge`（在 `src-tauri/`；若用户的常驻 dev 已在编译，跳过避免并行 cargo——见用户偏好"勿并行 cargo check"）
Expected: 编译通过（无 `read_clipboard_sequence` 未注册/未定义错误）。

- [ ] **Step 4: Commit（后端独立小步）**

```bash
git add src-tauri/src/commands/clipboard_files.rs src-tauri/src/lib.rs
git commit -m "feat(clipboard): 新增 read_clipboard_sequence 命令（GetClipboardSequenceNumber）"
```

### Task 2.2：`ExplorerClipboard.seq` + `resolveActiveClipboard` 纯函数

**Files:**
- Modify: `src/lib/stores/fileExplorer.ts:836-846`
- Test: `src/lib/stores/clipboardResolve.test.ts`

**Interfaces:**
- Consumes: `ExplorerClipboard`（现有），新增字段 `seq: number`。
- Produces: `resolveActiveClipboard(internal: ExplorerClipboard | null, currentSeq: number, systemFiles: string[]): ExplorerClipboard | null`
  - 内部非空且 `internal.seq === currentSeq` → 返回 internal（权威，覆盖 copy/cut）。
  - 否则系统文件非空 → 返回 `{ paths: 去空后的系统文件, mode: 'copy', seq: currentSeq }`。
  - 都不满足但内部非空 → 返回 internal（兜底）。
  - 否则 null。

- [ ] **Step 1: 改接口 + 加纯函数**

Modify `src/lib/stores/fileExplorer.ts`：把接口（836-839）改为：
```ts
export interface ExplorerClipboard {
	paths: string[];
	mode: 'copy' | 'cut';
	/** 设置该剪贴板时的系统剪贴板序列号（用于判定是否被外部改写而过期）。 */
	seq: number;
}
```
并在 `setExplorerClipboard`（844-846）之后追加：
```ts
/**
 * 判定本次粘贴应采用哪份剪贴板：内部（ridge 自己 copy/cut）还是系统（外部应用 copy 的文件）。
 * 规则：内部剪贴板序列号与当前系统序列号一致 → 内部权威（外部未改写过，覆盖 copy/cut）；
 * 否则内部已过期 → 优先用系统文件列表（一律按 copy）；系统也为空才退回内部兜底。
 * 纯函数，便于单测；真正读序列号/文件列表的 IPC 留在组件层。
 */
export function resolveActiveClipboard(
	internal: ExplorerClipboard | null,
	currentSeq: number,
	systemFiles: string[]
): ExplorerClipboard | null {
	if (internal && internal.paths.length > 0 && internal.seq === currentSeq) {
		return internal;
	}
	const sys = systemFiles.map((p) => p.trim()).filter((p) => p.length > 0);
	if (sys.length > 0) {
		return { paths: sys, mode: 'copy', seq: currentSeq };
	}
	if (internal && internal.paths.length > 0) {
		return internal;
	}
	return null;
}
```

- [ ] **Step 2: 写失败测试**

`src/lib/stores/clipboardResolve.test.ts`:
```ts
import { describe, it, expect } from 'vitest';
import { resolveActiveClipboard } from './fileExplorer';

const internalCopy = { paths: ['C:\\ridge\\old.txt'], mode: 'copy' as const, seq: 100 };
const internalCut = { paths: ['C:\\ridge\\moved.txt'], mode: 'cut' as const, seq: 100 };

describe('resolveActiveClipboard', () => {
	it('序列号一致 → 用内部复制', () => {
		expect(resolveActiveClipboard(internalCopy, 100, ['C:\\ext\\new.txt'])).toBe(internalCopy);
	});
	it('序列号一致 → 用内部剪切', () => {
		expect(resolveActiveClipboard(internalCut, 100, [])).toBe(internalCut);
	});
	it('序列号变了 + 系统有文件 → 用系统(复制)', () => {
		expect(resolveActiveClipboard(internalCopy, 101, ['C:\\ext\\new.txt'])).toEqual({
			paths: ['C:\\ext\\new.txt'],
			mode: 'copy',
			seq: 101,
		});
	});
	it('序列号变了 + 系统为空 → 退回内部兜底', () => {
		expect(resolveActiveClipboard(internalCut, 101, [])).toBe(internalCut);
	});
	it('无内部 + 系统有文件 → 用系统', () => {
		expect(resolveActiveClipboard(null, 5, [' C:\\a.txt ', ''])).toEqual({
			paths: ['C:\\a.txt'],
			mode: 'copy',
			seq: 5,
		});
	});
	it('无内部 + 系统为空 → null', () => {
		expect(resolveActiveClipboard(null, 5, [])).toBeNull();
	});
});
```

- [ ] **Step 3: 跑测试**

Run: `npx vitest run src/lib/stores/clipboardResolve.test.ts`
Expected: PASS（6 个用例）。若编译报 `ExplorerClipboard` 其它构造处缺 `seq`，记录待 Task 2.3 修。

- [ ] **Step 4: Commit**

```bash
git add src/lib/stores/fileExplorer.ts src/lib/stores/clipboardResolve.test.ts
git commit -m "feat(explorer): ExplorerClipboard 加 seq 字段 + resolveActiveClipboard 过期判定纯函数"
```

### Task 2.3：复制/剪切写 seq + 粘贴用 seq 判定

**Files:**
- Modify: `src/lib/components/Explorer.svelte`（`handleRootKeydown` 的 Ctrl+C/X 分支约 553-585；`pasteClipboard` 约 452-465）

**Interfaces:**
- Consumes: `resolveActiveClipboard`（Task 2.2）、命令 `read_clipboard_sequence`（Task 2.1）、现有 `read_clipboard_file_paths` / `write_clipboard_file_paths`。
- Produces: `pasteClipboard(target?: { columnId?: string; targetPath?: string }): Promise<void>`（供 Ctrl+V 与右键复用）。

- [ ] **Step 1: 引入 `resolveActiveClipboard`**

Modify `src/lib/components/Explorer.svelte`：在从 `$lib/stores/fileExplorer` 的具名 import 列表里加 `resolveActiveClipboard`（紧邻已有的 `explorerClipboard, setExplorerClipboard`）。

- [ ] **Step 2: 复制/剪切时记录 seq**

把 `handleRootKeydown` 里 Ctrl+C/X 分支（约 553-585）从 `const mode...` 到该分支 `return;` 之间替换为：
```ts
			if (e.key === 'c' || e.key === 'C' || e.key === 'x' || e.key === 'X') {
				const state = get(fileExplorerStore);
				const col = state.columns.find((c) => c.selectedPath);
				if (!col) return;
				const paths = Array.from(col.selectedPaths);
				if (paths.length === 0) return;
				const mode: 'copy' | 'cut' = e.key.toLowerCase() === 'c' ? 'copy' : 'cut';
				e.preventDefault();
				void (async () => {
					if (mode === 'copy' && isTauri()) {
						try {
							// 一次写 CF_HDROP + 文本镜像；返回 true 表示已连带写文本。
							const wroteText = await invoke<boolean>('write_clipboard_file_paths', { paths });
							if (!wroteText) await writeText(paths.join('\n'));
						} catch (err) {
							try { await writeText(paths.join('\n')); }
							catch (e2) { console.warn('[explorer] clipboard writeText failed', e2); }
						}
					}
					// 记录"设置此剪贴板时"的系统序列号：copy 在写完后读（含我们这次写入），
					// cut 直接读当前值（cut 不写系统剪贴板）。
					let seq = 0;
					if (isTauri()) {
						try { seq = await invoke<number>('read_clipboard_sequence'); } catch { /* 退化为 0 */ }
					}
					setExplorerClipboard({ paths, mode, seq });
				})();
				return;
			}
```

- [ ] **Step 3: `pasteClipboard` 改用 seq 判定 + 接受 target**

把 `pasteClipboard` 的函数签名与开头取剪贴板的段（约 452-465，从 `async function pasteClipboard()` 到第一个 `if (!clip || clip.paths.length === 0) return;`）替换为：
```ts
	/** Paste clipboard into the target dir（右键传 target；Ctrl+V 不传，用当前选中）。 */
	async function pasteClipboard(target?: { columnId?: string; targetPath?: string }): Promise<void> {
		if (!isTauri()) return;
		// 读当前系统序列号 + 系统文件列表，交给纯函数判定该用内部还是系统剪贴板。
		let curSeq = 0;
		try { curSeq = await invoke<number>('read_clipboard_sequence'); } catch { /* 0 */ }
		let sysFiles: string[] = [];
		try { sysFiles = await invoke<string[]>('read_clipboard_file_paths'); }
		catch (err) { console.warn('[explorer] read system clipboard files failed', err); }
		const clip = resolveActiveClipboard(get(explorerClipboard), curSeq, sysFiles);
		if (!clip || clip.paths.length === 0) return;
```
紧接着的 `const state = get(fileExplorerStore);` 段（找目标列/目录）改为支持 target：
把
```ts
		const state = get(fileExplorerStore);
		// Find the active column & target dir.
		let col = state.columns.find((c) => c.selectedPath);
		if (!col) col = state.columns.find((c) => c.tree);
		if (!col) return;

		let targetDir: string | null = null;
		const primary = col.selectedPath;
```
替换为：
```ts
		const state = get(fileExplorerStore);
		// Find the active column & target dir（右键 target 优先，否则用当前选中/首个有树的列）。
		let col = target?.columnId ? state.columns.find((c) => c.id === target.columnId) : undefined;
		if (!col) col = state.columns.find((c) => c.selectedPath);
		if (!col) col = state.columns.find((c) => c.tree);
		if (!col) return;

		let targetDir: string | null = null;
		const primary = target?.targetPath ?? col.selectedPath;
```
（其余目录判定、copy/move 循环、刷新逻辑不变——它们都基于 `primary` / `clip.mode`，自动适配。）

- [ ] **Step 4: 类型检查**

Run: `npx vitest run src/lib/stores/clipboardResolve.test.ts`（确认接口改动未破坏纯函数测试）
Expected: PASS。并确认 `Explorer.svelte` 无类型错误（如本地有 `npm run check` 则跑一次）。

- [ ] **Step 5: Commit**

```bash
git add src/lib/components/Explorer.svelte
git commit -m "fix(explorer): 粘贴用剪贴板序列号判内部剪贴板过期，修系统复制被旧内部剪贴板遮住"
```

### Task 2.4：右键"粘贴"项（FileTree 节点 + cwd 空白）

**Files:**
- Modify: `src/lib/i18n/messages/explorer.ts:29,90`
- Modify: `src/lib/components/FileTree.svelte`（props 约 42-55；`handleContextMenu` 约 541-560）
- Modify: `src/lib/components/Explorer.svelte`（`showCwdContextMenu` 约 375-385；FileTree 实例 约 927-940）

**Interfaces:**
- Consumes: `pasteClipboard(target?)`（Task 2.3）。
- Produces: FileTree 新增 prop `onPaste?: (targetPath: string) => void`。

- [ ] **Step 1: i18n 加 `ctxPaste`**

Modify `src/lib/i18n/messages/explorer.ts`：
- 在 zh 块 `ctxCopyRelative: '复制相对路径',`（约 29）下一行加：`ctxPaste: '粘贴',`
- 在 en 块 `ctxCopyRelative: 'Copy Relative Path',`（约 90）下一行加：`ctxPaste: 'Paste',`

- [ ] **Step 2: FileTree 加 `onPaste` prop**

Modify `src/lib/components/FileTree.svelte`：在 `Props` 接口里（与现有 `onSelect?` 同级）加：
```ts
		/** 右键"粘贴"回调：把剪贴板内容粘到本节点（目录粘入其内，文件粘入其父目录）。 */
		onPaste?: (targetPath: string) => void;
```
并在 `let { ... }: Props = $props();` 解构里加入 `onPaste`。

- [ ] **Step 3: FileTree 节点右键加"粘贴"项**

在 `handleContextMenu`（约 541-560）的两个 `items` 数组里，各加一条"粘贴"。目录分支：放在 `copy-rel` 之后、`reveal` 之前；文件分支：放在 `copy-rel` 之后、`reveal` 之前。两处都插入：
```ts
					{ id: 'paste', label: tr('explorer.ctxPaste'), action: () => onPaste?.(pathAtMenu) },
```

- [ ] **Step 4: Explorer 把 `onPaste` 接到 FileTree 实例**

Modify `src/lib/components/Explorer.svelte`：在 `<FileTree ... />`（约 927-940）的属性里，紧挨 `onSelect={...}` 加：
```svelte
												onPaste={(path) => void pasteClipboard({ columnId: col.id, targetPath: path })}
```

- [ ] **Step 5: cwd 空白右键加"粘贴"项**

Modify `src/lib/components/Explorer.svelte` `showCwdContextMenu`（约 375-385）：在 `new-folder` 项之后、第一个 divider 之前插入：
```ts
			{ id: 'paste', label: tr('explorer.ctxPaste'), action: () => void pasteClipboard({ columnId: col.id }) },
```

- [ ] **Step 6: 类型检查 + 测试**

Run: `npx vitest run src/lib/stores/clipboardResolve.test.ts src/lib/terminal/dropPaste.test.ts`
Expected: PASS。确认 FileTree / Explorer 无类型错误。

- [ ] **Step 7: Commit**

```bash
git add src/lib/i18n/messages/explorer.ts src/lib/components/FileTree.svelte src/lib/components/Explorer.svelte
git commit -m "feat(explorer): 文件树节点 + cwd 空白右键加\"粘贴\"项（复用序列号判定的 pasteClipboard）"
```

---

## Feature 3：手风琴 — 多开 + 顶节点常驻（flex 布局）

> 纯 UI/CSS 重构，无纯逻辑可 TDD；以"精确编辑 + 视觉手动验证"推进。实现者按需微调 `min-height` / flex 值。

### File Structure
- Modify: `src/lib/components/Explorer.svelte`（根 `.explorer` 容器约 734-741；`.explorer-workspace` 约 756-758；`.explorer-section` 约 832；cwd 头约 834-844；body 约 902-905；`<style>` 约 961-981）

### 目标布局
- `.explorer`：flex 列、`min-h-0`、原生竖向滚动（去 `use:overlayScroll`，改用 `rg-scroll` 作"头太多时"的外层兜底滚动）。
- 工作区头、cwd 头：`flex:0 0 auto`，永在流中、不随文件列表滚走；保留 sticky 作外层滚动时的二级钉住。
- 展开的 cwd body：`flex:1 1 0; min-height:6rem; overflow-y:auto`（`rg-scroll`），多个展开 body 平分剩余纵向空间、各自内部滚动。
- 用 `display:contents` 抹平 `.explorer-workspace` / `.explorer-section` 的盒子，使各级头/体成为 `.explorer` 的直接 flex 项；被抹平元素的边框移到对应头部。

### Task 3.1：根容器改 flex + 去整树滚动

**Files:** `src/lib/components/Explorer.svelte`

- [ ] **Step 1: 改根容器**

把根 `<div class="explorer flex h-full flex-col" ... use:overlayScroll onkeydown={handleRootKeydown} role="tree">`（约 734-741）改为：
```svelte
<div
	class="explorer flex h-full min-h-0 flex-col overflow-y-auto rg-scroll"
	data-testid="file-tree"
	tabindex="-1"
	onkeydown={handleRootKeydown}
	role="tree"
>
```
（删除 `use:overlayScroll`；保留其余属性。`overlayScroll` import 若不再被本组件使用则一并删除。）

- [ ] **Step 2: 抹平 workspace / section 盒子**

- `.explorer-workspace` 的 `<div class="explorer-workspace border-b border-[var(--rg-border)] last:border-b-0">`（约 756-758）→ 改为 `<div class="explorer-workspace" style="display:contents">`（边框改由其下首个头承担）。
- `.explorer-section` 的 `<div class="explorer-section group/col border-t border-[var(--rg-border)]/50">`（约 832）→ 改为 `<div class="explorer-section" style="display:contents">`。
- 把 `group/col`（refresh 按钮 hover 依赖）从 section 移到 **cwd 头 div**（约 834 那个 `sticky top-8 ...` 的 div）：在其 class 串里加 `group/col`。
- cwd 头加上分隔边框：在该头 class 串里加 `border-t border-[var(--rg-border)]/50`。

- [ ] **Step 3: body 改 flex 项 + 内部滚动**

把文件树 body（约 902-905）的 class
```
relative explorer-body py-0.5 {group.workspaceId !== $activeWorkspaceId ? "max-h-[32vh] overflow-y-auto rg-scroll" : ""}
```
改为（去掉"活动/非活动"特例，统一 flex+内部滚动）：
```
relative explorer-body py-0.5 min-h-[6rem] flex-1 basis-0 overflow-y-auto rg-scroll
```
（其余 `oncontextmenu` 等属性不变。）

- [ ] **Step 4: 清理 `<style>`**

`<style>` 内若无对 `.explorer-progress` 之外的 overlayscrollbars 局部覆盖则不动；确认无遗留对 `.explorer` overflow 的冲突规则。`.explorer-progress` 进度条样式保留。

- [ ] **Step 5: 视觉手动验证**

在常驻 tauri dev 里打开多工作区、每个工作区多个 cwd、展开多个 cwd 并塞入长文件列表，确认：
1. 所有工作区头 + cwd 头始终可见，不被文件列表推走；
2. 每个展开 cwd 的文件列表在自身区域内部滚动；
3. 折叠/展开、刷新按钮 hover 显隐、pane 标签条、慢加载进度条均正常；
4. 工作区/cwd 数量极多时，外层 `rg-scroll` 兜底出现且 sticky 头仍钉住。
按观感微调 body `min-h`（6rem 起）。

- [ ] **Step 6: Commit**

```bash
git add src/lib/components/Explorer.svelte
git commit -m "feat(explorer): 手风琴改 flex 布局，工作区/cwd 顶层节点常驻可见、各 cwd 文件列表内部滚动"
```

---

## Self-Review

**Spec coverage：**
- #1 拖入终端走 bracketed paste → Task 1.1（含 OS 拖放 + 文件树拖拽两条路径）✓
- #2 序列号判过期 → Task 2.1（命令）+ 2.2（字段/纯函数）+ 2.3（写 seq/粘贴判定）✓；右键粘贴 → Task 2.4 ✓；i18n `ctxPaste` → Task 2.4 Step 1 ✓
- #3 多开 + 顶节点常驻 flex → Task 3.1 ✓
- 提交策略：功能级 commit 对齐（#1 一个、#2 拆 2.1 后端/2.2 纯函数/2.3 逻辑/2.4 右键四个小 commit、#3 一个）；#2 多 commit 是同一功能内的安全小步，符合"一功能一关注点"。

**Placeholder scan：** 无 TBD/TODO；每个代码步给了完整代码；Feature 3 为 CSS 改动，明确标注"手动验证 + 微调"，非占位。

**Type consistency：**
- `formatDroppedPathsForPaste(paths: string[]): string` 在 Task 1.1 定义、Step 5/6 使用，签名一致。
- `ExplorerClipboard` 加 `seq: number` 后，所有构造点（copy/cut 处的 `setExplorerClipboard({paths,mode,seq})`、`resolveActiveClipboard` 返回）均带 seq；`setExplorerClipboard(null)` 清空不受影响。
- `resolveActiveClipboard(internal, currentSeq, systemFiles)` 定义（2.2）与使用（2.3 Step 3）一致。
- `pasteClipboard(target?)` 定义（2.3）与调用（Ctrl+V 无参、FileTree onPaste、cwd 右键）一致；FileTree `onPaste(targetPath)` 定义与 Explorer 传入一致。
- `read_clipboard_sequence` Rust 命令名与前端 `invoke('read_clipboard_sequence')` 一致。
