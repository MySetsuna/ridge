# CONTRACT — iteration 4

日期：2026-07-21  
主题：Cloud Remote 双平台真机生命周期 smoke

## 唯一主目标

取得 iOS Safari 与 Android Chrome 在真实 Cloud Remote 换网、切后台和 token 跨窗恢复场景下的可复核物理证据；自动测试只补齐进入真机验收前的 watchdog/deadline 状态机门禁，不以模拟证据替代真机结论。

## 短自动前置

- 在现有 `faultInjection.test.ts` 增加两条真实生产类时序：
  1. RTC `disconnected` 未满 15 秒即恢复：不创建 ICE restart offer、不重建 PC/WS、不重复 hello/pane recovery，timer 清零。
  2. RTC `disconnected` 满 15 秒触发退避，ICE restart 后 12 秒仍未恢复：升级整体重建；旧 PC/DC/WS 关闭，新连接重新 E2EE + TOTP 后 hello/pane recovery 恰好一次，timer 清零。
- 不新增 watchdog/deadline 实现；它们已存在于 `ControllerCloudProvider`。

## 文档与证据

- 新建聚焦的 Cloud Remote physical smoke runbook，不复用含过宽/陈旧项的旧 checklist。
- 提供 evidence JSON Schema、示例与本地校验脚本；记录 commit/build、设备/OS/浏览器、网络转换、后台时长、恢复耗时、结果及脱敏截图/日志路径。
- evidence 默认放仓外或 gitignored 目录；模板不得包含 JWT、TOTP、账号、完整设备域名或其它凭据。
- 不新增产品 Evidence Capture 开关、运行时遥测、RPC、后端存储、用户标识或隐私授权。

## 自动验收

```powershell
pnpm exec vitest run packages/remote/src/shared/cloud/faultInjection.test.ts --reporter=verbose
pnpm exec vitest run packages/remote/src/shared/cloud/controllerCloudProvider.test.ts packages/remote/src/shared/cloud/cloudHostBridge.test.ts packages/remote/src/shared/transport/cloudWebrtcAdapter.test.ts packages/remote/src/shared/transport/rpcClient.test.ts packages/remote/src/shared/transport/conformance.test.ts
pnpm exec svelte-check --tsconfig ./tsconfig.json --incremental --output machine --threshold error
```

每份人工 evidence JSON 还须通过 schema 校验，必填字段齐全，引用的本地附件存在。

## 必须人工验收

iOS Safari 与 Android Chrome 各执行一次：

1. 基线连接、TOTP/信任授权、终端输入回显正常。
2. Wi-Fi → 蜂窝 → Wi-Fi，无手动刷新恢复。
3. 切后台至少跨过 15 分钟 token 窗口，再回前台。
4. 恢复后 pane、scrollback、输入和 capability UI 正常，无黑屏、串 pane 或重复订阅迹象。
5. 记录每阶段恢复耗时；建议目标 ≤45 秒，超出按失败证据记录。

## 停机条件

- 自动前置任一失败：停止真机 smoke，先收窄并修复首个状态机缺陷。
- 基线连接失败：停止后续换网/后台场景，只记录首因。
- 仅完成一个平台：不得宣称双平台通过。
- 发现可复现产品缺陷：本轮转为单缺陷复现与修复，不扩遥测、不夹带 Agent roster。
- 证据采集若要求新增 RPC、后端存储、用户标识或隐私授权，立即停止并另起设计。

## 明确不做

- 不实现 `RpcNotReadyError`、请求排队或自动重放写操作。
- 不实现 Remote Agent roster/topology 或 pane-switch 新接口。
- 不新增 watchdog/deadline、随机丢包模拟、产品 RTT/丢包遥测。
- 不以自动测试或单平台结果代替双平台物理结论。
