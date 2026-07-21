# Cloud Remote 真机生命周期 smoke 设计

日期：2026-07-21  
状态：Iteration 4 实施基线

## 目标

以最小自动前置和可复核人工证据，判断 Cloud Remote 在 iOS Safari、Android Chrome 的真实换网、后台冻结和 token 跨窗后能否恢复正确 pane。自动测试证明状态机升级顺序，真机执行证明浏览器与系统网络栈行为。

## 自动前置设计

复用 `faultInjection.test.ts` 的真实 `ControllerCloudProvider → CloudWebrtcAdapter → RpcClient` 夹具，不新增生产 seam：

1. 短抖动：RTC 进入 `disconnected`，fake timer 推进 14,999 ms 后恢复。断言没有 ICE restart offer、PC/WS 数量不变、未触发 hello/pane recovery；恢复后既有授权仍有效且无 timer。
2. 升级链：RTC 保持 `disconnected` 15,000 ms，watchdog 排定首次 1,000 ms 退避；推进后同 PC 发一次 ICE restart。再保持未恢复 12,000 ms，deadline 触发整体重建。新 DC 完成 E2EE 但未授权时不得恢复业务；TOTP 授权后 hello/pane recovery 各一次，旧资源关闭且无 timer。

所有时间均用 fake timers，`Math.random=0`；不修改 15s/12s 生产常量，不用随机网络。

## 真机 runbook

文档分为准备、基线、换网、后台跨窗、恢复正确性和收尾六段。每一步都规定开始/结束时间戳、期望状态、失败即停条件和对应 evidence 字段。iOS 与 Android 各生成独立 JSON，不允许用一个平台代替另一个。

## 证据格式

仓库提供 JSON Schema、脱敏示例和无依赖 Node 校验器：

- Schema 检查 commit/build、平台、设备、OS、浏览器、场景、恢复耗时、结果和附件。
- 校验器除结构外检查附件路径存在，拒绝 evidence 中出现 `token`、`jwt`、`totp`、`password`、`secret` 等键。
- 实际 evidence 默认写到 gitignored 的 `artifacts/remote-smoke/`；只提交 schema/example，不提交真实设备证据。

## 边界

- 不增加产品运行时遥测、Evidence Capture UI、RPC、后端存储或用户标识。
- 不实现 Not-ready 请求排队、Agent roster 或新 watchdog。
- 自动前置失败则停止真机 smoke，先修单一状态机缺陷。
- 真机失败保留原始脱敏证据，不以放宽恢复阈值改成通过。

## 验收

- 新 fault suite 和既有 6-file Cloud 回归 exit 0。
- 增量 Svelte checker 0 errors；若生产代码未变，不要求重复 bundle build。
- evidence example 通过校验；缺必填项、敏感键或不存在附件的 fixture 必须失败。
- 最终完成条件仍要求 iOS Safari、Android Chrome 各一份通过的真实 evidence；在此之前 iteration 4 只能标记“自动前置完成，待物理验收”。
