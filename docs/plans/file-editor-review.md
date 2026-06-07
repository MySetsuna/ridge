# 文件编辑器可用性审查报告

审查范围：`src/lib/components/FileEditor.svelte`（Monaco 编辑器组件）、`src/lib/stores/fileEditor.ts`（编辑器 store）、`src/lib/components/DiffEditorModal.svelte`（现仅导出 `openDiffEditor`）、以及打开/保存链路（`FileTree.svelte` → `Explorer.handleFileSelect` → `fileEditorStore.openFile`；`fileWatcherSync.ts` → `handleExternalChange`；Rust 命令 `read_file_for_editor` / `write_file` / `apply_file_edits`）。

编辑器实现为 **Monaco**（非 CodeMirror），无 worker，单实例 + 按 path 的 keep-alive model 缓存。

---

## 已确认根因并修复

### finding-1【CRITICAL — 已修复】打开文件时 `each_key_duplicate`，导致 tab 丢失/串内容

- **位置（根因）**：`src/lib/stores/fileEditor.ts:301` `openFile()` —— 早期 `existing` 查找（:314）与最终 push（:426）之间隔着一个 `await invoke('read_file_for_editor')`（:394）。
- **触发渲染崩溃处**：`src/lib/components/FileEditor.svelte:1315` `{#each dndItems as it (it.id)}`，其中 `it.id === f.path`。
- **GM 实测现场证据**：在运行中的 host（WebView2 CDP）打开文件时控制台抛
  `Uncaught Error: https://svelte.dev/e/each_key_duplicate`。桌面端文件本身能打开/加载/高亮，说明 bug 在「列表 keying / tab 状态」而非基础加载。
- **根因链**：
  1. `Explorer.handleFileSelect`（`Explorer.svelte:349`）对文件的**普通单击**就会 `void fileEditorStore.openFile(path)`。
  2. 快速双击（用户「打开文件」的常见操作）会触发 `FileTree.handleClick` 两次 → 两次 `openFile(path)`。
  3. `openFile` 是异步的：两次调用都在 `await read_file_for_editor` 处挂起，**都在 push 之前**看到 `existing === undefined`（经典 TOCTOU）。
  4. 两次 `update()` 各自向 `openFiles` 追加一个**相同 `path`** 的 `OpenFile` → `openFiles` 出现重复 path。
  5. `dndItems` 同步 effect（`FileEditor.svelte:777`）用 `openFiles` 重建 `dndItems`，得到两个 `id === path` 相同的项。
  6. Svelte 5 keyed each 检测到重复 key → 抛 `each_key_duplicate`，丢弃/错渲列表项 → 表现为「tab 切不动 / 显示了错误文件的内容 / model 串台」。
- **修复（双层，防御纵深）**：
  1. **store 层（根治）** `fileEditor.ts:426`：把重复检查移进**原子 updater 内部**——`update((s) => { if (s.openFiles.some(f => f.path === path)) { 仅激活已存在 tab } else { 追加 } })`。updater 对最新 state 同步执行，彻底关闭 await 期间的竞态窗口，保证 `openFiles` 内 path 唯一这一不变量。
  2. **渲染层（防御）** `FileEditor.svelte:777`：`dndItems` 同步 effect 在读取 `openFiles` 后先按 path 去重（首次出现胜出）再派生 `paths`/`dndItems`，使重复 key 从渲染层不可达——即便未来某条新代码路径再次漏进重复 path，也不会崩 UI。
- **严重度判定**：CRITICAL —— 直接破坏「打开文件」这一核心流程，且会让用户看到错误文件内容、tab 失灵。

---

### finding-2【HIGH — 已修复】同一 tab 内 `setValue` 把光标/滚动弹回顶部

