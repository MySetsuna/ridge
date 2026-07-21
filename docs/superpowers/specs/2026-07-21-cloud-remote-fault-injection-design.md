# Cloud Remote 确定性故障注入设计

日期：2026-07-21  
状态：Iteration 3 实施基线

## 目标

在不依赖真实网络、随机丢包或平台网络工具的条件下，为 Cloud Remote 的断线、重连、重新授权、pane 恢复与背压自愈建立可重复门禁。测试须覆盖真实生产类之间的状态传播，而非再增加互不相连的单元断言。

## 现状与缺口

- `ControllerCloudProvider` 已实现 RTC failure 调度、信令存活时 ICE restart、信令失效时整体重建，以及重连 timer/watchdog。
- `CloudWebrtcAdapter` 已把 provider 状态和 `0x11/0x12/0x10` 帧映射到统一 transport，并在断线时把授权状态重置为 `pending`。
- `RpcClient` 已在连接离开 `connected` 时拒绝在途请求，并在恢复边沿重新发送 `$/hello`、执行 resync hook。
- `CloudHostBridge` 已实现 TOTP 业务门控、pane 订阅幂等、高水位丢帧和 drain 后按 pane 重同步。
- 现有测试分别验证上述行为，但缺少同一故障时序下的跨层计数、顺序和泄漏断言。

## 方案

新增 `packages/remote/src/shared/cloud/faultInjection.test.ts`，包含两个互补夹具：

1. Controller 夹具使用项目现有 WebSocket、RTCPeerConnection、RTCDataChannel mock，实例化真实 `ControllerCloudProvider`、`CloudWebrtcAdapter` 和 `RpcClient`。夹具只提供显式事件：RTC failed、信令 close、RTC connected、授权结果、控制响应；不复制生产状态机。
2. Host 夹具实例化真实 `CloudHostBridge`，注入可调 `bufferedAmount`、drain 回调、固定 pane source 和 invoke spy，验证受影响 pane 集合与重同步次数。

测试时间统一由 Vitest fake timers 管理；`Math.random` 固定为零，使首次退避严格为 1000 ms。每个测试在 `afterEach` 恢复 timer/random/global mock，避免跨测试污染。

## 故障矩阵

### RTC failure，信令存活

- 先建立 connected 状态并发出一个永不响应的 RPC。
- 触发旧 PC `connectionState='failed'`。
- 断言 RPC 立即以 `RpcReconnectError` 结束，`inFlight===0`。
- 推进首次退避，仅旧 PC 收到一次 `createOffer({iceRestart:true})`；PC/WS 数量不变。
- 模拟旧 PC 恢复 connected，断言 `$/hello` 与每个 resync hook 各一次，无第二个 reconnect timer。

### RTC failure，信令同时失效

- 触发 WS close 与 RTC failed，再推进退避。
- 断言创建新 PC/DC/WS；旧资源关闭，ICE restart 不发生。
- 在 E2EE/绑定/授权完成前，不允许业务订阅帧通过。
- 完成握手与授权后，恢复 `$/hello` 和 pane 订阅各一次。

### Host 背压

- A、B pane 在 `bufferedAmount > 8 MiB` 时各丢多次增量，C pane 不受影响。
- 第一次 drain 仅对 A、B 各调用一次 `resync_pane_raw`；重复 drain 不再调用。
- 新一轮只让 A 受压，下一次 drain 仅增加 A 一次，证明集合按周期清空且不串 pane。

### 100 周期稳定性

- 使用同一 Controller 夹具依次执行 100 次“failed → 退避 → connected”。
- 每周期只保留一个 active pane resync hook；断言 hello、订阅、ICE restart 数量均与周期数线性一致。
- 每周期均断言 `inFlight===0`，最终 `vi.getTimerCount()===0`。
- 若真实状态机要求一次故障只能 ICE restart 一次，则每周期完整恢复后再开始下一周期，不绕过生产字段。

## 最小实现边界

- 首选只新增测试文件。
- 若 private 状态使关键泄漏无法观测，只允许增加只读诊断 getter 或构造注入的时钟/随机源；默认生产行为不变。
- 不新增 RPC、协议字段、capability、遥测、依赖或网络模拟器。
- 不在本轮修复 Vite/PWA/rustfmt 历史问题，不实现 Remote Agent 控制台。

## 验收

先运行新测试取得可解释的红灯；若失败揭示生产缺陷，保留最小复现并只修该根因。最终执行：

```powershell
pnpm exec vitest run packages/remote/src/shared/cloud/faultInjection.test.ts --reporter=verbose
pnpm exec vitest run packages/remote/src/shared/cloud/controllerCloudProvider.test.ts packages/remote/src/shared/cloud/cloudHostBridge.test.ts packages/remote/src/shared/transport/cloudWebrtcAdapter.test.ts packages/remote/src/shared/transport/rpcClient.test.ts packages/remote/src/shared/transport/conformance.test.ts
pnpm exec svelte-check --tsconfig ./tsconfig.json --incremental --output machine --threshold error
```

若修改生产代码，再执行 Remote Vite build。测试必须在 120 秒内完成，且退出后无残留 timer、pending RPC 或未恢复的全局 mock。
