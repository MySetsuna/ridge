# 移动端远控:切 Pane 后切回 scrollback 丢失 — 修复设计

日期: 2026-06-25
范围: **纯客户端 `src/remote`**(不动 host `src-tauri`,不动 64KB replay 上限)
关联: commit 4062eef(引入 `pruneDeadPanes` 修内存泄漏)

## 问题

移动端远控(`src/remote`)用**单一共享终端内核** + **按 pane 缓存原始字节流**
(`paneBuffers`,每 pane ≤256KB)+ sessionStorage 镜像。切 pane 时
`resetForSwitch()` 清内核 → 用缓存即时预绘 → re-subscribe,host 在 subscribe 时
replay ≤64KB scrollback → 在 `onRawBytes` 用 `bytesEndsWith` tail-match 去重。

**现象:切到另一个 pane(尤其跨工作区)再切回,终端历史回滚(scrollback)丢失。**

### 根因(读码确认)

**缺陷1(主因)——跨工作区切换缓存被 GC 误删。**
`pruneDeadPanes(liveIds)`(`MainApp.svelte`)删除所有不在 `liveIds` 里的 pane 缓存
(含内存 `paneBuffers`、sessionStorage 镜像、以及 `ws.pruneOutputs`)。它由 `panes`
消息处理器调用,`liveIds = 当前这条 panes 列表的 id`。但 host 的 `list-panes`
(`server.rs` ~1604–1615,`workspaces.get(&active_ws_id)`)**只返回当前 active 工作区
的 pane**。→ 切到工作区 B 时,新 `panes` 列表只含 B 的 pane,不含工作区 A 的 pane →
A 的 256KB 缓存被 prune 删掉 → 切回 A 时只剩 host ≤64KB replay → scrollback 丢。

GC 不能直接删:它是为修内存泄漏(commit 4062eef)加的——已关闭的 pane 必须释放缓存,
否则长跑(PWA 标签页存活数天)会把每个开过的终端的 scrollback 永久泄漏到内存 +
sessionStorage,最终页面打不开。

**缺陷2(次因)——对账反噬把缓存缩短。**
`onRawBytes` replay 分支(~447–450):`bytesEndsWith(cached, replay)` 为 false 时
`resetForSwitch()` 清内核并把 256KB 缓存**替换**成 ≤64KB replay,即使本地缓存更长更全。
tail 不完全匹配(常见:期间有新输出、或缓存与 replay 边界不对齐)就退化成 64KB。

## 客户端已具备的能力(关键发现)

- `ws.listWorkspaces()` 返回**所有**工作区。
- `ws.listWorkspacePanes(wsId)`(host `list-workspace-panes`,`server.rs` ~2148)
  可只读列出**任意**工作区的 pane,**不切换** active 工作区。
- `WorkspaceTree.svelte` 已用 `peekedPanes: Map<wsId, PaneInfo[]>` + `fetchPeek`
  缓存各非 active 工作区的 pane 列表。

即:客户端**有办法**知道全部工作区的全部 pane —— 子方案 A 可行;但下面选 B。

## 方案1:修过度 prune — 子方案取舍

硬约束:(a) 跨工作区切回**绝不**丢缓存;(b) 真正关闭的 pane 仍被 GC。

### 子方案 A — 跨工作区全量 live-set(被否)

prune 前对每个工作区发 `list-workspace-panes`,把全部 pane 并成存活集。
- 优点:存活判定权威完整;真正关闭(不在任何工作区)的 pane 立即 GC。
- 缺点:每次 `panes` 回包要 N 个工作区各一次往返(N 次 round-trip),引入跨工作区
  时序耦合;弱网下放大延迟与 DataChannel 压力。复用 `peekedPanes` 只能近似(可能过期)。

### 子方案 B — 仅删"当前工作区内真正消失"的 pane(**采用**)

不再用"不在 active 列表里"当死亡判据。改为:
- 维护 `paneWorkspace: Map<paneId, wsId>` —— 每个**缓存中**的 pane 属于哪个工作区。
- 每条 `panes` 回包(其 id 全属当前 active 工作区):只删除
  **"`paneWorkspace` 标记为当前 active 工作区、但已不在这条新列表里"** 的 pane ——
  即在当前工作区内被真正关闭的 pane。**其他工作区的 pane 缓存一律保留。**
- 把新列表里的 pane 全部(重新)标记为属于 active 工作区。
- **工作区收缩兜底:** 当 `workspaces` 列表里某工作区消失(被 `closeWorkspace`),
  删除 `paneWorkspace` 指向该已消失工作区的所有 pane 缓存。

为何 B 安全且足够:
- 跨工作区切回不丢:工作区 A 的 pane 不在 B 的 panes 列表里,但它们标记为 A 而非 B,
  prune 当前(B)工作区时不碰它们。(主约束 a ✓)
- 真正关闭的 pane 仍 GC:移动端**只能关当前工作区的 pane**
  (`WorkspaceTree.closePaneRow` 的关闭按钮仅在 `isActiveWs` 时渲染),所以 pane 关闭
  必然发生在 active 工作区内,被"当前工作区内消失"分支覆盖。(约束 b ✓)
- 关闭整个工作区:其 pane 不再出现在任何工作区,由工作区收缩兜底清理。(约束 b ✓)