- **位置**：`src/lib/components/FileEditor.svelte:442`（model-swap effect 的 same-path 分支）。
- **根因**：当 store 里某个 tab 的 `content` 与 Monaco model 漂移（典型为外部 clean reconcile：文件在磁盘被外部修改、当前 tab 非 dirty、且用户正停在这个 tab 上），effect 执行 `editor.setValue(c.content)`。Monaco 的 `setValue` 会把**光标与滚动位置重置到第 1 行/顶部**，用户正在阅读的位置被弹走，体验割裂。
- **修复**：围绕 `setValue` 存/还原 view state：
  ```ts
  const vs = editor.saveViewState();
  editor.setValue(c.content);
  if (vs) editor.restoreViewState(vs);
  ```
  滚动与光标尽量留在原处（行号已不在新内容范围时 Monaco 会自行 clamp，安全）。
- **严重度判定**：HIGH —— 非数据损坏，但在「外部改文件 + 当前正看着」的常见场景下显著影响可用性。

---

## 静态核对通过、未发现缺陷的点（不改）

- **save 正确性**：`saveFile`（`fileEditor.ts:650`）`await invoke('write_file'|'apply_file_edits')`，错误经 `try/catch` → `alertDialog`，未吞错；保存成功后才置 `originalContent=content, isDirty=false, external=undefined`，dirty 清除时机正确。`markRecentlyWritten`（800ms 窗口，`fsEvents.ts:76`）抑制自写回环。Rust `write_file` 为 UTF-8 全量写 + 建父目录，错误向上传播；`apply_file_edits` 在 UTF-16 code unit 空间 splice，越界即报错让前端回退全量写——与 `computeSingleEdit` 的 UTF-16 语义一致。**无 lost-write / stale-content / 吞错。**
- **finding-2 邻近的 cached-model drift 分支**（`FileEditor.svelte:472` `model.setValue`）：该 `setValue` 发生在 `editor.setModel(model)`（:477）**之前**，目标 model 尚未挂到 editor 上，故**不会**触发 editor 的 `onDidChangeModelContent`（:393），不会把新文件内容误写进旧 path 的 store 项。最初怀疑的「跨 path 内容污染」**不成立**，不改。
- **语言检测**：`langFromPath`（`fileEditor.ts:217`）覆盖常见扩展名 + Dockerfile 特例，缺省 `plaintext`；`createModel` 对未知语言有 `try/catch` 回退 plaintext。无缺陷。
- **生命周期**：`onDestroy`（:327）dispose editor + 全部缓存 model + emptyModel + diff editor；GC effect（:496）在 tab 关闭时释放对应 model/viewState/markdownScroll/diffPair，且守卫 active model 不被提前 dispose。`automaticLayout: true` + 多处 `tick().then(layout)` 处理 hidden→visible。未见泄漏/0 高度。

---

## 待 GM live 确认（未强改，疑似项）

- **S-1（疑似 LOW/MEDIUM）model-swap effect 每次按键重跑**：effect（:418）track 了 `current`（store 的 derived），而 `updateContent` 每次按键都产生新的 `current` 引用 → effect 每键重跑。same-path 分支里 `editor.getValue() === c.content`（监听器已先写回 store），不会触发 `setValue`，**无正确性问题**，但有微小无谓开销。是否优化取决于大文件下是否有可感卡顿——建议 GM 在大文件连续输入时观察，否则按 YAGNI 不动。
- **S-2（疑似 LOW）`emptyModel` 单例可被编辑**：无活动文件/image/diff 时 editor 指向共享 `emptyModel`（:435）。正常流程下这些态不可编辑（被 preview/image/diff 层覆盖），暂未发现可在空白 model 上输入再串到其它 tab 的路径；若 GM 能在「无 tab 但 Monaco 可见」态打字，请反馈。

---

## 验证

- `pnpm check`（svelte-check）：见会话末尾运行结果（要求保持 0 errors / 0 warnings）。
- 无既有 `fileEditor` 单测（`**/*fileEditor*.{test,spec}.ts` 为空），未新增测试以免超出「最小外科修改」范围；本轮修复均为 store 不变量与渲染防御，行为可由 GM live 复测（双击/快速重复打开同一文件 → 不再抛 `each_key_duplicate`，tab 唯一）。
