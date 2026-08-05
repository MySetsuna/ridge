# CONTRACT — iteration 3

日期：2026-07-21  
主题：Cloud Remote 确定性 fault-injection 门禁

## 唯一主目标

建立跨 `ControllerCloudProvider → CloudWebrtcAdapter → RpcClient → CloudHostBridge` 的确定性故障注入测试，使重连、重新授权、pane 恢复与背压自愈的关键不变量在本地和 CI 可重复判定；本轮目标是补证据门禁，不重写传输实现。

## 已确认前置

- 静态 checker 可用：`pnpm exec svelte-check --tsconfig ./tsconfig.json --incremental --output machine --threshold error` 已在 24.6 秒完成，70 files、0 errors、exit 0。
- 现有单层测试已覆盖 RpcClient 断线拒绝、resync hook、Cloud host 高水位丢帧/drain resync、provider fake-timer 重连；本轮复用这些 seam，补跨层时序。

## 范围

- 新增一个 Cloud Remote fault-injection 测试夹具，使用固定事件序列、fake timers、固定 pane id 与字节 fixture。
- 覆盖两类恢复路径：信令仍存活时的 ICE restart；信令与 RTC 同时失败时的 PC/DC + E2EE/授权重建。
- 把 host 背压丢帧、drain resync 与 controller 重订阅串成可判定的跨层不变量。
- 固定运行 100 次失败/恢复周期，检查请求、订阅、timer 与发送次数不会累积泄漏。
- 若现有边界无法注入，允许增加最小测试 seam；生产默认行为必须不变。

## 边界

- 不使用随机丢包百分比作为门禁，不依赖 Network Link Conditioner 或平台特有网络工具。
- 不新增 RPC、capability、协议字段、第三方依赖或产品遥测。
- 不做 WebRTC/SCTP/E2EE/重连算法重构；只有确定性测试先证明现有不变量失败时，才修对应单一根因。
- 不实现 Remote Agent 控制台、topology、HITL 或 pane-switch 新接口。
- 不修 Vite/PWA/rustfmt 历史警告；真机 Wi-Fi/4G 切换只作非阻断人工证据。

## 可验证验收

1. 在途 RPC 遇显式 RTC failure 后立即以 `RpcReconnectError` settle；旧连接迟到响应不能完成新请求。
2. 信令存活路径只触发一次 ICE restart，不重建 peer connection；恢复后 `$/hello` 与每个活动 pane 的订阅各执行一次。
3. 信令失效路径重建 PC/DC，重新完成 E2EE/授权门控后才恢复业务订阅；验证前业务帧保持拒绝。
4. `bufferedAmount` 超过 256 KiB 时 pane 增量被丢弃；drain 后每个受影响 pane 恰好一次 `resync_pane_raw`，未受影响 pane 为零次，禁止串 pane。
5. fake timers 下固定 100 个失败/恢复周期后：所有 Promise settled、pending request 为 0、无重复订阅、无残留重连 timer、发送/重建计数与周期数精确线性。
6. 以下命令均 exit 0：
   - `pnpm exec vitest run packages/remote/src/shared/cloud/faultInjection.test.ts --reporter=verbose`
   - `pnpm exec vitest run packages/remote/src/shared/cloud/controllerCloudProvider.test.ts packages/remote/src/shared/cloud/cloudHostBridge.test.ts packages/remote/src/shared/transport/cloudWebrtcAdapter.test.ts packages/remote/src/shared/transport/rpcClient.test.ts packages/remote/src/shared/transport/conformance.test.ts`
   - `pnpm exec svelte-check --tsconfig ./tsconfig.json --incremental --output machine --threshold error`
   - 若改生产代码：`node node_modules\\vite\\bin\\vite.js build --config vite.remote.config.js`

## 停机条件

- 若测试需要新增 RPC、协议字段、产品遥测或外部网络模拟器，停止并另起设计。
- 若发现串 pane、错误历史或安全握手回归，立即收窄为单一缺陷修复，并保留触发序列作为回归测试。
- 固定故障矩阵与 100 周期全绿即收尾；不为“继续找问题”扩大成传输重构。
- 若 100 周期导致测试本身超出 120 秒，先证明是夹具开销并缩短单周期 fixture；不得以减少不变量断言掩盖生产泄漏。