代价/上界:缓存 = 所有访问过且仍存活的 pane,每个 ≤256KB。正常使用(几个工作区 ×
每区几个 pane)完全可控。两条真实释放路径(关 pane / 关工作区)都覆盖,无泄漏回归。
相比 A:主路径**零额外往返、零时序耦合**,纯本地状态,弱网无额外负担。

### `pruneOutputs` 一致性

`ws.pruneOutputs(liveIds)` 同样按"不在集合里就删"工作,旧实现传 active-only 集合
会误删跨工作区 output 缓冲。新实现 prune 时传入**所有应保留的 pane id**
(= 所有 `paneWorkspace` 里仍存活的 pane,差集删除后剩下的)给 `pruneOutputs`,
保持与 `paneBuffers`/sessionStorage 同一存活语义。

## 方案2:对账不缩短

`onRawBytes` 的 replay 分支,把"是否保留缓存"的判定从单一 tail-match 改为:
- **缓存为空/不存在** → reset + 用 replay 重绘(权威),写入缓存。(pane 首次/缓存丢失)
- **`bytesEndsWith(cached, replay)`**(缓存尾部即 replay,现状保留条件) → 保留缓存,
  丢弃 replay(已预绘)。
- **缓存显著长于 replay**(`cached.length > replay.length`,replay 是 ≤64KB 尾部、
  缓存 ≤256KB) → **保留缓存、丢弃 replay**,**不**再 `resetForSwitch()` 截短。
  本地有更全历史,host 的 64KB 尾巴不该覆盖它;预绘已把缓存画上,直接放行 live 流。
- **其余**(缓存存在但不长于 replay 且 tail 不匹配,说明 pane 确实变了/缓存更短) →
  reset + 用 replay 重绘,写入缓存。

净效果:**只有当 replay 带来的信息确实多于本地缓存时才 reset+repaint;否则保留 256KB
缓存。** 杜绝"对账把 256KB 截成 64KB"。

## 可测纯逻辑抽取

把缓存/prune/对账的**纯决策**抽到 `src/remote/lib/paneScrollbackCache.ts`,
不依赖 DOM 全局(sessionStorage/btoa 等留在 `MainApp.svelte` 的薄壳层,通过现有
`loadPaneFromSession`/`scheduleSessionMirror` 调用)。模块导出:

- `class PaneScrollbackCache`
  - `buffers: Map<paneId, Uint8Array>`、`paneWorkspace: Map<paneId, wsId>`、`cap`。
  - `append(paneId, data)`:追加并裁到 `cap`(等价现 `appendPaneBuffer`)。
  - `set(paneId, data)` / `get(paneId)` / `has`。
  - `pruneCurrentWorkspace(activeWsId, livePaneIds): { survivingIds: string[] }`
    —— 子方案 B 的差集删除 + 重标记;返回应交给 `pruneOutputs` 的存活 id 集。
  - `pruneClosedWorkspaces(liveWorkspaceIds): string[]` —— 工作区收缩兜底,返回被删
    pane id(供 sessionStorage 同步清理)。
  - `reconcileReplay(paneId, replay): { action: 'keep' | 'repaint'; buffer: Uint8Array }`
    —— 方案2 的纯决策:`keep`=保留缓存丢 replay;`repaint`=reset 后用返回 buffer 重绘
    并写回缓存。
- 纯函数 `bytesEndsWith(hay, tail)`(从 `MainApp.svelte` 提出,供模块与测试共用)。

`MainApp.svelte` 改为持有一个 `PaneScrollbackCache` 实例,`pruneDeadPanes`/
`appendPaneBuffer`/`onRawBytes` replay 分支转调它;sessionStorage 镜像与删除仍在壳层
按返回的 id 集合执行。

## 测试(TDD,vitest,node 环境)

新增 `src/remote/lib/paneScrollbackCache.test.ts`,纯逻辑、零 DOM/host:
1. **方案1 跨工作区切回不丢:** A 有缓存 → 切到 B(`pruneCurrentWorkspace('B', [b1,b2])`)
   → A 的缓存仍在;返回的存活集含 A、B 的 pane。
2. **方案1 关闭当前工作区 pane 仍 GC:** active=A 含 a1,a2 → `pruneCurrentWorkspace('A',[a1])`
   → a2 缓存被删,a1 保留。
3. **方案1 工作区收缩兜底:** A、B 各有缓存 → `pruneClosedWorkspaces(['B'])`(A 被关)
   → A 的 pane 缓存全删,返回其 id;B 保留。
4. **方案2 缓存长于 replay → keep:** cached 256KB,replay 64KB 尾巴不 tail-match →
   `reconcileReplay` 返回 `keep`,缓存不变。
5. **方案2 tail-match → keep**(现状保留条件回归)。
6. **方案2 缓存为空 → repaint** 用 replay。
7. **方案2 缓存短于 replay 且不匹配(pane 变了)→ repaint** 用 replay。
8. `bytesEndsWith` 边界(空 tail、tail 长于 hay、相等)。
9. `append` 裁到 `cap`。

## 提交粒度

- commit 1:设计文档(本文件)。
- commit 2:方案1(`paneScrollbackCache` 模块 + prune 跨工作区修复 + 接入 + 对应测试)。
- commit 3:方案2(对账不缩短 + 对应测试)。

(方案1、方案2 各自单独 commit;不写手动测试清单。)
