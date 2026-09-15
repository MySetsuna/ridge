# B 代际保护证据（Goal #1）

**Date**: 2026-09-15
**改了什么**: `src/lib/terminal/ptyWriteQueue.ts` + `src/lib/components/RidgePane.svelte`

## 旧问题

`enqueuePtyInput(key, data, write, opts)` 按 `key = ${workspaceId}:${paneId}` 在模块级 Map 中持有 lane。
- Svelte 组件 unmount→remount：旧实例的 `write` 闭包不再被调用，但若旧实例已 enqueue 但未 drain 的字节仍挂在 lane 上；
- 新实例 enqueue 看到旧 lane 还在（`inputLanes.get(key)` 命中），把新字节 **追加** 到旧 lane.queued；
- 旧 lane 后续被 drain 时，**旧挂起字节 + 新字节** 一同发给（可能已换 workspace 的）PTY。

旧 `ptyWriteQueue` 的 `lanes`（write queue）有 generation guard，但 `inputLanes`（input coalesce）没有 — 同样的 unmount→remount race 漏在 input 侧。

## 修复

### 1. `enqueuePtyInput` 新增 `mountToken` 参数

```ts
export function enqueuePtyInput(
  key, data, write,
  options: { ..., mountToken?: unknown } = {},
): boolean
```

检测 `mountToken` 与 lane 上记录的不一致 → 旧 lane + queued bytes 全部丢弃：
```ts
const tokensDiffer = (incomingToken === undefined && prevToken === undefined)
  ? (lane.write !== write || lane.onError !== options.onError)  // 旧兼容路径
  : incomingToken !== prevToken;
if (tokensDiffer) {
  if (lane.flushTimer !== null) clearTimeout(lane.flushTimer);
  inputLanes.delete(key);
  lane = undefined;
}
```

> 设计：`mountToken` 是「代次」标识，**不**依赖 `write` 闭包身份。RidgePane 每次 keystroke 都新建 `(data) => invoke(...)` 箭头函数；用闭包身份当信号会把合法连击误判成 remount。改用显式不透明 token（组件实例对象 / 计数器 / mount 闭包引用）才能精确表达「哪一代实例」。

### 2. `drainPtyInput` 每次迭代现读 `lane.write`

旧实现 `drainPtyInput(key, lane, write, onError)` 在启动时快照 `write`；新代码每次 `while` 迭代现读 `lane.write`：
- 同实例内 `enqueuePtyInput` 多次 enqueue 新闭包 → drain 后续迭代用最新闭包；
- mountToken 变 → 旧 lane 已被新闭包分支删 → `inputLanes.get(key) !== lane` 跳出 while。

### 3. `RidgePane.svelte` 加 `mountInstance`

```ts
// 每个 mount 实例分配独立对象身份 `mountInstance`；unmount→remount
// 拿到新对象，旧 mountInstance 不再匹配 → ptyWriteQueue 立即清理旧 lane 与挂起
// 字节。注意：alive 标志不够——同一 paneId 多次 mount 都把 alive 翻成 true，
// boolean 不行，必须用对象身份。
const mountInstance = { paneId };
```

`onPtyData` 调用 `enqueuePtyInput` 时把 `mountInstance` 当 `mountToken` 传入。

## 测试

`src/lib/terminal/ptyWriteQueue.test.ts` 新增 5 个 B-protect 用例，全部通过：

```
✓ A→B→A: mountToken 变化 → 旧 lane 清空，新 lane 独立
✓ A→A: mountToken 不变 → 同一 lane 合并，bytes 保留
✓ 旧响应晚到：drain 期间 mountToken 变 → 旧 bytes 不再 flush 到新闭包
✓ 重连同时切换：retirePtyWriteQueuesForPane 硬清，再 enqueue → 旧挂起清空
✓ write 闭包身份变化但 mountToken 相同 → 不触发 generation guard（合法快键）
```

总计：`Test Files 1 passed (1) | Tests 12 passed (12)`

## 不动的部分

- `manager.park(paneId)` 仍保留 dataHandler / kernel / resizeHandler（transient unmount 场景，正确）
- `manager.detach(paneId)` 仍由 `cleanupPaneRuntime` → `retirePtyWriteQueuesForPane` 触发（real close 场景）
- `lanes`（write queue）的 generation guard 不动（已存在）
- `enqueuePtyWrite` 的 lanes.generation 检查不动（已存在）
- `paneRpcScheduler.retireScope` / `prune` 不动（已正确按 scope 清 lane + cancel in-flight）

## 已知局限

- 在 flight 的 Tauri `invoke('write_to_pty', ...)` 调用**无法**取消（已出 enqueue 队列、进了 Tauri IPC）。旧 bytes 仍会到 shell；shell 响应回 kernel/wasm/canvas 也走同一 paneId，渲染给当前 mount 的组件。这是 IPC 不可撤回性质，不是 queue bug。
- `mountToken` 由调用方提供；若调用方传 `undefined` 则退化为旧「write 闭包身份」行为，足够防御最常见的闭包重建场景但不防显式代次变更。
